// SPDX-License-Identifier: MIT OR Apache-2.0
//! Structured validation, diffing, and environment-override helpers.
//!
//! This module adds a [`ConfigValidator`] (struct-based entry point),
//! [`diff_configs`] for debugging, and [`from_env_overrides`] as a
//! convenience re-export.

use crate::{is_valid_hostname, BackendEntry, BackplaneConfig, ConfigError};
use crate::{LARGE_TIMEOUT_THRESHOLD, MAX_TIMEOUT_SECS, VALID_LOG_LEVELS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

// ---------------------------------------------------------------------------
// Severity / ValidationIssue
// ---------------------------------------------------------------------------

/// Severity level for a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Purely informational — no action needed.
    Info,
    /// Something is sub-optimal but will not prevent operation.
    Warning,
    /// A hard error that must be fixed before the config can be used.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => f.write_str("info"),
            Severity::Warning => f.write_str("warning"),
            Severity::Error => f.write_str("error"),
        }
    }
}

/// A single validation finding with a [`Severity`] and a human-readable
/// message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// How serious the issue is.
    pub severity: Severity,
    /// Human-readable description.
    pub message: String,
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.severity, self.message)
    }
}

// ---------------------------------------------------------------------------
// ConfigValidator
// ---------------------------------------------------------------------------

/// Struct-based validator that produces [`ValidationIssue`]s with severity
/// levels.
///
/// This complements the free-function [`crate::validate_config`] by returning
/// richer issue objects instead of the opaque [`crate::ConfigWarning`] enum.
///
/// # Examples
///
/// ```
/// use abp_config::{BackplaneConfig, validate::ConfigValidator};
///
/// let cfg = BackplaneConfig::default();
/// let issues = ConfigValidator::validate(&cfg).unwrap();
/// for issue in &issues {
///     println!("{issue}");
/// }
/// ```
pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate `config` and return all findings.
    ///
    /// Hard errors (empty sidecar commands, out-of-range timeouts, invalid
    /// log levels) cause an `Err(ConfigError::ValidationError)`.
    /// Softer findings are returned in the `Ok` vec.
    pub fn validate(config: &BackplaneConfig) -> Result<Vec<ValidationIssue>, ConfigError> {
        let mut errors: Vec<String> = Vec::new();
        let mut issues: Vec<ValidationIssue> = Vec::new();

        // -- log_level -------------------------------------------------------
        if let Some(ref level) = config.log_level {
            if !VALID_LOG_LEVELS.contains(&level.as_str()) {
                errors.push(format!("invalid log_level '{level}'"));
            }
        }

        // -- backends --------------------------------------------------------
        if config.backends.is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Info,
                message: "no backends configured".into(),
            });
        }

        for (name, backend) in &config.backends {
            if name.is_empty() {
                errors.push("backend name must not be empty".into());
            }

            match backend {
                BackendEntry::Sidecar {
                    command,
                    timeout_secs,
                    ..
                } => {
                    if command.trim().is_empty() {
                        errors.push(format!(
                            "backend '{name}': sidecar command must not be empty"
                        ));
                    }
                    if let Some(t) = timeout_secs {
                        if *t == 0 || *t > MAX_TIMEOUT_SECS {
                            errors.push(format!(
                                "backend '{name}': timeout {t}s out of range (1..{MAX_TIMEOUT_SECS})"
                            ));
                        } else if *t > LARGE_TIMEOUT_THRESHOLD {
                            issues.push(ValidationIssue {
                                severity: Severity::Warning,
                                message: format!("backend '{name}' has a large timeout ({t}s)"),
                            });
                        }
                    }
                }
                BackendEntry::Mock {} => {}
            }
        }

        // -- missing optional fields -----------------------------------------
        if config.default_backend.is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: "missing optional field 'default_backend': callers must always specify --backend explicitly".into(),
            });
        }
        if config.receipts_dir.is_none() {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message:
                    "missing optional field 'receipts_dir': receipts will not be persisted to disk"
                        .into(),
            });
        }

        if errors.is_empty() {
            Ok(issues)
        } else {
            Err(ConfigError::ValidationError { reasons: errors })
        }
    }

    /// Return only issues at or above the given severity.
    pub fn validate_at(
        config: &BackplaneConfig,
        min_severity: Severity,
    ) -> Result<Vec<ValidationIssue>, ConfigError> {
        let issues = Self::validate(config)?;
        Ok(issues
            .into_iter()
            .filter(|i| i.severity >= min_severity)
            .collect())
    }
}

// ---------------------------------------------------------------------------
// ConfigDiff
// ---------------------------------------------------------------------------

/// A single difference between two [`BackplaneConfig`] values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDiff {
    /// Dot-separated field path (e.g. `"backends.mock"`).
    pub path: String,
    /// Old value rendered as a human-readable string.
    pub old_value: String,
    /// New value rendered as a human-readable string.
    pub new_value: String,
}

impl fmt::Display for ConfigDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} -> {}", self.path, self.old_value, self.new_value)
    }
}

/// Compare two configs and return a list of field-level differences.
///
/// Useful for debugging merge results or showing what an environment overlay
/// changed.
pub fn diff_configs(a: &BackplaneConfig, b: &BackplaneConfig) -> Vec<ConfigDiff> {
    let mut diffs = Vec::new();

    diff_option_field(
        &mut diffs,
        "default_backend",
        &a.default_backend,
        &b.default_backend,
    );
    diff_option_field(
        &mut diffs,
        "workspace_dir",
        &a.workspace_dir,
        &b.workspace_dir,
    );
    diff_option_field(&mut diffs, "log_level", &a.log_level, &b.log_level);
    diff_option_field(&mut diffs, "receipts_dir", &a.receipts_dir, &b.receipts_dir);
    diff_option_field(&mut diffs, "bind_address", &a.bind_address, &b.bind_address);

    // Port
    let port_a = a.port.map(|p| p.to_string());
    let port_b = b.port.map(|p| p.to_string());
    diff_option_field(&mut diffs, "port", &port_a, &port_b);

    // Policy profiles
    if a.policy_profiles != b.policy_profiles {
        diffs.push(ConfigDiff {
            path: "policy_profiles".into(),
            old_value: format!("{:?}", a.policy_profiles),
            new_value: format!("{:?}", b.policy_profiles),
        });
    }

    // Backends: compare keyed entries.
    let all_keys: BTreeSet<&String> = a.backends.keys().chain(b.backends.keys()).collect();
    for key in all_keys {
        let path = format!("backends.{key}");
        match (a.backends.get(key), b.backends.get(key)) {
            (None, Some(entry)) => {
                diffs.push(ConfigDiff {
                    path,
                    old_value: "<absent>".into(),
                    new_value: format_backend_entry(entry),
                });
            }
            (Some(entry), None) => {
                diffs.push(ConfigDiff {
                    path,
                    old_value: format_backend_entry(entry),
                    new_value: "<absent>".into(),
                });
            }
            (Some(ea), Some(eb)) if ea != eb => {
                diffs.push(ConfigDiff {
                    path,
                    old_value: format_backend_entry(ea),
                    new_value: format_backend_entry(eb),
                });
            }
            _ => {}
        }
    }

    diffs
}

// ---------------------------------------------------------------------------
// Env overrides (convenience re-export)
// ---------------------------------------------------------------------------

/// Apply `ABP_*` environment variable overrides to a mutable config.
///
/// This delegates to [`crate::apply_env_overrides`].
pub fn from_env_overrides(config: &mut BackplaneConfig) {
    crate::apply_env_overrides(config);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn diff_option_field(
    diffs: &mut Vec<ConfigDiff>,
    name: &str,
    a: &Option<String>,
    b: &Option<String>,
) {
    if a != b {
        diffs.push(ConfigDiff {
            path: name.into(),
            old_value: option_display(a),
            new_value: option_display(b),
        });
    }
}

fn option_display(opt: &Option<String>) -> String {
    match opt {
        Some(v) => format!("\"{v}\""),
        None => "<none>".into(),
    }
}

fn format_backend_entry(entry: &BackendEntry) -> String {
    match entry {
        BackendEntry::Mock {} => "mock".into(),
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            let mut s = format!("sidecar(command={command:?}");
            if !args.is_empty() {
                s.push_str(&format!(", args={args:?}"));
            }
            if let Some(t) = timeout_secs {
                s.push_str(&format!(", timeout={t}s"));
            }
            s.push(')');
            s
        }
    }
}

// ---------------------------------------------------------------------------
// IssueSeverity
// ---------------------------------------------------------------------------

/// Severity for a [`ConfigIssue`]: either a hard error or a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    /// A hard error that must be fixed.
    Error,
    /// A non-fatal issue that deserves attention.
    Warning,
}

impl fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IssueSeverity::Error => f.write_str("error"),
            IssueSeverity::Warning => f.write_str("warning"),
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigIssue
// ---------------------------------------------------------------------------

/// A single validation issue with a dotted field path and severity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigIssue {
    /// Dotted path to the field, e.g. `"backends.sc.command"`.
    pub field: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// How serious the issue is.
    pub severity: IssueSeverity,
}

impl fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.severity, self.field, self.message)
    }
}

// ---------------------------------------------------------------------------
// ConfigValidationResult
// ---------------------------------------------------------------------------

/// Structured result from [`ConfigValidator::check`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValidationResult {
    /// `true` when there are zero errors.
    pub valid: bool,
    /// Hard errors that must be fixed.
    pub errors: Vec<ConfigIssue>,
    /// Non-fatal warnings.
    pub warnings: Vec<ConfigIssue>,
    /// Actionable suggestions for improvement.
    pub suggestions: Vec<String>,
}

// ---------------------------------------------------------------------------
// ConfigValidator::check
// ---------------------------------------------------------------------------

impl ConfigValidator {
    /// Validate `config` and return a [`ConfigValidationResult`] with
    /// field-level issues, warnings, and suggestions.
    pub fn check(config: &BackplaneConfig) -> ConfigValidationResult {
        let mut errors: Vec<ConfigIssue> = Vec::new();
        let mut warnings: Vec<ConfigIssue> = Vec::new();
        let mut suggestions: Vec<String> = Vec::new();

        // -- log_level -------------------------------------------------------
        if let Some(ref level) = config.log_level {
            if !VALID_LOG_LEVELS.contains(&level.as_str()) {
                errors.push(ConfigIssue {
                    field: "log_level".into(),
                    message: format!(
                        "invalid log_level '{level}'; expected one of: {}",
                        VALID_LOG_LEVELS.join(", ")
                    ),
                    severity: IssueSeverity::Error,
                });
            }
        }

        // -- port ------------------------------------------------------------
        if let Some(p) = config.port {
            if p == 0 {
                errors.push(ConfigIssue {
                    field: "port".into(),
                    message: "port must be between 1 and 65535".into(),
                    severity: IssueSeverity::Error,
                });
            }
        }

        // -- bind_address ----------------------------------------------------
        if let Some(ref addr) = config.bind_address {
            if addr.trim().is_empty() {
                errors.push(ConfigIssue {
                    field: "bind_address".into(),
                    message: "bind_address must not be empty".into(),
                    severity: IssueSeverity::Error,
                });
            } else if addr.parse::<std::net::IpAddr>().is_err() && !is_valid_hostname(addr) {
                errors.push(ConfigIssue {
                    field: "bind_address".into(),
                    message: format!("bind_address '{addr}' is not a valid IP address or hostname"),
                    severity: IssueSeverity::Error,
                });
            }
        }

        // -- policy_profiles -------------------------------------------------
        for (i, path_str) in config.policy_profiles.iter().enumerate() {
            if path_str.trim().is_empty() {
                errors.push(ConfigIssue {
                    field: format!("policy_profiles[{i}]"),
                    message: "policy profile path must not be empty".into(),
                    severity: IssueSeverity::Error,
                });
            }
        }

        // -- backends --------------------------------------------------------
        for (name, backend) in &config.backends {
            if name.is_empty() {
                errors.push(ConfigIssue {
                    field: "backends".into(),
                    message: "backend name must not be empty".into(),
                    severity: IssueSeverity::Error,
                });
            }

            match backend {
                BackendEntry::Sidecar {
                    command,
                    timeout_secs,
                    ..
                } => {
                    if command.trim().is_empty() {
                        errors.push(ConfigIssue {
                            field: format!("backends.{name}.command"),
                            message: "sidecar command must not be empty".into(),
                            severity: IssueSeverity::Error,
                        });
                    }
                    if let Some(t) = timeout_secs {
                        if *t == 0 || *t > MAX_TIMEOUT_SECS {
                            errors.push(ConfigIssue {
                                field: format!("backends.{name}.timeout_secs"),
                                message: format!(
                                    "timeout {t}s out of range (1..{MAX_TIMEOUT_SECS})"
                                ),
                                severity: IssueSeverity::Error,
                            });
                        } else if *t > LARGE_TIMEOUT_THRESHOLD {
                            warnings.push(ConfigIssue {
                                field: format!("backends.{name}.timeout_secs"),
                                message: format!("large timeout ({t}s); consider reducing"),
                                severity: IssueSeverity::Warning,
                            });
                        }
                    }
                }
                BackendEntry::Mock {} => {}
            }
        }

        // -- missing optional fields -----------------------------------------
        if config.default_backend.is_none() {
            warnings.push(ConfigIssue {
                field: "default_backend".into(),
                message: "no default backend set; callers must always specify --backend".into(),
                severity: IssueSeverity::Warning,
            });
        }
        if config.receipts_dir.is_none() {
            warnings.push(ConfigIssue {
                field: "receipts_dir".into(),
                message: "receipts directory not configured; receipts will not be persisted".into(),
                severity: IssueSeverity::Warning,
            });
        }

        // -- empty path strings ----------------------------------------------
        if let Some(ref p) = config.workspace_dir {
            if p.trim().is_empty() {
                warnings.push(ConfigIssue {
                    field: "workspace_dir".into(),
                    message: "workspace_dir is set but empty".into(),
                    severity: IssueSeverity::Warning,
                });
            }
        }

        // -- default_backend references unknown backend ----------------------
        if let Some(ref name) = config.default_backend {
            if !config.backends.is_empty() && !config.backends.contains_key(name) {
                warnings.push(ConfigIssue {
                    field: "default_backend".into(),
                    message: format!(
                        "default_backend '{name}' does not match any configured backend"
                    ),
                    severity: IssueSeverity::Warning,
                });
                suggestions.push(format!(
                    "Set default_backend to one of: {}",
                    config
                        .backends
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }

        // -- suggestions -----------------------------------------------------
        if config.backends.is_empty() {
            suggestions.push("Consider adding at least one backend to the configuration.".into());
        }

        let valid = errors.is_empty();
        ConfigValidationResult {
            valid,
            errors,
            warnings,
            suggestions,
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigMerger
// ---------------------------------------------------------------------------

/// Struct-based entry point for merging two [`BackplaneConfig`] values.
///
/// Overlay values override base values; backend maps are combined with
/// overlay entries winning on key collision.
#[derive(Debug, Clone, Copy)]
pub struct ConfigMerger;

impl ConfigMerger {
    /// Merge `overlay` on top of `base`, returning the combined config.
    pub fn merge(base: &BackplaneConfig, overlay: &BackplaneConfig) -> BackplaneConfig {
        crate::merge_configs(base.clone(), overlay.clone())
    }
}

// ---------------------------------------------------------------------------
// ConfigChange
// ---------------------------------------------------------------------------

/// A single field-level change between two configs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigChange {
    /// Dotted field path (e.g. `"backends.mock"`).
    pub field: String,
    /// Previous value as a human-readable string.
    pub old_value: String,
    /// New value as a human-readable string.
    pub new_value: String,
}

impl fmt::Display for ConfigChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} -> {}",
            self.field, self.old_value, self.new_value
        )
    }
}

impl ConfigDiff {
    /// Compare two configs and return a list of [`ConfigChange`]s.
    pub fn diff(a: &BackplaneConfig, b: &BackplaneConfig) -> Vec<ConfigChange> {
        diff_configs(a, b)
            .into_iter()
            .map(|d| ConfigChange {
                field: d.path,
                old_value: d.old_value,
                new_value: d.new_value,
            })
            .collect()
    }
}
