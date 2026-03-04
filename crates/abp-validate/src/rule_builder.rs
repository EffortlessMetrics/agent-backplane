// SPDX-License-Identifier: MIT OR Apache-2.0
//! Custom validation rule builder for composing ad-hoc validation logic.
//!
//! The [`RuleBuilder`] lets callers declaratively compose validation rules
//! from predicates, field-presence checks, and arbitrary closures, then
//! run them as a single validator.

use serde_json::Value;

use crate::{ValidationErrorKind, ValidationErrors};

/// Type alias for rule check closures.
type RuleCheckFn = Box<dyn Fn(&Value, &mut ValidationErrors) + Send + Sync>;

/// A single named validation rule backed by a closure.
struct Rule {
    name: String,
    check: RuleCheckFn,
}

impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rule").field("name", &self.name).finish()
    }
}

/// Builder for composing custom validation rules over [`serde_json::Value`].
///
/// # Example
///
/// ```
/// use abp_validate::RuleBuilder;
///
/// let validator = RuleBuilder::new()
///     .require_field("name")
///     .require_string("version")
///     .build();
///
/// let ok = serde_json::json!({"name": "x", "version": "1.0"});
/// assert!(validator.validate(&ok).is_ok());
/// ```
#[derive(Debug, Default)]
pub struct RuleBuilder {
    rules: Vec<Rule>,
}

impl RuleBuilder {
    /// Create an empty rule builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Require that `field` exists and is not `null`.
    #[must_use]
    pub fn require_field(mut self, field: &str) -> Self {
        let field = field.to_string();
        self.rules.push(Rule {
            name: format!("require:{field}"),
            check: Box::new(move |val, errs| {
                if let Some(obj) = val.as_object() {
                    match obj.get(&field) {
                        None | Some(Value::Null) => {
                            errs.add(
                                &field,
                                ValidationErrorKind::Required,
                                format!("missing required field \"{field}\""),
                            );
                        }
                        _ => {}
                    }
                }
            }),
        });
        self
    }

    /// Require that `field` exists and is a JSON string.
    #[must_use]
    pub fn require_string(mut self, field: &str) -> Self {
        let field = field.to_string();
        self.rules.push(Rule {
            name: format!("require_string:{field}"),
            check: Box::new(move |val, errs| {
                if let Some(obj) = val.as_object() {
                    match obj.get(&field) {
                        None | Some(Value::Null) => {
                            errs.add(
                                &field,
                                ValidationErrorKind::Required,
                                format!("missing required string field \"{field}\""),
                            );
                        }
                        Some(v) if !v.is_string() => {
                            errs.add(
                                &field,
                                ValidationErrorKind::InvalidFormat,
                                format!("\"{field}\" must be a string"),
                            );
                        }
                        _ => {}
                    }
                }
            }),
        });
        self
    }

    /// Require that `field` exists and is a JSON array.
    #[must_use]
    pub fn require_array(mut self, field: &str) -> Self {
        let field = field.to_string();
        self.rules.push(Rule {
            name: format!("require_array:{field}"),
            check: Box::new(move |val, errs| {
                if let Some(obj) = val.as_object() {
                    match obj.get(&field) {
                        None | Some(Value::Null) => {
                            errs.add(
                                &field,
                                ValidationErrorKind::Required,
                                format!("missing required array field \"{field}\""),
                            );
                        }
                        Some(v) if !v.is_array() => {
                            errs.add(
                                &field,
                                ValidationErrorKind::InvalidFormat,
                                format!("\"{field}\" must be an array"),
                            );
                        }
                        _ => {}
                    }
                }
            }),
        });
        self
    }

    /// Require that `field` is a number in the range `[min, max]` (inclusive).
    #[must_use]
    pub fn require_number_range(mut self, field: &str, min: f64, max: f64) -> Self {
        let field = field.to_string();
        self.rules.push(Rule {
            name: format!("range:{field}"),
            check: Box::new(move |val, errs| {
                if let Some(obj) = val.as_object() {
                    if let Some(v) = obj.get(&field) {
                        if let Some(n) = v.as_f64() {
                            if n < min || n > max {
                                errs.add(
                                    &field,
                                    ValidationErrorKind::OutOfRange,
                                    format!(
                                        "\"{field}\" value {n} is outside range [{min}, {max}]"
                                    ),
                                );
                            }
                        } else if !v.is_null() {
                            errs.add(
                                &field,
                                ValidationErrorKind::InvalidFormat,
                                format!("\"{field}\" must be a number"),
                            );
                        }
                    }
                }
            }),
        });
        self
    }

    /// Require that `field` is one of the given allowed string values.
    #[must_use]
    pub fn require_one_of(mut self, field: &str, allowed: &[&str]) -> Self {
        let field = field.to_string();
        let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_string()).collect();
        self.rules.push(Rule {
            name: format!("one_of:{field}"),
            check: Box::new(move |val, errs| {
                if let Some(obj) = val.as_object() {
                    if let Some(v) = obj.get(&field) {
                        if let Some(s) = v.as_str() {
                            if !allowed.iter().any(|a| a == s) {
                                errs.add(
                                    &field,
                                    ValidationErrorKind::InvalidFormat,
                                    format!(
                                        "\"{field}\" must be one of [{}], got \"{s}\"",
                                        allowed.join(", "),
                                    ),
                                );
                            }
                        }
                    }
                }
            }),
        });
        self
    }

    /// Add an arbitrary validation rule with a custom closure.
    ///
    /// The closure receives the JSON value and a mutable [`ValidationErrors`]
    /// to which it should append any problems found.
    #[must_use]
    pub fn custom<F>(mut self, name: &str, check: F) -> Self
    where
        F: Fn(&Value, &mut ValidationErrors) + Send + Sync + 'static,
    {
        self.rules.push(Rule {
            name: name.to_string(),
            check: Box::new(check),
        });
        self
    }

    /// Build a [`CustomValidator`] from the accumulated rules.
    #[must_use]
    pub fn build(self) -> CustomValidator {
        CustomValidator { rules: self.rules }
    }
}

/// A validator assembled from [`RuleBuilder`] rules.
#[derive(Debug)]
pub struct CustomValidator {
    rules: Vec<Rule>,
}

impl CustomValidator {
    /// Run all rules against `value`.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationErrors`] containing all rule violations.
    pub fn validate(&self, value: &Value) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        // Ensure top-level is an object for field-based rules
        if !value.is_object() && !self.rules.is_empty() {
            errs.add(
                "",
                ValidationErrorKind::InvalidFormat,
                "value must be a JSON object",
            );
            return errs.into_result();
        }

        for rule in &self.rules {
            (rule.check)(value, &mut errs);
        }

        errs.into_result()
    }

    /// Number of rules in this validator.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}
