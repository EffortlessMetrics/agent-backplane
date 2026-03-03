// SPDX-License-Identifier: MIT OR Apache-2.0
//! Structured validation, diffing, and environment-override helpers.
//!
//! This module adds a [`ConfigValidator`] (struct-based entry point),
//! [`diff_configs`] for debugging, and [`from_env_overrides`] as a
//! convenience re-export.

use crate::{BackendEntry, BackplaneConfig, ConfigError};
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
// Constants (mirrored from lib so this module is self-contained)
// ---------------------------------------------------------------------------

/// Maximum allowed timeout in seconds (24 hours).
const MAX_TIMEOUT_SECS: u64 = 86_400;

/// Threshold above which a timeout generates a warning.
const LARGE_TIMEOUT_THRESHOLD: u64 = 3_600;

/// Recognised log levels.
const VALID_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

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
