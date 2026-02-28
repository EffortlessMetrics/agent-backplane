// SPDX-License-Identifier: MIT OR Apache-2.0
//! Configuration loading and validation for the Agent Backplane CLI.

use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Top-level configuration for the agent backplane.
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
pub struct BackplaneConfig {
    #[serde(default)]
    pub backends: HashMap<String, BackendConfig>,
}

/// Configuration for a single backend.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum BackendConfig {
    #[serde(rename = "mock")]
    Mock {},
    #[serde(rename = "sidecar")]
    Sidecar {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Optional timeout in seconds for the sidecar process.
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
}

/// Errors found during configuration validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidBackend { name: String, reason: String },
    InvalidTimeout { value: u64 },
    MissingRequiredField { field: String },
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
pub fn load_config(path: &Path) -> anyhow::Result<BackplaneConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read config file '{}': {e}", path.display()))?;
    let config: BackplaneConfig = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse config file '{}': {e}", path.display()))?;
    Ok(config)
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
        };
        let errs = validate_config(&config).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::InvalidBackend { .. })));
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
        };
        let errs = validate_config(&config).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::InvalidTimeout { value: 0 })));
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
