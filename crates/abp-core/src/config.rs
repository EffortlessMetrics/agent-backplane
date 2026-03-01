// SPDX-License-Identifier: MIT OR Apache-2.0
//! Configuration validation and defaults for [`WorkOrder`]s.

use crate::WorkOrder;
use std::collections::HashSet;

/// Severity level for a configuration warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningSeverity {
    /// Informational note — not necessarily a problem.
    Info,
    /// Something likely unintended.
    Warning,
    /// Invalid configuration that will cause problems.
    Error,
}

/// A single configuration warning produced by [`ConfigValidator`].
#[derive(Debug, Clone)]
pub struct ConfigWarning {
    /// Dot-delimited path to the problematic field (e.g. `"config.max_turns"`).
    pub field: String,
    /// Human-readable description of the issue.
    pub message: String,
    /// How severe this issue is.
    pub severity: WarningSeverity,
}

/// Validates a [`WorkOrder`] and returns any configuration warnings.
#[derive(Debug, Default)]
pub struct ConfigValidator;

impl ConfigValidator {
    /// Create a new validator.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Validate a [`WorkOrder`] and return all detected warnings.
    #[must_use]
    pub fn validate_work_order(&self, wo: &WorkOrder) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        // Task must not be empty
        if wo.task.trim().is_empty() {
            warnings.push(ConfigWarning {
                field: "task".into(),
                message: "Task description must not be empty".into(),
                severity: WarningSeverity::Error,
            });
        }

        // max_turns > 0 if set
        if let Some(turns) = wo.config.max_turns
            && turns == 0
        {
            warnings.push(ConfigWarning {
                field: "config.max_turns".into(),
                message: "max_turns must be greater than 0".into(),
                severity: WarningSeverity::Error,
            });
        }

        // budget > 0 if set
        if let Some(budget) = wo.config.max_budget_usd
            && budget <= 0.0
        {
            warnings.push(ConfigWarning {
                field: "config.max_budget_usd".into(),
                message: "max_budget_usd must be greater than 0".into(),
                severity: WarningSeverity::Error,
            });
        }

        // No duplicate tool names in allowlist
        {
            let mut seen = HashSet::new();
            for tool in &wo.policy.allowed_tools {
                if !seen.insert(tool.as_str()) {
                    warnings.push(ConfigWarning {
                        field: "policy.allowed_tools".into(),
                        message: format!("Duplicate tool in allowlist: {tool}"),
                        severity: WarningSeverity::Warning,
                    });
                }
            }
        }

        // Model name not empty if set
        if let Some(ref model) = wo.config.model
            && model.trim().is_empty()
        {
            warnings.push(ConfigWarning {
                field: "config.model".into(),
                message: "Model name must not be empty".into(),
                severity: WarningSeverity::Error,
            });
        }

        // Policy profile: no empty globs
        self.check_no_empty_globs(&wo.policy.deny_read, "policy.deny_read", &mut warnings);
        self.check_no_empty_globs(&wo.policy.deny_write, "policy.deny_write", &mut warnings);
        self.check_no_empty_globs(
            &wo.policy.allow_network,
            "policy.allow_network",
            &mut warnings,
        );
        self.check_no_empty_globs(
            &wo.policy.deny_network,
            "policy.deny_network",
            &mut warnings,
        );
        self.check_no_empty_globs(
            &wo.policy.disallowed_tools,
            "policy.disallowed_tools",
            &mut warnings,
        );
        self.check_no_empty_globs(
            &wo.policy.require_approval_for,
            "policy.require_approval_for",
            &mut warnings,
        );

        // Vendor config keys: no empty keys
        for key in wo.config.vendor.keys() {
            if key.trim().is_empty() {
                warnings.push(ConfigWarning {
                    field: "config.vendor".into(),
                    message: "Vendor config contains an empty key".into(),
                    severity: WarningSeverity::Error,
                });
            }
        }

        warnings
    }

    fn check_no_empty_globs(
        &self,
        globs: &[String],
        field: &str,
        warnings: &mut Vec<ConfigWarning>,
    ) {
        for g in globs {
            if g.trim().is_empty() {
                warnings.push(ConfigWarning {
                    field: field.into(),
                    message: format!("Empty glob pattern in {field}"),
                    severity: WarningSeverity::Error,
                });
            }
        }
    }
}

/// Provides sensible default values for optional [`WorkOrder`] fields.
pub struct ConfigDefaults;

impl ConfigDefaults {
    /// Default maximum number of turns.
    #[must_use]
    pub fn default_max_turns() -> u32 {
        25
    }

    /// Default maximum budget in USD.
    #[must_use]
    pub fn default_max_budget() -> f64 {
        1.0
    }

    /// Default model identifier.
    #[must_use]
    pub fn default_model() -> &'static str {
        "gpt-4"
    }

    /// Fill in missing optional fields on a [`WorkOrder`] with defaults.
    pub fn apply_defaults(wo: &mut WorkOrder) {
        if wo.config.max_turns.is_none() {
            wo.config.max_turns = Some(Self::default_max_turns());
        }
        if wo.config.max_budget_usd.is_none() {
            wo.config.max_budget_usd = Some(Self::default_max_budget());
        }
        if wo.config.model.is_none() {
            wo.config.model = Some(Self::default_model().to_string());
        }
    }
}
