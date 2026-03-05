// SPDX-License-Identifier: MIT OR Apache-2.0
//! Config validation against schema rules (field types, ranges, relationships).

use serde_json::Value;

use crate::report::{Severity, ValidationReport};
use crate::rule::ValidationRule;

/// Validates a `BackplaneConfig`-shaped JSON value against schema rules.
///
/// This validator operates on `serde_json::Value` so it can be used
/// independently of the `abp-config` crate. It checks field types,
/// numeric ranges, string constraints, and cross-field relationships.
///
/// # Examples
///
/// ```
/// use abp_validate::config::ConfigValidator;
///
/// let cfg = serde_json::json!({
///     "log_level": "info",
///     "backends": {}
/// });
/// let report = ConfigValidator::new().validate(&cfg);
/// assert!(report.is_valid());
/// ```
pub struct ConfigValidator {
    rules: Vec<Box<dyn ValidationRule>>,
}

impl std::fmt::Debug for ConfigValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigValidator")
            .field("rule_count", &self.rules.len())
            .finish()
    }
}

impl Default for ConfigValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigValidator {
    /// Create a validator pre-loaded with built-in config rules.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rules: default_config_rules(),
        }
    }

    /// Create a validator with no built-in rules.
    #[must_use]
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a custom rule.
    #[must_use]
    pub fn with_rule(mut self, rule: impl ValidationRule + 'static) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    /// Number of rules in this validator.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Validate a config JSON value and return a [`ValidationReport`].
    #[must_use]
    pub fn validate(&self, value: &Value) -> ValidationReport {
        let mut report = ValidationReport::new();

        if !value.is_object() {
            report.error("", "config must be a JSON object");
            return report;
        }

        for rule in &self.rules {
            rule.check(value, &mut report);
        }

        // Cross-field: default_backend must name an existing backend
        if let Some(obj) = value.as_object() {
            if let (Some(Value::String(default)), Some(Value::Object(backends))) =
                (obj.get("default_backend"), obj.get("backends"))
            {
                if !backends.is_empty() && !backends.contains_key(default) {
                    report.add_with_rule(
                        "default_backend",
                        Severity::Warning,
                        format!(
                            "default_backend '{}' does not match any configured backend",
                            default,
                        ),
                        "cross_field_ref",
                    );
                }
            }

            // Cross-field: port must be 1-65535
            if let Some(port_val) = obj.get("port") {
                if let Some(p) = port_val.as_u64() {
                    if p == 0 || p > 65535 {
                        report.add_with_rule(
                            "port",
                            Severity::Error,
                            format!("port {} is out of range (1-65535)", p),
                            "port_range",
                        );
                    }
                }
            }

            // Check each backend entry
            if let Some(Value::Object(backends)) = obj.get("backends") {
                for (name, entry) in backends {
                    validate_backend_entry(name, entry, &mut report);
                }
            }
        }

        report
    }
}

fn validate_backend_entry(name: &str, entry: &Value, report: &mut ValidationReport) {
    let Some(obj) = entry.as_object() else {
        report.error(
            format!("backends.{}", name),
            "backend entry must be an object",
        );
        return;
    };

    // Type tag is required
    match obj.get("type").and_then(Value::as_str) {
        Some("sidecar") => {
            // command is required and non-empty
            match obj.get("command").and_then(Value::as_str) {
                None => {
                    report.error(
                        format!("backends.{}.command", name),
                        "sidecar backend requires a 'command' field",
                    );
                }
                Some(cmd) if cmd.trim().is_empty() => {
                    report.error(
                        format!("backends.{}.command", name),
                        "sidecar command must not be empty",
                    );
                }
                _ => {}
            }

            // timeout_secs must be 1..86400
            if let Some(t) = obj.get("timeout_secs").and_then(Value::as_u64) {
                if t == 0 || t > 86_400 {
                    report.error(
                        format!("backends.{}.timeout_secs", name),
                        format!("timeout {}s out of range (1-86400)", t),
                    );
                } else if t > 3_600 {
                    report.warn(
                        format!("backends.{}.timeout_secs", name),
                        format!("large timeout ({}s)", t),
                    );
                }
            }
        }
        Some("mock") => { /* no additional checks */ }
        Some(other) => {
            report.error(
                format!("backends.{}.type", name),
                format!("unknown backend type '{}'", other),
            );
        }
        None => {
            report.error(
                format!("backends.{}.type", name),
                "backend entry requires a 'type' field",
            );
        }
    }
}

/// Default rules for BackplaneConfig validation.
fn default_config_rules() -> Vec<Box<dyn ValidationRule>> {
    use crate::rule::*;

    vec![
        Box::new(TypeCheckRule::new("log_level", ExpectedType::String)),
        Box::new(TypeCheckRule::new("backends", ExpectedType::Object)),
        Box::new(TypeCheckRule::new("default_backend", ExpectedType::String)),
        Box::new(TypeCheckRule::new("workspace_dir", ExpectedType::String)),
        Box::new(TypeCheckRule::new("receipts_dir", ExpectedType::String)),
        Box::new(TypeCheckRule::new("bind_address", ExpectedType::String)),
        Box::new(TypeCheckRule::new("port", ExpectedType::Number)),
        Box::new(TypeCheckRule::new("policy_profiles", ExpectedType::Array)),
        Box::new(OneOfRule::new(
            "log_level",
            vec![
                "trace".into(),
                "debug".into(),
                "info".into(),
                "warn".into(),
                "error".into(),
            ],
        )),
    ]
}
