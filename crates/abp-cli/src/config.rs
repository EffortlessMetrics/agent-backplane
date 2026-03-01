// SPDX-License-Identifier: MIT OR Apache-2.0
//! Configuration loading and validation for the Agent Backplane CLI.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Top-level configuration for the agent backplane.
#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct BackplaneConfig {
    /// Default backend name used when none is specified on the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_backend: Option<String>,
    /// Log level override (e.g. "debug", "info", "warn").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    /// Directory for storing receipt files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipts_dir: Option<String>,
    /// Named backend definitions.
    #[serde(default)]
    pub backends: HashMap<String, BackendConfig>,
}

/// Configuration for a single backend.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type")]
pub enum BackendConfig {
    /// A mock backend that returns synthetic responses.
    #[serde(rename = "mock")]
    Mock {},
    /// A sidecar process backend that communicates over JSONL stdio.
    #[serde(rename = "sidecar")]
    Sidecar {
        /// Executable path or command name for the sidecar process.
        command: String,
        /// Additional command-line arguments passed to the sidecar.
        #[serde(default)]
        args: Vec<String>,
        /// Optional timeout in seconds for the sidecar process.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
    },
}

/// Errors found during configuration validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A backend definition is semantically invalid.
    InvalidBackend {
        /// Name of the backend that failed validation.
        name: String,
        /// Human-readable explanation of what is wrong.
        reason: String,
    },
    /// A timeout value is out of the allowed range.
    InvalidTimeout {
        /// The invalid timeout value in seconds.
        value: u64,
    },
    /// A required configuration field is missing.
    MissingRequiredField {
        /// Name of the missing field.
        field: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidBackend { name, reason } => {
                write!(f, "invalid backend '{name}': {reason}")
            }
            ConfigError::InvalidTimeout { value } => {
                write!(f, "invalid timeout: {value}s (must be 1..86400)")
            }
            ConfigError::MissingRequiredField { field } => {
                write!(f, "missing required field: {field}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

const MAX_TIMEOUT_SECS: u64 = 86_400;

/// Load and parse a TOML configuration file.
///
/// If `path` is `None`, returns a default configuration with environment
/// variable overrides applied.  If `path` is `Some`, reads and parses the
/// file, then applies environment variable overrides on top.
pub fn load_config(path: Option<&Path>) -> anyhow::Result<BackplaneConfig> {
    let mut config = match path {
        Some(p) => {
            let content = std::fs::read_to_string(p).map_err(|e| {
                anyhow::anyhow!("failed to read config file '{}': {e}", p.display())
            })?;
            toml::from_str::<BackplaneConfig>(&content).map_err(|e| {
                anyhow::anyhow!("failed to parse config file '{}': {e}", p.display())
            })?
        }
        None => BackplaneConfig::default(),
    };
    apply_env_overrides(&mut config);
    Ok(config)
}

/// Merge two configurations.  Values present in `overlay` take precedence;
/// backends from both are combined (overlay wins on name collisions).
pub fn merge_configs(base: BackplaneConfig, overlay: BackplaneConfig) -> BackplaneConfig {
    let mut backends = base.backends;
    backends.extend(overlay.backends);
    BackplaneConfig {
        default_backend: overlay.default_backend.or(base.default_backend),
        log_level: overlay.log_level.or(base.log_level),
        receipts_dir: overlay.receipts_dir.or(base.receipts_dir),
        backends,
    }
}

/// Apply environment variable overrides to a configuration.
///
/// Recognised variables:
/// - `ABP_DEFAULT_BACKEND` — overrides `default_backend`
/// - `ABP_LOG_LEVEL` — overrides `log_level`
/// - `ABP_RECEIPTS_DIR` — overrides `receipts_dir`
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
}

/// Validate a parsed configuration, returning any semantic errors found.
pub fn validate_config(config: &BackplaneConfig) -> Result<(), Vec<ConfigError>> {
    let mut errors = Vec::new();

    for (name, backend) in &config.backends {
        if name.is_empty() {
            errors.push(ConfigError::MissingRequiredField {
                field: "backend name".into(),
            });
        }

        match backend {
            BackendConfig::Sidecar {
                command,
                timeout_secs,
                ..
            } => {
                if command.trim().is_empty() {
                    errors.push(ConfigError::InvalidBackend {
                        name: name.clone(),
                        reason: "sidecar command must not be empty".into(),
                    });
                }
                if let Some(t) = timeout_secs
                    && (*t == 0 || *t > MAX_TIMEOUT_SECS)
                {
                    errors.push(ConfigError::InvalidTimeout { value: *t });
                }
            }
            BackendConfig::Mock {} => {}
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_empty_command_is_invalid() {
        let config = BackplaneConfig {
            backends: HashMap::from([(
                "bad".into(),
                BackendConfig::Sidecar {
                    command: "  ".into(),
                    args: vec![],
                    timeout_secs: None,
                },
            )]),
            ..Default::default()
        };
        let errs = validate_config(&config).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::InvalidBackend { .. }))
        );
    }

    #[test]
    fn validate_zero_timeout_is_invalid() {
        let config = BackplaneConfig {
            backends: HashMap::from([(
                "s".into(),
                BackendConfig::Sidecar {
                    command: "node".into(),
                    args: vec![],
                    timeout_secs: Some(0),
                },
            )]),
            ..Default::default()
        };
        let errs = validate_config(&config).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ConfigError::InvalidTimeout { value: 0 }))
        );
    }

    #[test]
    fn validate_valid_config_succeeds() {
        let config = BackplaneConfig {
            backends: HashMap::from([
                ("mock".into(), BackendConfig::Mock {}),
                (
                    "sc".into(),
                    BackendConfig::Sidecar {
                        command: "node".into(),
                        args: vec!["host.js".into()],
                        timeout_secs: Some(300),
                    },
                ),
            ]),
            ..Default::default()
        };
        validate_config(&config).unwrap();
    }

    #[test]
    fn display_config_errors() {
        let e = ConfigError::InvalidBackend {
            name: "x".into(),
            reason: "bad".into(),
        };
        assert_eq!(e.to_string(), "invalid backend 'x': bad");

        let e = ConfigError::InvalidTimeout { value: 0 };
        assert!(e.to_string().contains("invalid timeout"));

        let e = ConfigError::MissingRequiredField {
            field: "name".into(),
        };
        assert!(e.to_string().contains("missing required field"));
    }
}
