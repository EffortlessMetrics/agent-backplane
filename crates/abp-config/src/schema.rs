// SPDX-License-Identifier: MIT OR Apache-2.0
//! Schema-level description and validation for [`BackplaneConfig`] fields.
//!
//! [`ConfigSchema`] enumerates every valid field together with its expected
//! type, default value, and constraints. [`validate_against_schema`] checks a
//! config instance against those rules and returns a list of
//! [`ValidationIssue`]s from [`crate::validate`].

use crate::validate::{Severity, ValidationIssue};
use crate::{BackendEntry, BackplaneConfig, LARGE_TIMEOUT_THRESHOLD, MAX_TIMEOUT_SECS, VALID_LOG_LEVELS};
use std::fmt;

// ---------------------------------------------------------------------------
// FieldType
// ---------------------------------------------------------------------------

/// The expected JSON/TOML type for a config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// A UTF-8 string.
    String,
    /// An unsigned 16-bit integer.
    U16,
    /// An unsigned 64-bit integer.
    U64,
    /// An array of strings.
    StringArray,
    /// A map of named backend entries.
    BackendMap,
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::String => f.write_str("string"),
            FieldType::U16 => f.write_str("u16"),
            FieldType::U64 => f.write_str("u64"),
            FieldType::StringArray => f.write_str("string[]"),
            FieldType::BackendMap => f.write_str("map<string, BackendEntry>"),
        }
    }
}

// ---------------------------------------------------------------------------
// FieldSchema
// ---------------------------------------------------------------------------

/// Description of a single configuration field.
#[derive(Debug, Clone)]
pub struct FieldSchema {
    /// Dot-separated path (e.g. `"log_level"`).
    pub name: &'static str,
    /// Expected value type.
    pub field_type: FieldType,
    /// Whether the field is required for a valid config.
    pub required: bool,
    /// Human-readable default value (if any).
    pub default: Option<&'static str>,
    /// Human-readable constraint description.
    pub constraint: Option<&'static str>,
    /// Short documentation string.
    pub description: &'static str,
}

// ---------------------------------------------------------------------------
// ConfigSchema
// ---------------------------------------------------------------------------

/// Describes every valid field in [`BackplaneConfig`].
pub struct ConfigSchema;

impl ConfigSchema {
    /// Return the full list of field schemas.
    pub fn fields() -> Vec<FieldSchema> {
        vec![
            FieldSchema {
                name: "default_backend",
                field_type: FieldType::String,
                required: false,
                default: None,
                constraint: Some("must match a key in `backends` if set"),
                description: "Default backend name when none is specified on the CLI",
            },
            FieldSchema {
                name: "workspace_dir",
                field_type: FieldType::String,
                required: false,
                default: None,
                constraint: None,
                description: "Working directory used for staged workspaces",
            },
            FieldSchema {
                name: "log_level",
                field_type: FieldType::String,
                required: false,
                default: Some("info"),
                constraint: Some("one of: error, warn, info, debug, trace"),
                description: "Log level override",
            },
            FieldSchema {
                name: "receipts_dir",
                field_type: FieldType::String,
                required: false,
                default: None,
                constraint: None,
                description: "Directory for persisting receipt JSON files",
            },
            FieldSchema {
                name: "bind_address",
                field_type: FieldType::String,
                required: false,
                default: None,
                constraint: Some("valid IP address or hostname"),
                description: "Network bind address",
            },
            FieldSchema {
                name: "port",
                field_type: FieldType::U16,
                required: false,
                default: None,
                constraint: Some("1..65535"),
                description: "Network port number",
            },
            FieldSchema {
                name: "policy_profiles",
                field_type: FieldType::StringArray,
                required: false,
                default: Some("[]"),
                constraint: None,
                description: "Paths to policy profile files loaded at startup",
            },
            FieldSchema {
                name: "backends",
                field_type: FieldType::BackendMap,
                required: false,
                default: Some("{}"),
                constraint: None,
                description: "Named backend definitions",
            },
        ]
    }

    /// Look up the schema for a single field by name.
    pub fn field(name: &str) -> Option<FieldSchema> {
        Self::fields().into_iter().find(|f| f.name == name)
    }
}

// ---------------------------------------------------------------------------
// validate_against_schema
// ---------------------------------------------------------------------------

/// Validate `config` against the schema rules and return all findings.
///
/// This is a schema-oriented validator: it checks type-level and
/// constraint-level invariants described by [`ConfigSchema`] rather than
/// higher-level business rules.
pub fn validate_against_schema(config: &BackplaneConfig) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // -- log_level -----------------------------------------------------------
    if let Some(ref level) = config.log_level {
        if !VALID_LOG_LEVELS.contains(&level.as_str()) {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: format!(
                    "log_level: '{}' is not one of: {}",
                    level,
                    VALID_LOG_LEVELS.join(", ")
                ),
            });
        }
    }

    // -- port ----------------------------------------------------------------
    if let Some(p) = config.port {
        if p == 0 {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: "port: must be between 1 and 65535".into(),
            });
        }
    }

    // -- bind_address --------------------------------------------------------
    if let Some(ref addr) = config.bind_address {
        if addr.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: "bind_address: must not be empty".into(),
            });
        } else if addr.parse::<std::net::IpAddr>().is_err() && !crate::is_valid_hostname(addr) {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: format!("bind_address: '{addr}' is not a valid IP or hostname"),
            });
        }
    }

    // -- policy_profiles -----------------------------------------------------
    for (i, path) in config.policy_profiles.iter().enumerate() {
        if path.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: format!("policy_profiles[{i}]: path must not be empty"),
            });
        }
    }

    // -- backends ------------------------------------------------------------
    for (name, backend) in &config.backends {
        if name.is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: "backends: backend name must not be empty".into(),
            });
        }

        match backend {
            BackendEntry::Sidecar {
                command,
                timeout_secs,
                ..
            } => {
                if command.trim().is_empty() {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        message: format!("backends.{name}.command: must not be empty"),
                    });
                }
                if let Some(t) = timeout_secs {
                    if *t == 0 || *t > MAX_TIMEOUT_SECS {
                        issues.push(ValidationIssue {
                            severity: Severity::Error,
                            message: format!(
                                "backends.{name}.timeout_secs: {t} out of range (1..{MAX_TIMEOUT_SECS})"
                            ),
                        });
                    } else if *t > LARGE_TIMEOUT_THRESHOLD {
                        issues.push(ValidationIssue {
                            severity: Severity::Warning,
                            message: format!(
                                "backends.{name}.timeout_secs: large timeout ({t}s)"
                            ),
                        });
                    }
                }
            }
            BackendEntry::Mock {} => {}
        }
    }

    // -- default_backend references ------------------------------------------
    if let Some(ref name) = config.default_backend {
        if !config.backends.is_empty() && !config.backends.contains_key(name) {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: format!(
                    "default_backend: '{name}' does not match any configured backend"
                ),
            });
        }
    }

    // -- missing optional advisories -----------------------------------------
    if config.default_backend.is_none() {
        issues.push(ValidationIssue {
            severity: Severity::Info,
            message: "default_backend not set; callers must specify --backend".into(),
        });
    }
    if config.receipts_dir.is_none() {
        issues.push(ValidationIssue {
            severity: Severity::Info,
            message: "receipts_dir not set; receipts will not be persisted".into(),
        });
    }

    issues
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn minimal_valid_config() -> BackplaneConfig {
        BackplaneConfig {
            default_backend: Some("mock".into()),
            workspace_dir: None,
            log_level: Some("info".into()),
            receipts_dir: Some("/tmp/r".into()),
            bind_address: None,
            port: None,
            policy_profiles: Vec::new(),
            backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        }
    }

    // -- ConfigSchema fields --------------------------------------------------

    #[test]
    fn schema_has_all_top_level_fields() {
        let fields = ConfigSchema::fields();
        let names: Vec<&str> = fields.iter().map(|f| f.name).collect();
        assert!(names.contains(&"default_backend"));
        assert!(names.contains(&"workspace_dir"));
        assert!(names.contains(&"log_level"));
        assert!(names.contains(&"receipts_dir"));
        assert!(names.contains(&"bind_address"));
        assert!(names.contains(&"port"));
        assert!(names.contains(&"policy_profiles"));
        assert!(names.contains(&"backends"));
    }

    #[test]
    fn schema_field_lookup() {
        let f = ConfigSchema::field("log_level").unwrap();
        assert_eq!(f.field_type, FieldType::String);
        assert_eq!(f.default, Some("info"));
    }

    #[test]
    fn schema_field_lookup_missing() {
        assert!(ConfigSchema::field("nonexistent").is_none());
    }

    #[test]
    fn schema_port_field_has_constraint() {
        let f = ConfigSchema::field("port").unwrap();
        assert!(f.constraint.is_some());
    }

    // -- validate_against_schema: valid configs -------------------------------

    #[test]
    fn valid_config_has_no_errors() {
        let issues = validate_against_schema(&minimal_valid_config());
        assert!(
            !issues.iter().any(|i| i.severity == Severity::Error),
            "no errors expected: {issues:?}"
        );
    }

    #[test]
    fn default_config_has_info_issues_only() {
        let issues = validate_against_schema(&BackplaneConfig::default());
        // Only info/warning — no hard errors.
        for issue in &issues {
            assert_ne!(issue.severity, Severity::Error, "unexpected error: {issue}");
        }
    }

    // -- validate_against_schema: invalid configs -----------------------------

    #[test]
    fn schema_catches_invalid_log_level() {
        let mut cfg = minimal_valid_config();
        cfg.log_level = Some("verbose".into());
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("log_level")));
    }

    #[test]
    fn schema_catches_zero_port() {
        let mut cfg = minimal_valid_config();
        cfg.port = Some(0);
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("port")));
    }

    #[test]
    fn schema_catches_empty_bind_address() {
        let mut cfg = minimal_valid_config();
        cfg.bind_address = Some("".into());
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("bind_address")));
    }

    #[test]
    fn schema_catches_invalid_bind_address() {
        let mut cfg = minimal_valid_config();
        cfg.bind_address = Some("not valid!".into());
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("bind_address")));
    }

    #[test]
    fn schema_catches_empty_policy_profile() {
        let mut cfg = minimal_valid_config();
        cfg.policy_profiles = vec!["  ".into()];
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("policy_profiles")));
    }

    #[test]
    fn schema_catches_empty_backend_name() {
        let mut cfg = minimal_valid_config();
        cfg.backends.insert("".into(), BackendEntry::Mock {});
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("backend name")));
    }

    #[test]
    fn schema_catches_empty_sidecar_command() {
        let mut cfg = minimal_valid_config();
        cfg.backends.insert(
            "bad".into(),
            BackendEntry::Sidecar { command: "  ".into(), args: vec![], timeout_secs: None },
        );
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("command")));
    }

    #[test]
    fn schema_catches_zero_timeout() {
        let mut cfg = minimal_valid_config();
        cfg.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar { command: "node".into(), args: vec![], timeout_secs: Some(0) },
        );
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("timeout")));
    }

    #[test]
    fn schema_catches_excessive_timeout() {
        let mut cfg = minimal_valid_config();
        cfg.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(MAX_TIMEOUT_SECS + 1),
            },
        );
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("timeout")));
    }

    #[test]
    fn schema_warns_on_large_timeout() {
        let mut cfg = minimal_valid_config();
        cfg.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(LARGE_TIMEOUT_THRESHOLD + 1),
            },
        );
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Warning && i.message.contains("large timeout")));
    }

    #[test]
    fn schema_warns_unresolved_default_backend() {
        let mut cfg = minimal_valid_config();
        cfg.default_backend = Some("nonexistent".into());
        let issues = validate_against_schema(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Warning && i.message.contains("nonexistent")));
    }

    #[test]
    fn field_type_display() {
        assert_eq!(FieldType::String.to_string(), "string");
        assert_eq!(FieldType::U16.to_string(), "u16");
        assert_eq!(FieldType::U64.to_string(), "u64");
        assert_eq!(FieldType::StringArray.to_string(), "string[]");
        assert_eq!(FieldType::BackendMap.to_string(), "map<string, BackendEntry>");
    }
}
