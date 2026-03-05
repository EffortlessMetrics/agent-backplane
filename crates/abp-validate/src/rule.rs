// SPDX-License-Identifier: MIT OR Apache-2.0
//! `ValidationRule` trait and built-in rule implementations.

use serde_json::Value;

use crate::report::{Severity, ValidationReport};

/// A single, reusable validation rule that inspects a JSON value
/// and appends findings to a [`ValidationReport`].
pub trait ValidationRule: Send + Sync {
    /// Human-readable name of this rule.
    fn name(&self) -> &str;

    /// Check `value` and push any issues into `report`.
    fn check(&self, value: &Value, report: &mut ValidationReport);
}

// ── Built-in rules ─────────────────────────────────────────────────────

/// Requires that a top-level field exists and is not `null`.
#[derive(Debug, Clone)]
pub struct RequiredFieldRule {
    field: String,
}

impl RequiredFieldRule {
    /// Create a rule requiring `field`.
    #[must_use]
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl ValidationRule for RequiredFieldRule {
    fn name(&self) -> &str {
        "required_field"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            match obj.get(&self.field) {
                None | Some(Value::Null) => {
                    report.add_with_rule(
                        &self.field,
                        Severity::Error,
                        format!("missing required field '{}'", self.field),
                        self.name(),
                    );
                }
                _ => {}
            }
        }
    }
}

/// Requires that a field, if present, is a JSON string.
#[derive(Debug, Clone)]
pub struct TypeCheckRule {
    field: String,
    expected: ExpectedType,
}

/// Expected JSON type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedType {
    /// JSON string.
    String,
    /// JSON number.
    Number,
    /// JSON boolean.
    Bool,
    /// JSON object.
    Object,
    /// JSON array.
    Array,
}

impl std::fmt::Display for ExpectedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => f.write_str("string"),
            Self::Number => f.write_str("number"),
            Self::Bool => f.write_str("boolean"),
            Self::Object => f.write_str("object"),
            Self::Array => f.write_str("array"),
        }
    }
}

impl TypeCheckRule {
    /// Create a rule checking that `field` has `expected` type.
    #[must_use]
    pub fn new(field: impl Into<String>, expected: ExpectedType) -> Self {
        Self {
            field: field.into(),
            expected,
        }
    }
}

impl ValidationRule for TypeCheckRule {
    fn name(&self) -> &str {
        "type_check"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            if let Some(val) = obj.get(&self.field) {
                if val.is_null() {
                    return; // null is handled by RequiredFieldRule
                }
                let ok = match self.expected {
                    ExpectedType::String => val.is_string(),
                    ExpectedType::Number => val.is_number(),
                    ExpectedType::Bool => val.is_boolean(),
                    ExpectedType::Object => val.is_object(),
                    ExpectedType::Array => val.is_array(),
                };
                if !ok {
                    report.add_with_rule(
                        &self.field,
                        Severity::Error,
                        format!(
                            "field '{}' expected {}, got {}",
                            self.field,
                            self.expected,
                            json_type_label(val),
                        ),
                        self.name(),
                    );
                }
            }
        }
    }
}

/// Requires that a string field has length within `[min, max]`.
#[derive(Debug, Clone)]
pub struct StringLengthRule {
    field: String,
    min: Option<usize>,
    max: Option<usize>,
}

impl StringLengthRule {
    /// Create a rule bounding the string length of `field`.
    #[must_use]
    pub fn new(field: impl Into<String>, min: Option<usize>, max: Option<usize>) -> Self {
        Self {
            field: field.into(),
            min,
            max,
        }
    }
}

impl ValidationRule for StringLengthRule {
    fn name(&self) -> &str {
        "string_length"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            if let Some(Value::String(s)) = obj.get(&self.field) {
                if let Some(min) = self.min {
                    if s.len() < min {
                        report.add_with_rule(
                            &self.field,
                            Severity::Error,
                            format!(
                                "field '{}' length {} is below minimum {}",
                                self.field,
                                s.len(),
                                min,
                            ),
                            self.name(),
                        );
                    }
                }
                if let Some(max) = self.max {
                    if s.len() > max {
                        report.add_with_rule(
                            &self.field,
                            Severity::Error,
                            format!(
                                "field '{}' length {} exceeds maximum {}",
                                self.field,
                                s.len(),
                                max,
                            ),
                            self.name(),
                        );
                    }
                }
            }
        }
    }
}

/// Requires that a numeric field falls within `[min, max]`.
#[derive(Debug, Clone)]
pub struct NumberRangeRule {
    field: String,
    min: Option<f64>,
    max: Option<f64>,
}

impl NumberRangeRule {
    /// Create a rule bounding the numeric value of `field`.
    #[must_use]
    pub fn new(field: impl Into<String>, min: Option<f64>, max: Option<f64>) -> Self {
        Self {
            field: field.into(),
            min,
            max,
        }
    }
}

impl ValidationRule for NumberRangeRule {
    fn name(&self) -> &str {
        "number_range"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            if let Some(val) = obj.get(&self.field) {
                if let Some(n) = val.as_f64() {
                    if let Some(min) = self.min {
                        if n < min {
                            report.add_with_rule(
                                &self.field,
                                Severity::Error,
                                format!(
                                    "field '{}' value {} is below minimum {}",
                                    self.field, n, min,
                                ),
                                self.name(),
                            );
                        }
                    }
                    if let Some(max) = self.max {
                        if n > max {
                            report.add_with_rule(
                                &self.field,
                                Severity::Error,
                                format!(
                                    "field '{}' value {} exceeds maximum {}",
                                    self.field, n, max,
                                ),
                                self.name(),
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Requires that a string field is non-empty (after trimming).
#[derive(Debug, Clone)]
pub struct NonEmptyStringRule {
    field: String,
}

impl NonEmptyStringRule {
    /// Create a rule requiring a non-empty string in `field`.
    #[must_use]
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl ValidationRule for NonEmptyStringRule {
    fn name(&self) -> &str {
        "non_empty_string"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            if let Some(Value::String(s)) = obj.get(&self.field) {
                if s.trim().is_empty() {
                    report.add_with_rule(
                        &self.field,
                        Severity::Error,
                        format!("field '{}' must not be empty", self.field),
                        self.name(),
                    );
                }
            }
        }
    }
}

/// Requires that a string field is one of a set of allowed values.
#[derive(Debug, Clone)]
pub struct OneOfRule {
    field: String,
    allowed: Vec<String>,
}

impl OneOfRule {
    /// Create a rule constraining `field` to one of `allowed` values.
    #[must_use]
    pub fn new(field: impl Into<String>, allowed: Vec<String>) -> Self {
        Self {
            field: field.into(),
            allowed,
        }
    }
}

impl ValidationRule for OneOfRule {
    fn name(&self) -> &str {
        "one_of"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            if let Some(Value::String(s)) = obj.get(&self.field) {
                if !self.allowed.iter().any(|a| a == s) {
                    report.add_with_rule(
                        &self.field,
                        Severity::Error,
                        format!(
                            "field '{}' value '{}' is not one of [{}]",
                            self.field,
                            s,
                            self.allowed.join(", "),
                        ),
                        self.name(),
                    );
                }
            }
        }
    }
}

/// Warns when an object field is empty (zero keys or zero elements).
#[derive(Debug, Clone)]
pub struct NonEmptyCollectionRule {
    field: String,
}

impl NonEmptyCollectionRule {
    /// Create a rule warning when `field` is an empty object or array.
    #[must_use]
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl ValidationRule for NonEmptyCollectionRule {
    fn name(&self) -> &str {
        "non_empty_collection"
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        if let Some(obj) = value.as_object() {
            match obj.get(&self.field) {
                Some(Value::Object(m)) if m.is_empty() => {
                    report.add_with_rule(
                        &self.field,
                        Severity::Warning,
                        format!("field '{}' is an empty object", self.field),
                        self.name(),
                    );
                }
                Some(Value::Array(a)) if a.is_empty() => {
                    report.add_with_rule(
                        &self.field,
                        Severity::Warning,
                        format!("field '{}' is an empty array", self.field),
                        self.name(),
                    );
                }
                _ => {}
            }
        }
    }
}

/// A rule backed by a closure for ad-hoc checks.
pub struct ClosureRule<F> {
    name: String,
    check_fn: F,
}

impl<F> ClosureRule<F>
where
    F: Fn(&Value, &mut ValidationReport) + Send + Sync,
{
    /// Create a closure-based rule.
    pub fn new(name: impl Into<String>, check_fn: F) -> Self {
        Self {
            name: name.into(),
            check_fn,
        }
    }
}

impl<F> std::fmt::Debug for ClosureRule<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosureRule")
            .field("name", &self.name)
            .finish()
    }
}

impl<F> ValidationRule for ClosureRule<F>
where
    F: Fn(&Value, &mut ValidationReport) + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn check(&self, value: &Value, report: &mut ValidationReport) {
        (self.check_fn)(value, report);
    }
}

fn json_type_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
