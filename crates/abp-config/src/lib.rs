// SPDX-License-Identifier: MIT OR Apache-2.0
//! Configuration loading, validation, and merging for the Agent Backplane.
//!
//! This crate provides [`BackplaneConfig`] — the top-level runtime settings —
//! together with helpers for loading from TOML files, merging overlays, and
//! producing advisory [`ConfigWarning`]s.
#![deny(unsafe_code)]
#![warn(missing_docs)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during configuration loading or validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The requested configuration file was not found.
    #[error("config file not found: {path}")]
    FileNotFound {
        /// Path that was requested.
        path: String,
    },

    /// The file could not be parsed as valid TOML.
    #[error("failed to parse config: {reason}")]
    ParseError {
        /// Human-readable parse error detail.
        reason: String,
    },

    /// Semantic validation failed (one or more problems).
    #[error("config validation failed: {reasons:?}")]
    ValidationError {
        /// Individual validation failure messages.
        reasons: Vec<String>,
    },

    /// Two configs could not be merged because of conflicting constraints.
    #[error("merge conflict: {reason}")]
    MergeConflict {
        /// Description of the conflict.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Warnings
// ---------------------------------------------------------------------------

/// Advisory-level issues that do not prevent operation but deserve attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigWarning {
    /// A deprecated field was used in the configuration.
    DeprecatedField {
        /// Name of the deprecated field.
        field: String,
        /// Suggested replacement, if any.
        suggestion: Option<String>,
    },
    /// A recommended optional field is missing.
    MissingOptionalField {
        /// Name of the missing field.
        field: String,
        /// Why it matters.
        hint: String,
    },
    /// A backend timeout is unusually large.
    LargeTimeout {
        /// Backend name.
        backend: String,
        /// Timeout value in seconds.
        secs: u64,
    },
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigWarning::DeprecatedField { field, suggestion } => {
                write!(f, "deprecated field '{field}'")?;
                if let Some(s) = suggestion {
                    write!(f, " — use '{s}' instead")?;
                }
                Ok(())
            }
            ConfigWarning::MissingOptionalField { field, hint } => {
                write!(f, "missing optional field '{field}': {hint}")
            }
            ConfigWarning::LargeTimeout { backend, secs } => {
                write!(f, "backend '{backend}' has a large timeout ({secs}s)")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

/// Top-level runtime configuration for the Agent Backplane.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct BackplaneConfig {
    /// Default backend name when none is specified on the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_backend: Option<String>,

    /// Working directory used for staged workspaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_dir: Option<String>,

    /// Log level override (e.g. `"debug"`, `"info"`, `"warn"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Directory for persisting receipt JSON files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipts_dir: Option<String>,

    /// Named backend definitions.
    #[serde(default)]
    pub backends: BTreeMap<String, BackendEntry>,
}

impl Default for BackplaneConfig {
    fn default() -> Self {
        Self {
            default_backend: None,
            workspace_dir: None,
            log_level: Some("info".into()),
            receipts_dir: None,
            backends: BTreeMap::new(),
        }
    }
}

/// Configuration for a single backend.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum BackendEntry {
    /// A mock backend (for testing).
    #[serde(rename = "mock")]
    Mock {},
    /// A sidecar process backend.
    #[serde(rename = "sidecar")]
    Sidecar {
        /// Command to spawn.
        command: String,
        /// Extra CLI arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Optional timeout in seconds (1–86 400).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
    },
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum allowed timeout in seconds (24 hours).
const MAX_TIMEOUT_SECS: u64 = 86_400;

/// Threshold above which a timeout generates a warning.
const LARGE_TIMEOUT_THRESHOLD: u64 = 3_600;

/// Recognised log levels.
const VALID_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load a [`BackplaneConfig`] from an optional TOML file path.
///
/// * If `path` is `Some`, reads and parses the file.
/// * If `path` is `None`, returns [`BackplaneConfig::default()`].
///
/// Environment variable overrides are applied on top in both cases.
pub fn load_config(path: Option<&Path>) -> Result<BackplaneConfig, ConfigError> {
    let mut config = match path {
        Some(p) => {
            let content = std::fs::read_to_string(p).map_err(|_| ConfigError::FileNotFound {
                path: p.display().to_string(),
            })?;
            parse_toml(&content)?
        }
        None => BackplaneConfig::default(),
    };
    apply_env_overrides(&mut config);
    Ok(config)
}

/// Parse a TOML string into a [`BackplaneConfig`].
pub fn parse_toml(content: &str) -> Result<BackplaneConfig, ConfigError> {
    toml::from_str::<BackplaneConfig>(content).map_err(|e| ConfigError::ParseError {
        reason: e.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Env overrides
// ---------------------------------------------------------------------------

/// Apply environment variable overrides.
///
/// Recognised variables:
/// - `ABP_DEFAULT_BACKEND`
/// - `ABP_LOG_LEVEL`
/// - `ABP_RECEIPTS_DIR`
/// - `ABP_WORKSPACE_DIR`
pub fn apply_env_overrides(config: &mut BackplaneConfig) {
    if let Ok(val) = std::env::var("ABP_DEFAULT_BACKEND") {
        config.default_backend = Some(val);
    }
    if let Ok(val) = std::env::var("ABP_LOG_LEVEL") {
        config.log_level = Some(val);
    }
    if let Ok(val) = std::env::var("ABP_RECEIPTS_DIR") {
        config.receipts_dir = Some(val);
    }
    if let Ok(val) = std::env::var("ABP_WORKSPACE_DIR") {
        config.workspace_dir = Some(val);
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a parsed configuration, returning advisory warnings.
///
/// Hard errors (empty sidecar commands, out-of-range timeouts) are returned
/// as a [`ConfigError::ValidationError`]; soft issues come back as warnings.
pub fn validate_config(config: &BackplaneConfig) -> Result<Vec<ConfigWarning>, ConfigError> {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<ConfigWarning> = Vec::new();

    // Validate log_level value.
    if let Some(ref level) = config.log_level
        && !VALID_LOG_LEVELS.contains(&level.as_str())
    {
        errors.push(format!("invalid log_level '{level}'"));
    }

    // Validate each backend entry.
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
                        warnings.push(ConfigWarning::LargeTimeout {
                            backend: name.clone(),
                            secs: *t,
                        });
                    }
                }
            }
            BackendEntry::Mock {} => {}
        }
    }

    // Advisory: missing optional fields.
    if config.default_backend.is_none() {
        warnings.push(ConfigWarning::MissingOptionalField {
            field: "default_backend".into(),
            hint: "callers must always specify --backend explicitly".into(),
        });
    }
    if config.receipts_dir.is_none() {
        warnings.push(ConfigWarning::MissingOptionalField {
            field: "receipts_dir".into(),
            hint: "receipts will not be persisted to disk".into(),
        });
    }

    if errors.is_empty() {
        Ok(warnings)
    } else {
        Err(ConfigError::ValidationError { reasons: errors })
    }
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

/// Merge two configurations.  Values in `overlay` take precedence over `base`.
///
/// Backend maps are combined; on name collisions the overlay entry wins.
pub fn merge_configs(base: BackplaneConfig, overlay: BackplaneConfig) -> BackplaneConfig {
    let mut backends = base.backends;
    backends.extend(overlay.backends);
    BackplaneConfig {
        default_backend: overlay.default_backend.or(base.default_backend),
        workspace_dir: overlay.workspace_dir.or(base.workspace_dir),
        log_level: overlay.log_level.or(base.log_level),
        receipts_dir: overlay.receipts_dir.or(base.receipts_dir),
        backends,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use std::io::Write;

    // -- 1. Default config is valid ------------------------------------------

    #[test]
    fn default_config_is_valid() {
        let cfg = BackplaneConfig::default();
        let warnings = validate_config(&cfg).expect("default config should be valid");
        assert!(!warnings.is_empty(), "should have advisory warnings");
    }

    // -- 2. Default config has sensible defaults -----------------------------

    #[test]
    fn default_config_has_sensible_defaults() {
        let cfg = BackplaneConfig::default();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
        assert!(cfg.backends.is_empty());
    }

    // -- 3. Load from valid TOML string --------------------------------------

    #[test]
    fn parse_valid_toml_string() {
        let toml = r#"
            default_backend = "mock"
            log_level = "debug"
            receipts_dir = "/tmp/receipts"

            [backends.mock]
            type = "mock"
        "#;
        let cfg = parse_toml(toml).unwrap();
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
        assert_eq!(cfg.log_level.as_deref(), Some("debug"));
        assert_eq!(cfg.backends.len(), 1);
    }

    // -- 4. Load from invalid TOML produces ParseError -----------------------

    #[test]
    fn parse_invalid_toml_gives_parse_error() {
        let bad = "this is [not valid toml =";
        let err = parse_toml(bad).unwrap_err();
        assert!(matches!(err, ConfigError::ParseError { .. }));
    }

    // -- 5. Valid TOML but wrong types gives ParseError ----------------------

    #[test]
    fn parse_wrong_types_gives_parse_error() {
        let toml = r#"log_level = 42"#;
        let err = parse_toml(toml).unwrap_err();
        assert!(matches!(err, ConfigError::ParseError { .. }));
    }

    // -- 6. Validation catches invalid log level -----------------------------

    #[test]
    fn validation_catches_invalid_log_level() {
        let cfg = BackplaneConfig {
            log_level: Some("verbose".into()),
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::ValidationError { .. }));
    }

    // -- 7. Validation catches empty sidecar command -------------------------

    #[test]
    fn validation_catches_empty_sidecar_command() {
        let mut cfg = BackplaneConfig::default();
        cfg.backends.insert(
            "bad".into(),
            BackendEntry::Sidecar {
                command: "  ".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let err = validate_config(&cfg).unwrap_err();
        match err {
            ConfigError::ValidationError { reasons } => {
                assert!(
                    reasons
                        .iter()
                        .any(|r| r.contains("command must not be empty"))
                );
            }
            other => panic!("expected ValidationError, got {other:?}"),
        }
    }

    // -- 8. Validation catches zero timeout ----------------------------------

    #[test]
    fn validation_catches_zero_timeout() {
        let mut cfg = BackplaneConfig::default();
        cfg.backends.insert(
            "s".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        );
        let err = validate_config(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::ValidationError { .. }));
    }

    // -- 9. Validation catches timeout exceeding max -------------------------

    #[test]
    fn validation_catches_timeout_exceeding_max() {
        let mut cfg = BackplaneConfig::default();
        cfg.backends.insert(
            "s".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(MAX_TIMEOUT_SECS + 1),
            },
        );
        let err = validate_config(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::ValidationError { .. }));
    }

    // -- 10. Valid config with backends passes validation ---------------------

    #[test]
    fn valid_config_with_backends_passes() {
        let mut cfg = BackplaneConfig::default();
        cfg.backends.insert("mock".into(), BackendEntry::Mock {});
        cfg.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: Some(300),
            },
        );
        validate_config(&cfg).expect("should pass");
    }

    // -- 11. Large timeout produces warning ----------------------------------

    #[test]
    fn large_timeout_produces_warning() {
        let mut cfg = BackplaneConfig::default();
        cfg.default_backend = Some("sc".into());
        cfg.receipts_dir = Some("/tmp".into());
        cfg.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(7200),
            },
        );
        let warnings = validate_config(&cfg).unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
        );
    }

    // -- 12. Merge overlay overrides base values -----------------------------

    #[test]
    fn merge_overlay_overrides_base() {
        let base = BackplaneConfig {
            default_backend: Some("mock".into()),
            log_level: Some("info".into()),
            ..Default::default()
        };
        let overlay = BackplaneConfig {
            default_backend: Some("openai".into()),
            log_level: None,
            ..Default::default()
        };
        let merged = merge_configs(base, overlay);
        assert_eq!(merged.default_backend.as_deref(), Some("openai"));
        assert_eq!(merged.log_level.as_deref(), Some("info"));
    }

    // -- 13. Merge preserves base when overlay is default --------------------

    #[test]
    fn merge_preserves_base_when_overlay_is_default() {
        let base = BackplaneConfig {
            default_backend: Some("mock".into()),
            workspace_dir: Some("/work".into()),
            log_level: Some("debug".into()),
            receipts_dir: Some("/receipts".into()),
            backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        };
        let merged = merge_configs(base.clone(), BackplaneConfig::default());
        // overlay log_level is Some("info"), so it will override base.
        assert_eq!(merged.default_backend.as_deref(), Some("mock"));
        assert_eq!(merged.workspace_dir.as_deref(), Some("/work"));
        assert_eq!(merged.receipts_dir.as_deref(), Some("/receipts"));
        assert!(merged.backends.contains_key("m"));
    }

    // -- 14. Merge combines backend maps -------------------------------------

    #[test]
    fn merge_combines_backend_maps() {
        let base = BackplaneConfig {
            backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
            ..Default::default()
        };
        let overlay = BackplaneConfig {
            backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
            ..Default::default()
        };
        let merged = merge_configs(base, overlay);
        assert!(merged.backends.contains_key("a"));
        assert!(merged.backends.contains_key("b"));
    }

    // -- 15. Merge overlay backend wins on collision -------------------------

    #[test]
    fn merge_overlay_backend_wins_on_collision() {
        let base = BackplaneConfig {
            backends: BTreeMap::from([(
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "python".into(),
                    args: vec![],
                    timeout_secs: None,
                },
            )]),
            ..Default::default()
        };
        let overlay = BackplaneConfig {
            backends: BTreeMap::from([(
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(60),
                },
            )]),
            ..Default::default()
        };
        let merged = merge_configs(base, overlay);
        match &merged.backends["sc"] {
            BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
            other => panic!("expected Sidecar, got {other:?}"),
        }
    }

    // -- 16. Empty string TOML is valid (all defaults) -----------------------

    #[test]
    fn empty_string_toml_parses_to_defaults() {
        let cfg = parse_toml("").unwrap();
        assert_eq!(cfg.default_backend, None);
        assert!(cfg.backends.is_empty());
    }

    // -- 17. Roundtrip serialize / deserialize -------------------------------

    #[test]
    fn toml_roundtrip() {
        let cfg = BackplaneConfig {
            default_backend: Some("mock".into()),
            workspace_dir: Some("/ws".into()),
            log_level: Some("debug".into()),
            receipts_dir: Some("/r".into()),
            backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        };
        let serialized = toml::to_string(&cfg).unwrap();
        let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg, deserialized);
    }

    // -- 18. Load from file on disk ------------------------------------------

    #[test]
    fn load_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backplane.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "default_backend = \"mock\"\nlog_level = \"warn\"").unwrap();
        let cfg = load_config(Some(&path)).unwrap();
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
        assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    }

    // -- 19. Load missing file gives FileNotFound ----------------------------

    #[test]
    fn load_missing_file_gives_file_not_found() {
        let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
        assert!(matches!(err, ConfigError::FileNotFound { .. }));
    }

    // -- 20. Load None path returns default config ---------------------------

    #[test]
    fn load_none_returns_default() {
        let cfg = load_config(None).unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
    }

    // -- 21. Validation catches empty backend name ---------------------------

    #[test]
    fn validation_catches_empty_backend_name() {
        let mut cfg = BackplaneConfig::default();
        cfg.backends.insert("".into(), BackendEntry::Mock {});
        let err = validate_config(&cfg).unwrap_err();
        match err {
            ConfigError::ValidationError { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
            }
            other => panic!("expected ValidationError, got {other:?}"),
        }
    }

    // -- 22. ConfigError Display trait ----------------------------------------

    #[test]
    fn config_error_display() {
        let e = ConfigError::FileNotFound {
            path: "/foo".into(),
        };
        assert!(e.to_string().contains("/foo"));

        let e = ConfigError::ParseError {
            reason: "bad toml".into(),
        };
        assert!(e.to_string().contains("bad toml"));

        let e = ConfigError::MergeConflict {
            reason: "oops".into(),
        };
        assert!(e.to_string().contains("oops"));
    }

    // -- 23. ConfigWarning Display trait --------------------------------------

    #[test]
    fn config_warning_display() {
        let w = ConfigWarning::DeprecatedField {
            field: "old_field".into(),
            suggestion: Some("new_field".into()),
        };
        let s = w.to_string();
        assert!(s.contains("old_field"));
        assert!(s.contains("new_field"));

        let w = ConfigWarning::DeprecatedField {
            field: "old".into(),
            suggestion: None,
        };
        assert!(w.to_string().contains("old"));

        let w = ConfigWarning::MissingOptionalField {
            field: "f".into(),
            hint: "h".into(),
        };
        assert!(w.to_string().contains('f'));

        let w = ConfigWarning::LargeTimeout {
            backend: "b".into(),
            secs: 9999,
        };
        assert!(w.to_string().contains("9999"));
    }

    // -- 24. Nested sidecar args roundtrip -----------------------------------

    #[test]
    fn sidecar_with_args_roundtrip() {
        let toml_str = r#"
            [backends.node]
            type = "sidecar"
            command = "node"
            args = ["--experimental", "host.js"]
            timeout_secs = 120
        "#;
        let cfg = parse_toml(toml_str).unwrap();
        match &cfg.backends["node"] {
            BackendEntry::Sidecar {
                command,
                args,
                timeout_secs,
            } => {
                assert_eq!(command, "node");
                assert_eq!(args, &["--experimental", "host.js"]);
                assert_eq!(*timeout_secs, Some(120));
            }
            other => panic!("expected Sidecar, got {other:?}"),
        }
    }

    // -- 25. Merge workspace_dir overlay wins --------------------------------

    #[test]
    fn merge_workspace_dir_overlay_wins() {
        let base = BackplaneConfig {
            workspace_dir: Some("/old".into()),
            ..Default::default()
        };
        let overlay = BackplaneConfig {
            workspace_dir: Some("/new".into()),
            ..Default::default()
        };
        let merged = merge_configs(base, overlay);
        assert_eq!(merged.workspace_dir.as_deref(), Some("/new"));
    }
}
