// SPDX-License-Identifier: MIT OR Apache-2.0

//! Validation pipeline for dialect mapping correctness.
//!
//! Provides the [`MappingValidator`](crate::validation::MappingValidator) trait, a
//! [`DefaultMappingValidator`](crate::validation::DefaultMappingValidator)
//! implementation, and a [`ValidationPipeline`](crate::validation::ValidationPipeline)
//! that chains pre-validate → map → post-validate in a single pass.

use std::collections::BTreeSet;

use abp_core::ir::{IrContentBlock, IrConversation, IrRole};
use abp_dialect::Dialect;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capabilities::dialect_capabilities;

// ── Required fields per dialect ─────────────────────────────────────────

/// Returns the required top-level fields for a given dialect's request.
fn required_fields(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::OpenAi => &["model", "messages"],
        Dialect::Claude => &["model", "messages", "max_tokens"],
        Dialect::Gemini => &["model", "contents"],
        Dialect::Codex => &["model", "messages"],
        Dialect::Kimi => &["model", "messages"],
        Dialect::Copilot => &["model", "messages"],
    }
}

// ── ValidationIssue ─────────────────────────────────────────────────────

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    /// Informational note.
    Info,
    /// Warning — valid but potentially incorrect.
    Warning,
    /// Error — violates a required constraint.
    Error,
}

impl std::fmt::Display for ValidationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => f.write_str("info"),
            Self::Warning => f.write_str("warning"),
            Self::Error => f.write_str("error"),
        }
    }
}

/// A single issue discovered during mapping validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Severity of this issue.
    pub severity: ValidationSeverity,
    /// JSON-pointer-style path to the problematic field.
    pub field: String,
    /// Human-readable description.
    pub message: String,
    /// Machine-readable issue code.
    pub code: String,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {} ({})",
            self.severity, self.field, self.message, self.code
        )
    }
}

// ── ValidationResult ────────────────────────────────────────────────────

/// Result of pre- or post-mapping validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the value passed validation (no error-level issues).
    pub valid: bool,
    /// All issues discovered.
    pub issues: Vec<ValidationIssue>,
    /// Percentage of required fields present (`0.0..=100.0`).
    pub field_coverage: f64,
}

impl ValidationResult {
    /// Returns `true` when no error-level issues exist.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Number of error-level issues.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Error)
            .count()
    }

    /// Number of warning-level issues.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Warning)
            .count()
    }
}

// ── RoundtripResult ─────────────────────────────────────────────────────

/// Result of roundtrip validation (A → B → A).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundtripResult {
    /// Whether the original and roundtripped values are structurally equivalent.
    pub equivalent: bool,
    /// Fields present in the original but absent after roundtrip.
    pub lost_fields: Vec<String>,
    /// Fields absent in the original but present after roundtrip.
    pub added_fields: Vec<String>,
    /// Fields present in both but with different values.
    pub changed_fields: Vec<String>,
}

impl RoundtripResult {
    /// Returns `true` when no fields were lost, added, or changed.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.equivalent
    }
}

// ── MappingValidator trait ──────────────────────────────────────────────

/// Trait for validating mapping correctness before and after dialect translation.
pub trait MappingValidator: Send + Sync {
    /// Validate a request *before* it is mapped from `source` dialect.
    fn validate_pre_mapping(
        &self,
        source: Dialect,
        request: &serde_json::Value,
    ) -> ValidationResult;

    /// Validate a mapped value *after* translation to `target` dialect.
    fn validate_post_mapping(
        &self,
        target: Dialect,
        mapped: &serde_json::Value,
    ) -> ValidationResult;

    /// Compare the original request with its roundtripped version.
    fn validate_roundtrip(
        &self,
        original: &serde_json::Value,
        roundtripped: &serde_json::Value,
    ) -> RoundtripResult;
}

// ── DefaultMappingValidator ─────────────────────────────────────────────

/// Default implementation of [`MappingValidator`].
///
/// - Pre-mapping: checks required fields for the source dialect.
/// - Post-mapping: checks required fields for the target dialect.
/// - Roundtrip: computes a structural JSON diff (lost / added / changed).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefaultMappingValidator {
    _priv: (),
}

impl DefaultMappingValidator {
    /// Create a new default validator.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl MappingValidator for DefaultMappingValidator {
    fn validate_pre_mapping(&self, source: Dialect, request: &Value) -> ValidationResult {
        validate_dialect_fields(source, request)
    }

    fn validate_post_mapping(&self, target: Dialect, mapped: &Value) -> ValidationResult {
        validate_dialect_fields(target, mapped)
    }

    fn validate_roundtrip(&self, original: &Value, roundtripped: &Value) -> RoundtripResult {
        structural_diff(original, roundtripped)
    }
}

/// Validates that a JSON value contains the required fields for a dialect.
fn validate_dialect_fields(dialect: Dialect, value: &Value) -> ValidationResult {
    let mut issues = Vec::new();
    let required = required_fields(dialect);

    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                field: String::new(),
                message: "expected a JSON object".into(),
                code: "invalid_type".into(),
            });
            return ValidationResult {
                valid: false,
                issues,
                field_coverage: 0.0,
            };
        }
    };

    let mut present = 0usize;
    for &field in required {
        if obj.contains_key(field) {
            present += 1;
        } else {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                field: field.into(),
                message: format!("missing required field \"{field}\" for {}", dialect.label()),
                code: "missing_required_field".into(),
            });
        }
    }

    // Warn on empty messages/contents arrays.
    let messages_key = if dialect == Dialect::Gemini {
        "contents"
    } else {
        "messages"
    };
    if let Some(Value::Array(arr)) = obj.get(messages_key) {
        if arr.is_empty() {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Warning,
                field: messages_key.into(),
                message: format!("\"{messages_key}\" array is empty"),
                code: "empty_messages".into(),
            });
        }
    }

    let coverage = if required.is_empty() {
        100.0
    } else {
        (present as f64 / required.len() as f64) * 100.0
    };

    let valid = !issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error);

    ValidationResult {
        valid,
        issues,
        field_coverage: coverage,
    }
}

// ── Structural diff ─────────────────────────────────────────────────────

/// Recursively collect all leaf key-paths from a JSON value.
fn collect_paths(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                collect_paths(v, &path, out);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let path = format!("{prefix}[{i}]");
                collect_paths(v, &path, out);
            }
        }
        _ => {
            out.insert(prefix.to_string());
        }
    }
}

/// Resolve a dotted/bracket path against a JSON value.
fn resolve_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    let mut remaining = path;

    while !remaining.is_empty() {
        if remaining.starts_with('[') {
            // Array index
            let end = remaining.find(']')?;
            let idx: usize = remaining[1..end].parse().ok()?;
            current = current.get(idx)?;
            remaining = &remaining[end + 1..];
            if remaining.starts_with('.') {
                remaining = &remaining[1..];
            }
        } else {
            let dot = remaining.find('.').unwrap_or(remaining.len());
            let bracket = remaining.find('[').unwrap_or(remaining.len());
            let end = dot.min(bracket);
            let key = &remaining[..end];
            current = current.get(key)?;
            remaining = &remaining[end..];
            if remaining.starts_with('.') {
                remaining = &remaining[1..];
            }
        }
    }

    Some(current)
}

/// Compute a structural diff between two JSON values.
fn structural_diff(original: &Value, roundtripped: &Value) -> RoundtripResult {
    let mut orig_paths = BTreeSet::new();
    let mut rt_paths = BTreeSet::new();

    collect_paths(original, "", &mut orig_paths);
    collect_paths(roundtripped, "", &mut rt_paths);

    let lost_fields: Vec<String> = orig_paths.difference(&rt_paths).cloned().collect();
    let added_fields: Vec<String> = rt_paths.difference(&orig_paths).cloned().collect();

    let mut changed_fields = Vec::new();
    for path in orig_paths.intersection(&rt_paths) {
        let ov = resolve_path(original, path);
        let rv = resolve_path(roundtripped, path);
        if ov != rv {
            changed_fields.push(path.clone());
        }
    }

    let equivalent = lost_fields.is_empty() && added_fields.is_empty() && changed_fields.is_empty();

    RoundtripResult {
        equivalent,
        lost_fields,
        added_fields,
        changed_fields,
    }
}

// ── ValidationPipeline ──────────────────────────────────────────────────

/// Outcome of running a value through the [`ValidationPipeline`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    /// Pre-mapping validation result.
    pub pre: ValidationResult,
    /// The mapped JSON value (present only when pre-validation passed).
    pub mapped: Option<serde_json::Value>,
    /// Post-mapping validation result (present only when mapping succeeded).
    pub post: Option<ValidationResult>,
}

/// Chains pre-validate → map → post-validate into a single pass.
///
/// The mapping function is supplied at call-time so the pipeline is agnostic
/// to the concrete mapper implementation.
pub struct ValidationPipeline<V: MappingValidator> {
    validator: V,
    /// Source dialect of the request.
    pub source: Dialect,
    /// Target dialect after mapping.
    pub target: Dialect,
}

impl<V: MappingValidator> ValidationPipeline<V> {
    /// Create a new pipeline.
    pub fn new(validator: V, source: Dialect, target: Dialect) -> Self {
        Self {
            validator,
            source,
            target,
        }
    }

    /// Run the pipeline: pre-validate, map (via the supplied closure),
    /// then post-validate.
    ///
    /// If pre-validation fails (has error-level issues), the map function
    /// is **not** called and `mapped` / `post` are `None`.
    pub fn run<F>(&self, request: &Value, map_fn: F) -> PipelineResult
    where
        F: FnOnce(&Value) -> Result<Value, String>,
    {
        let pre = self.validator.validate_pre_mapping(self.source, request);

        if !pre.is_valid() {
            return PipelineResult {
                pre,
                mapped: None,
                post: None,
            };
        }

        match map_fn(request) {
            Ok(mapped) => {
                let post = self.validator.validate_post_mapping(self.target, &mapped);
                PipelineResult {
                    pre,
                    mapped: Some(mapped),
                    post: Some(post),
                }
            }
            Err(reason) => {
                // Mapping itself failed — report as a post-validation error.
                let post = ValidationResult {
                    valid: false,
                    issues: vec![ValidationIssue {
                        severity: ValidationSeverity::Error,
                        field: String::new(),
                        message: reason,
                        code: "mapping_failed".into(),
                    }],
                    field_coverage: 0.0,
                };
                PipelineResult {
                    pre,
                    mapped: None,
                    post: Some(post),
                }
            }
        }
    }

    /// Access the underlying validator.
    pub fn validator(&self) -> &V {
        &self.validator
    }
}

// ── MappingValidationIssue ──────────────────────────────────────────────

/// A single issue found during IR-level mapping validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingValidationIssue {
    /// Machine-readable issue code.
    pub code: String,
    /// Feature or field that triggered the issue.
    pub feature: String,
    /// Human-readable description.
    pub message: String,
}

impl std::fmt::Display for MappingValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.feature, self.message)
    }
}

// ── MappingValidationError ─────────────────────────────────────────────

/// Error returned when mapping validation fails.
///
/// Contains all issues found — the validator never short-circuits on
/// the first problem so that callers see the full picture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingValidationError {
    /// Source dialect of the attempted mapping.
    pub source_dialect: Dialect,
    /// Target dialect of the attempted mapping.
    pub target_dialect: Dialect,
    /// All issues found during validation.
    pub issues: Vec<MappingValidationIssue>,
}

impl std::fmt::Display for MappingValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mapping validation failed ({} -> {}): {} issue(s)",
            self.source_dialect.label(),
            self.target_dialect.label(),
            self.issues.len()
        )
    }
}

impl std::error::Error for MappingValidationError {}

impl MappingValidationError {
    /// Returns `true` when there are no issues.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Number of issues.
    #[must_use]
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// Check if a specific issue code is present.
    #[must_use]
    pub fn has_code(&self, code: &str) -> bool {
        self.issues.iter().any(|i| i.code == code)
    }
}

// ── validate_mapping ───────────────────────────────────────────────────

/// Validate that a mapping from one dialect to another can succeed for
/// the given IR request.
///
/// Checks all features used in the request against the target dialect's
/// capabilities, collecting **all** issues without short-circuiting.
///
/// Returns `Ok(())` when no issues are found, or `Err(MappingValidationError)`
/// with the full list of problems.
pub fn validate_mapping(
    from_dialect: Dialect,
    to_dialect: Dialect,
    request: &IrConversation,
) -> Result<(), MappingValidationError> {
    let mut issues = Vec::new();
    let to_caps = dialect_capabilities(to_dialect);

    // Check dialect pair support
    if crate::default_ir_mapper(from_dialect, to_dialect).is_none() {
        issues.push(MappingValidationIssue {
            code: "unsupported_dialect_pair".into(),
            feature: "dialect_pair".into(),
            message: format!(
                "no mapper available for {} -> {}",
                from_dialect.label(),
                to_dialect.label()
            ),
        });
    }

    // Check each message for unsupported features
    for (idx, msg) in request.messages.iter().enumerate() {
        // System prompt
        if msg.role == IrRole::System && !to_caps.system_prompt.is_native() {
            issues.push(MappingValidationIssue {
                code: "unsupported_system_prompt".into(),
                feature: "system_prompt".into(),
                message: format!(
                    "system message at index {} not supported by {}",
                    idx,
                    to_dialect.label()
                ),
            });
        }

        for block in &msg.content {
            match block {
                IrContentBlock::Thinking { .. } if !to_caps.thinking.is_native() => {
                    issues.push(MappingValidationIssue {
                        code: "unsupported_thinking".into(),
                        feature: "thinking".into(),
                        message: format!(
                            "thinking block at message {} not supported by {}",
                            idx,
                            to_dialect.label()
                        ),
                    });
                }
                IrContentBlock::Image { .. } if !to_caps.images.is_native() => {
                    issues.push(MappingValidationIssue {
                        code: "unsupported_image".into(),
                        feature: "images".into(),
                        message: format!(
                            "image block at message {} not supported by {}",
                            idx,
                            to_dialect.label()
                        ),
                    });
                }
                IrContentBlock::ToolUse { .. } if !to_caps.tool_use.is_native() => {
                    issues.push(MappingValidationIssue {
                        code: "unsupported_tool_use".into(),
                        feature: "tool_use".into(),
                        message: format!(
                            "tool use at message {} not supported by {}",
                            idx,
                            to_dialect.label()
                        ),
                    });
                }
                IrContentBlock::ToolResult { .. } if !to_caps.tool_use.is_native() => {
                    issues.push(MappingValidationIssue {
                        code: "unsupported_tool_result".into(),
                        feature: "tool_use".into(),
                        message: format!(
                            "tool result at message {} not supported by {}",
                            idx,
                            to_dialect.label()
                        ),
                    });
                }
                _ => {}
            }
        }
    }

    // Check for empty messages (structural issue)
    for (idx, msg) in request.messages.iter().enumerate() {
        if msg.content.is_empty() {
            issues.push(MappingValidationIssue {
                code: "empty_message".into(),
                feature: "structure".into(),
                message: format!("message at index {} has no content blocks", idx),
            });
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(MappingValidationError {
            source_dialect: from_dialect,
            target_dialect: to_dialect,
            issues,
        })
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn validator() -> DefaultMappingValidator {
        DefaultMappingValidator::new()
    }

    // -- ValidationSeverity --

    #[test]
    fn severity_display() {
        assert_eq!(ValidationSeverity::Info.to_string(), "info");
        assert_eq!(ValidationSeverity::Warning.to_string(), "warning");
        assert_eq!(ValidationSeverity::Error.to_string(), "error");
    }

    #[test]
    fn severity_serde_roundtrip() {
        let s = ValidationSeverity::Warning;
        let json = serde_json::to_string(&s).unwrap();
        let back: ValidationSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    // -- ValidationIssue --

    #[test]
    fn issue_display() {
        let issue = ValidationIssue {
            severity: ValidationSeverity::Error,
            field: "model".into(),
            message: "missing".into(),
            code: "missing_required_field".into(),
        };
        let s = format!("{issue}");
        assert!(s.contains("error"));
        assert!(s.contains("model"));
    }

    // -- DefaultMappingValidator pre-mapping --

    #[test]
    fn pre_mapping_valid_openai() {
        let v = validator();
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
        assert!(r.is_valid());
        assert_eq!(r.field_coverage, 100.0);
    }

    #[test]
    fn pre_mapping_missing_model() {
        let v = validator();
        let req = json!({"messages": [{"role": "user", "content": "hi"}]});
        let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.field == "model"));
    }

    #[test]
    fn pre_mapping_non_object() {
        let v = validator();
        let r = v.validate_pre_mapping(Dialect::OpenAi, &json!(42));
        assert!(!r.is_valid());
        assert_eq!(r.field_coverage, 0.0);
    }

    #[test]
    fn pre_mapping_claude_missing_max_tokens() {
        let v = validator();
        let req = json!({"model": "claude-3", "messages": []});
        let r = v.validate_pre_mapping(Dialect::Claude, &req);
        assert!(!r.is_valid());
        assert!(
            r.issues
                .iter()
                .any(|i| i.field == "max_tokens" && i.code == "missing_required_field")
        );
    }

    #[test]
    fn pre_mapping_gemini_requires_contents() {
        let v = validator();
        let req = json!({"model": "gemini-pro"});
        let r = v.validate_pre_mapping(Dialect::Gemini, &req);
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.field == "contents"));
    }

    // -- DefaultMappingValidator post-mapping --

    #[test]
    fn post_mapping_valid_claude() {
        let v = validator();
        let mapped = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 1024});
        let r = v.validate_post_mapping(Dialect::Claude, &mapped);
        assert!(r.is_valid());
        assert_eq!(r.field_coverage, 100.0);
    }

    #[test]
    fn post_mapping_missing_field() {
        let v = validator();
        let mapped = json!({"model": "gpt-4"});
        let r = v.validate_post_mapping(Dialect::OpenAi, &mapped);
        assert!(!r.is_valid());
    }

    // -- Roundtrip --

    #[test]
    fn roundtrip_identical() {
        let v = validator();
        let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let r = v.validate_roundtrip(&val, &val);
        assert!(r.is_lossless());
        assert!(r.lost_fields.is_empty());
        assert!(r.added_fields.is_empty());
        assert!(r.changed_fields.is_empty());
    }

    #[test]
    fn roundtrip_lost_field() {
        let v = validator();
        let orig = json!({"model": "gpt-4", "temperature": 0.7});
        let roundtripped = json!({"model": "gpt-4"});
        let r = v.validate_roundtrip(&orig, &roundtripped);
        assert!(!r.is_lossless());
        assert!(r.lost_fields.contains(&"temperature".to_string()));
    }

    #[test]
    fn roundtrip_added_field() {
        let v = validator();
        let orig = json!({"model": "gpt-4"});
        let roundtripped = json!({"model": "gpt-4", "extra": true});
        let r = v.validate_roundtrip(&orig, &roundtripped);
        assert!(!r.is_lossless());
        assert!(r.added_fields.contains(&"extra".to_string()));
    }

    #[test]
    fn roundtrip_changed_field() {
        let v = validator();
        let orig = json!({"model": "gpt-4", "temperature": 0.7});
        let roundtripped = json!({"model": "gpt-4", "temperature": 0.5});
        let r = v.validate_roundtrip(&orig, &roundtripped);
        assert!(!r.is_lossless());
        assert!(r.changed_fields.contains(&"temperature".to_string()));
    }

    // -- ValidationPipeline --

    #[test]
    fn pipeline_full_pass() {
        let pipe = ValidationPipeline::new(
            DefaultMappingValidator::new(),
            Dialect::OpenAi,
            Dialect::OpenAi,
        );
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let result = pipe.run(&req, |v| Ok(v.clone()));
        assert!(result.pre.is_valid());
        assert!(result.mapped.is_some());
        assert!(result.post.as_ref().unwrap().is_valid());
    }

    #[test]
    fn pipeline_pre_fail_skips_map() {
        let pipe = ValidationPipeline::new(
            DefaultMappingValidator::new(),
            Dialect::OpenAi,
            Dialect::OpenAi,
        );
        let req = json!({"not_model": true});
        let result = pipe.run(&req, |_| panic!("should not be called"));
        assert!(!result.pre.is_valid());
        assert!(result.mapped.is_none());
        assert!(result.post.is_none());
    }

    #[test]
    fn pipeline_map_error() {
        let pipe = ValidationPipeline::new(
            DefaultMappingValidator::new(),
            Dialect::OpenAi,
            Dialect::Claude,
        );
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let result = pipe.run(&req, |_| Err("boom".into()));
        assert!(result.pre.is_valid());
        assert!(result.mapped.is_none());
        let post = result.post.unwrap();
        assert!(!post.is_valid());
        assert_eq!(post.issues[0].code, "mapping_failed");
    }

    #[test]
    fn pipeline_accessor() {
        let pipe = ValidationPipeline::new(
            DefaultMappingValidator::new(),
            Dialect::OpenAi,
            Dialect::Claude,
        );
        let _v = pipe.validator();
        assert_eq!(pipe.source, Dialect::OpenAi);
        assert_eq!(pipe.target, Dialect::Claude);
    }

    // -- validate_mapping (IR-level) --

    #[test]
    fn validate_mapping_same_dialect_succeeds() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        assert!(validate_mapping(Dialect::OpenAi, Dialect::OpenAi, &conv).is_ok());
    }

    #[test]
    fn validate_mapping_openai_to_claude_simple() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        assert!(validate_mapping(Dialect::OpenAi, Dialect::Claude, &conv).is_ok());
    }

    #[test]
    fn validate_mapping_thinking_to_openai() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "hmm".into(),
            }],
        )]);
        let err = validate_mapping(Dialect::Claude, Dialect::OpenAi, &conv).unwrap_err();
        assert!(err.has_code("unsupported_thinking"));
    }

    #[test]
    fn validate_mapping_collects_all_errors() {
        use abp_core::ir::IrMessage;
        // Codex: no system, no tools, no images, no thinking
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "data".into(),
                }],
            ),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "test".into(),
                    input: json!({}),
                }],
            ),
        ]);
        let err = validate_mapping(Dialect::Claude, Dialect::Codex, &conv).unwrap_err();
        // Should have multiple issues, not just the first
        assert!(err.issue_count() >= 3);
        assert!(err.has_code("unsupported_system_prompt"));
        assert!(err.has_code("unsupported_image"));
        assert!(err.has_code("unsupported_tool_use"));
    }

    #[test]
    fn validate_mapping_empty_request_succeeds() {
        let conv = IrConversation::new();
        assert!(validate_mapping(Dialect::OpenAi, Dialect::Claude, &conv).is_ok());
    }

    #[test]
    fn validate_mapping_empty_message_detected() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);
        let err = validate_mapping(Dialect::OpenAi, Dialect::Claude, &conv).unwrap_err();
        assert!(err.has_code("empty_message"));
    }

    #[test]
    fn validate_mapping_images_to_codex() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data".into(),
            }],
        )]);
        let err = validate_mapping(Dialect::OpenAi, Dialect::Codex, &conv).unwrap_err();
        assert!(err.has_code("unsupported_image"));
    }

    #[test]
    fn validate_mapping_tools_to_codex() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "test".into(),
                input: json!({}),
            }],
        )]);
        let err = validate_mapping(Dialect::OpenAi, Dialect::Codex, &conv).unwrap_err();
        assert!(err.has_code("unsupported_tool_use"));
    }

    #[test]
    fn validate_mapping_unsupported_pair() {
        use abp_core::ir::IrMessage;
        // Codex ↔ Copilot has no mapper
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let err = validate_mapping(Dialect::Codex, Dialect::Copilot, &conv).unwrap_err();
        assert!(err.has_code("unsupported_dialect_pair"));
    }

    #[test]
    fn validate_mapping_all_supported_pairs_text_only() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        for (from, to) in crate::supported_ir_pairs() {
            let result = validate_mapping(from, to, &conv);
            assert!(
                result.is_ok(),
                "text-only should pass for supported pair {} -> {}",
                from.label(),
                to.label()
            );
        }
    }

    #[test]
    fn validate_mapping_error_display() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "hmm".into(),
            }],
        )]);
        let err = validate_mapping(Dialect::Claude, Dialect::OpenAi, &conv).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("validation failed"));
        assert!(msg.contains("issue(s)"));
    }

    #[test]
    fn validate_mapping_tool_result_to_codex() {
        use abp_core::ir::IrMessage;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let err = validate_mapping(Dialect::OpenAi, Dialect::Codex, &conv).unwrap_err();
        assert!(err.has_code("unsupported_tool_result"));
    }

    #[test]
    fn mapping_validation_error_is_debug_clone() {
        let err = MappingValidationError {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            issues: vec![MappingValidationIssue {
                code: "test".into(),
                feature: "test".into(),
                message: "test".into(),
            }],
        };
        let _ = format!("{:?}", err);
        let _ = err.clone();
    }

    #[test]
    fn mapping_validation_issue_serde_roundtrip() {
        let issue = MappingValidationIssue {
            code: "unsupported_thinking".into(),
            feature: "thinking".into(),
            message: "not supported".into(),
        };
        let json_str = serde_json::to_string(&issue).unwrap();
        let back: MappingValidationIssue = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, issue);
    }

    #[test]
    fn mapping_validation_error_serde_roundtrip() {
        let err = MappingValidationError {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Codex,
            issues: vec![MappingValidationIssue {
                code: "test".into(),
                feature: "feat".into(),
                message: "msg".into(),
            }],
        };
        let json_str = serde_json::to_string(&err).unwrap();
        let back: MappingValidationError = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.source_dialect, Dialect::Claude);
        assert_eq!(back.target_dialect, Dialect::Codex);
        assert_eq!(back.issue_count(), 1);
    }
}
