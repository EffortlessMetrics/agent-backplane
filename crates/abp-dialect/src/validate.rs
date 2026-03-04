// SPDX-License-Identifier: MIT OR Apache-2.0
//! Request/response validation and normalization for each dialect.
//!
//! Provides [`RequestValidator`](crate::validate::RequestValidator) which checks that a raw JSON value
//! conforms to a specific [`Dialect`](crate::Dialect)'s expected schema, returning
//! structured [`ValidationIssue`](crate::validate::ValidationIssue)s with machine-readable codes and
//! severity levels.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Dialect;

// ── Severity ────────────────────────────────────────────────────────────

/// Severity level for a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational note — the request is valid but could be improved.
    Info,
    /// Warning — the request is technically valid but likely unintended.
    Warning,
    /// Error — the request violates a required constraint.
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => f.write_str("info"),
            Self::Warning => f.write_str("warning"),
            Self::Error => f.write_str("error"),
        }
    }
}

// ── ValidationIssue ─────────────────────────────────────────────────────

/// A single issue found during validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Severity of the issue.
    pub severity: Severity,
    /// JSON-pointer-style path to the problematic field (e.g. `"model"`,
    /// `"messages[0].role"`).
    pub field: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// Machine-readable issue code (e.g. `"missing_required_field"`).
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

/// Aggregated result of validating a request or response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// All issues discovered during validation.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Returns `true` when no error-level issues were found.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    /// Returns `true` when at least one warning-level issue exists.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Warning)
    }

    /// Number of error-level issues.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    /// Number of warning-level issues.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }

    /// Number of info-level issues.
    #[must_use]
    pub fn info_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Info)
            .count()
    }
}

// ── RequestValidator ────────────────────────────────────────────────────

/// Validates a raw JSON request against a specific [`Dialect`]'s expected
/// schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestValidator {
    _priv: (),
}

impl RequestValidator {
    /// Create a new validator.
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }

    /// Validate `request` as a message in the given `dialect`.
    #[must_use]
    pub fn validate(&self, dialect: Dialect, request: &Value) -> ValidationResult {
        let mut issues = Vec::new();

        let Some(obj) = request.as_object() else {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                field: String::new(),
                message: "expected a JSON object".into(),
                code: "invalid_type".into(),
            });
            return ValidationResult { issues };
        };

        match dialect {
            Dialect::OpenAi => validate_openai(obj, &mut issues),
            Dialect::Claude => validate_claude(obj, &mut issues),
            Dialect::Gemini => validate_gemini(obj, &mut issues),
            Dialect::Codex => validate_codex(obj, &mut issues),
            Dialect::Kimi => validate_kimi(obj, &mut issues),
            Dialect::Copilot => validate_copilot(obj, &mut issues),
        }

        // Common: model name format check (applies when model is present).
        if let Some(model) = obj.get("model") {
            check_model_field(model, &mut issues);
        }

        ValidationResult { issues }
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────

/// Push a missing-required-field error.
fn require_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if !obj.contains_key(field) {
        issues.push(ValidationIssue {
            severity: Severity::Error,
            field: field.into(),
            message: format!("missing required \"{field}\" field"),
            code: "missing_required_field".into(),
        });
    }
}

/// Validate that a field, if present, is an array. Returns `true` when it
/// is an array (or absent).
fn require_array(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<ValidationIssue>,
) -> bool {
    match obj.get(field) {
        Some(Value::Array(_)) => true,
        Some(_) => {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                field: field.into(),
                message: format!("\"{field}\" must be an array"),
                code: "invalid_field_type".into(),
            });
            false
        }
        None => true,
    }
}

/// Validate model field value is a non-empty string with reasonable format.
fn check_model_field(value: &Value, issues: &mut Vec<ValidationIssue>) {
    match value {
        Value::String(s) => {
            if s.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    field: "model".into(),
                    message: "model name must not be empty".into(),
                    code: "empty_model_name".into(),
                });
            } else if s.len() > 256 {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "model".into(),
                    message: "model name is unusually long".into(),
                    code: "long_model_name".into(),
                });
            } else if s.contains(' ') {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "model".into(),
                    message: "model name contains spaces".into(),
                    code: "model_name_has_spaces".into(),
                });
            }
        }
        _ => {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                field: "model".into(),
                message: "\"model\" must be a string".into(),
                code: "invalid_field_type".into(),
            });
        }
    }
}

/// Validate messages array entries have a `role` field.
fn check_messages_roles(msgs: &[Value], field_prefix: &str, issues: &mut Vec<ValidationIssue>) {
    for (i, msg) in msgs.iter().enumerate() {
        if msg.get("role").is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                field: format!("{field_prefix}[{i}].role"),
                message: "each message must have a \"role\" field".into(),
                code: "missing_required_field".into(),
            });
        } else if let Some(role) = msg.get("role") {
            if !role.is_string() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    field: format!("{field_prefix}[{i}].role"),
                    message: "\"role\" must be a string".into(),
                    code: "invalid_field_type".into(),
                });
            }
        }
    }
}

// ── Per-dialect validators ──────────────────────────────────────────────

fn validate_openai(obj: &serde_json::Map<String, Value>, issues: &mut Vec<ValidationIssue>) {
    require_field(obj, "model", issues);
    require_field(obj, "messages", issues);

    if require_array(obj, "messages", issues) {
        if let Some(Value::Array(msgs)) = obj.get("messages") {
            check_messages_roles(msgs, "messages", issues);

            if msgs.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "messages".into(),
                    message: "messages array is empty".into(),
                    code: "empty_messages".into(),
                });
            }
        }
    }
}

fn validate_claude(obj: &serde_json::Map<String, Value>, issues: &mut Vec<ValidationIssue>) {
    require_field(obj, "model", issues);
    require_field(obj, "messages", issues);
    require_field(obj, "max_tokens", issues);

    if require_array(obj, "messages", issues) {
        if let Some(Value::Array(msgs)) = obj.get("messages") {
            check_messages_roles(msgs, "messages", issues);

            for (i, msg) in msgs.iter().enumerate() {
                if let Some(content) = msg.get("content") {
                    if !content.is_string() && !content.is_array() {
                        issues.push(ValidationIssue {
                            severity: Severity::Error,
                            field: format!("messages[{i}].content"),
                            message: "\"content\" must be a string or array of blocks".into(),
                            code: "invalid_field_type".into(),
                        });
                    }
                }
            }

            if msgs.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "messages".into(),
                    message: "messages array is empty".into(),
                    code: "empty_messages".into(),
                });
            }
        }
    }

    // max_tokens type check
    if let Some(mt) = obj.get("max_tokens") {
        if !mt.is_number() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                field: "max_tokens".into(),
                message: "\"max_tokens\" must be a number".into(),
                code: "invalid_field_type".into(),
            });
        }
    }
}

fn validate_gemini(obj: &serde_json::Map<String, Value>, issues: &mut Vec<ValidationIssue>) {
    require_field(obj, "model", issues);
    require_field(obj, "contents", issues);

    if require_array(obj, "contents", issues) {
        if let Some(Value::Array(contents)) = obj.get("contents") {
            for (i, c) in contents.iter().enumerate() {
                if c.get("parts").is_none() {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        field: format!("contents[{i}].parts"),
                        message: "each content entry must have \"parts\"".into(),
                        code: "missing_required_field".into(),
                    });
                } else if let Some(parts) = c.get("parts") {
                    if !parts.is_array() {
                        issues.push(ValidationIssue {
                            severity: Severity::Error,
                            field: format!("contents[{i}].parts"),
                            message: "\"parts\" must be an array".into(),
                            code: "invalid_field_type".into(),
                        });
                    }
                }
            }

            if contents.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "contents".into(),
                    message: "contents array is empty".into(),
                    code: "empty_contents".into(),
                });
            }
        }
    }
}

fn validate_codex(obj: &serde_json::Map<String, Value>, issues: &mut Vec<ValidationIssue>) {
    require_field(obj, "model", issues);
    require_field(obj, "messages", issues);

    if require_array(obj, "messages", issues) {
        if let Some(Value::Array(msgs)) = obj.get("messages") {
            check_messages_roles(msgs, "messages", issues);

            if msgs.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "messages".into(),
                    message: "messages array is empty".into(),
                    code: "empty_messages".into(),
                });
            }
        }
    }
}

fn validate_kimi(obj: &serde_json::Map<String, Value>, issues: &mut Vec<ValidationIssue>) {
    require_field(obj, "model", issues);
    require_field(obj, "messages", issues);

    if require_array(obj, "messages", issues) {
        if let Some(Value::Array(msgs)) = obj.get("messages") {
            check_messages_roles(msgs, "messages", issues);

            if msgs.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "messages".into(),
                    message: "messages array is empty".into(),
                    code: "empty_messages".into(),
                });
            }
        }
    }
}

fn validate_copilot(obj: &serde_json::Map<String, Value>, issues: &mut Vec<ValidationIssue>) {
    require_field(obj, "model", issues);
    require_field(obj, "messages", issues);

    if require_array(obj, "messages", issues) {
        if let Some(Value::Array(msgs)) = obj.get("messages") {
            check_messages_roles(msgs, "messages", issues);

            if msgs.is_empty() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    field: "messages".into(),
                    message: "messages array is empty".into(),
                    code: "empty_messages".into(),
                });
            }
        }
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v() -> RequestValidator {
        RequestValidator::new()
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Info.to_string(), "info");
    }

    #[test]
    fn severity_serde_roundtrip() {
        let s = Severity::Warning;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"warning\"");
        let back: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Severity::Warning);
    }

    #[test]
    fn issue_display() {
        let issue = ValidationIssue {
            severity: Severity::Error,
            field: "model".into(),
            message: "missing".into(),
            code: "missing_required_field".into(),
        };
        let s = format!("{issue}");
        assert!(s.contains("error"));
        assert!(s.contains("model"));
        assert!(s.contains("missing"));
    }

    #[test]
    fn result_is_valid_with_no_issues() {
        let r = ValidationResult { issues: vec![] };
        assert!(r.is_valid());
        assert!(!r.has_warnings());
        assert_eq!(r.error_count(), 0);
    }

    #[test]
    fn result_is_valid_with_only_warnings() {
        let r = ValidationResult {
            issues: vec![ValidationIssue {
                severity: Severity::Warning,
                field: "x".into(),
                message: "w".into(),
                code: "w".into(),
            }],
        };
        assert!(r.is_valid());
        assert!(r.has_warnings());
    }

    #[test]
    fn non_object_is_invalid() {
        let r = v().validate(Dialect::OpenAi, &json!(42));
        assert!(!r.is_valid());
        assert_eq!(r.error_count(), 1);
        assert_eq!(r.issues[0].code, "invalid_type");
    }
}
