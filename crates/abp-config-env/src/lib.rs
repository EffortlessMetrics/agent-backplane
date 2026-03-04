// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Environment variable used to override default backend selection.
pub const DEFAULT_BACKEND_ENV: &str = "ABP_DEFAULT_BACKEND";

/// Environment variable used to override log level.
pub const LOG_LEVEL_ENV: &str = "ABP_LOG_LEVEL";

/// Environment variable used to override receipts directory.
pub const RECEIPTS_DIR_ENV: &str = "ABP_RECEIPTS_DIR";

/// Environment variable used to override workspace directory.
pub const WORKSPACE_DIR_ENV: &str = "ABP_WORKSPACE_DIR";

/// Resolved environment overrides for Agent Backplane runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigEnvOverrides {
    /// Optional override for default backend selection.
    pub default_backend: Option<String>,
    /// Optional override for runtime log level.
    pub log_level: Option<String>,
    /// Optional override for receipts output directory.
    pub receipts_dir: Option<String>,
    /// Optional override for workspace directory.
    pub workspace_dir: Option<String>,
}

/// Read known configuration overrides from process environment variables.
pub fn read_config_env_overrides() -> ConfigEnvOverrides {
    read_config_env_overrides_from(|key| std::env::var(key).ok())
}

/// Build configuration overrides using a caller-provided key lookup function.
pub fn read_config_env_overrides_from<F>(mut read_var: F) -> ConfigEnvOverrides
where
    F: FnMut(&str) -> Option<String>,
{
    ConfigEnvOverrides {
        default_backend: read_var(DEFAULT_BACKEND_ENV),
        log_level: read_var(LOG_LEVEL_ENV),
        receipts_dir: read_var(RECEIPTS_DIR_ENV),
        workspace_dir: read_var(WORKSPACE_DIR_ENV),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_when_variables_are_missing() {
        let got = read_config_env_overrides_from(|_| None);
        assert_eq!(got, ConfigEnvOverrides::default());
    }

    #[test]
    fn returns_values_for_present_variables() {
        let got = read_config_env_overrides_from(|key| match key {
            DEFAULT_BACKEND_ENV => Some("mock".into()),
            LOG_LEVEL_ENV => Some("trace".into()),
            RECEIPTS_DIR_ENV => Some("/tmp/receipts".into()),
            WORKSPACE_DIR_ENV => Some("/tmp/workspace".into()),
            _ => None,
        });

        assert_eq!(got.default_backend.as_deref(), Some("mock"));
        assert_eq!(got.log_level.as_deref(), Some("trace"));
        assert_eq!(got.receipts_dir.as_deref(), Some("/tmp/receipts"));
        assert_eq!(got.workspace_dir.as_deref(), Some("/tmp/workspace"));
    }
}
