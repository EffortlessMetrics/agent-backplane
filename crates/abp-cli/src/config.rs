// SPDX-License-Identifier: MIT OR Apache-2.0
//! Compatibility wrappers around `abp-config` for the Agent Backplane CLI.

use std::path::Path;

pub use abp_config::BackendEntry as BackendConfig;
pub use abp_config::BackplaneConfig;

/// Errors found during CLI configuration validation.
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

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

/// Load and parse a TOML configuration file.
pub fn load_config(path: Option<&Path>) -> anyhow::Result<BackplaneConfig> {
    abp_config::load_config(path).map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Merge two configurations. Values present in `overlay` take precedence.
pub fn merge_configs(base: BackplaneConfig, overlay: BackplaneConfig) -> BackplaneConfig {
    abp_config::merge_configs(base, overlay)
}

/// Apply environment variable overrides to a configuration.
pub fn apply_env_overrides(config: &mut BackplaneConfig) {
    abp_config::apply_env_overrides(config)
}

/// Validate a parsed configuration, returning semantic errors found.
pub fn validate_config(config: &BackplaneConfig) -> Result<(), Vec<ConfigError>> {
    match abp_config::validate_config(config) {
        Ok(_) => Ok(()),
        Err(abp_config::ConfigError::ValidationError { reasons }) => Err(reasons
            .iter()
            .map(|reason| map_validation_reason(reason))
            .collect()),
        Err(other) => Err(vec![ConfigError::InvalidBackend {
            name: "config".into(),
            reason: other.to_string(),
        }]),
    }
}

fn map_validation_reason(reason: &str) -> ConfigError {
    if reason.contains("name must not be empty") {
        return ConfigError::MissingRequiredField {
            field: "backend name".into(),
        };
    }

    if reason.contains("timeout") && reason.contains("out of range") {
        if let Some(value) = parse_timeout_value(reason) {
            return ConfigError::InvalidTimeout { value };
        }
    }

    let name = reason
        .split('"')
        .nth(1)
        .map(ToOwned::to_owned)
        .or_else(|| reason.split('\'').nth(1).map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".into());

    ConfigError::InvalidBackend {
        name,
        reason: reason.into(),
    }
}

fn parse_timeout_value(reason: &str) -> Option<u64> {
    let timeout_pos = reason.find("timeout ")?;
    let value_part = &reason[timeout_pos + "timeout ".len()..];
    let end = value_part.find('s')?;
    value_part[..end].parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn validate_empty_command_is_invalid() {
        let config = BackplaneConfig {
            backends: BTreeMap::from([(
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
            backends: BTreeMap::from([(
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
}
