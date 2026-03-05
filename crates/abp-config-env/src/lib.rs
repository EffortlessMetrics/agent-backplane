// SPDX-License-Identifier: MIT OR Apache-2.0
//! Environment-variable overlays for backplane configuration.
#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Environment-derived overrides for common backplane config fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackplaneEnvOverrides {
    /// Value from `ABP_DEFAULT_BACKEND`.
    pub default_backend: Option<String>,
    /// Value from `ABP_LOG_LEVEL`.
    pub log_level: Option<String>,
    /// Value from `ABP_RECEIPTS_DIR`.
    pub receipts_dir: Option<String>,
    /// Value from `ABP_WORKSPACE_DIR`.
    pub workspace_dir: Option<String>,
}

impl BackplaneEnvOverrides {
    /// Read all supported environment overrides.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            default_backend: std::env::var("ABP_DEFAULT_BACKEND").ok(),
            log_level: std::env::var("ABP_LOG_LEVEL").ok(),
            receipts_dir: std::env::var("ABP_RECEIPTS_DIR").ok(),
            workspace_dir: std::env::var("ABP_WORKSPACE_DIR").ok(),
        }
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;

    #[test]
    fn reads_known_env_vars() {
        unsafe {
            std::env::set_var("ABP_DEFAULT_BACKEND", "mock");
            std::env::set_var("ABP_LOG_LEVEL", "debug");
            std::env::set_var("ABP_RECEIPTS_DIR", "/tmp/receipts");
            std::env::set_var("ABP_WORKSPACE_DIR", "/tmp/workspace");
        }

        let vars = BackplaneEnvOverrides::from_env();
        assert_eq!(vars.default_backend.as_deref(), Some("mock"));
        assert_eq!(vars.log_level.as_deref(), Some("debug"));
        assert_eq!(vars.receipts_dir.as_deref(), Some("/tmp/receipts"));
        assert_eq!(vars.workspace_dir.as_deref(), Some("/tmp/workspace"));

        unsafe {
            std::env::remove_var("ABP_DEFAULT_BACKEND");
            std::env::remove_var("ABP_LOG_LEVEL");
            std::env::remove_var("ABP_RECEIPTS_DIR");
            std::env::remove_var("ABP_WORKSPACE_DIR");
        }
    }
}
