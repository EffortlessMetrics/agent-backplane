// SPDX-License-Identifier: MIT OR Apache-2.0
//! Structured environment variable override support.
//!
//! [`EnvOverrides`] collects all recognised `ABP_*` environment variables
//! and provides [`apply_env_overrides`] to patch a [`BackplaneConfig`]
//! in-place.

use crate::BackplaneConfig;

// ---------------------------------------------------------------------------
// EnvOverrides
// ---------------------------------------------------------------------------

/// Snapshot of environment variable values that can override config fields.
///
/// Construct with [`EnvOverrides::from_env`] (reads `ABP_*` vars) or
/// [`EnvOverrides::from_env_with_prefix`] for a custom prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvOverrides {
    /// `ABP_DEFAULT_BACKEND`
    pub default_backend: Option<String>,
    /// `ABP_LOG_LEVEL`
    pub log_level: Option<String>,
    /// `ABP_RECEIPTS_DIR`
    pub receipts_dir: Option<String>,
    /// `ABP_WORKSPACE_DIR`
    pub workspace_dir: Option<String>,
    /// `ABP_BIND_ADDRESS`
    pub bind_address: Option<String>,
    /// `ABP_PORT`
    pub port: Option<u16>,
}

impl EnvOverrides {
    /// Read overrides from environment variables with the default `ABP_` prefix.
    pub fn from_env() -> Self {
        Self::from_env_with_prefix("ABP")
    }

    /// Read overrides from environment variables with a custom prefix.
    pub fn from_env_with_prefix(prefix: &str) -> Self {
        Self {
            default_backend: std::env::var(format!("{prefix}_DEFAULT_BACKEND")).ok(),
            log_level: std::env::var(format!("{prefix}_LOG_LEVEL")).ok(),
            receipts_dir: std::env::var(format!("{prefix}_RECEIPTS_DIR")).ok(),
            workspace_dir: std::env::var(format!("{prefix}_WORKSPACE_DIR")).ok(),
            bind_address: std::env::var(format!("{prefix}_BIND_ADDRESS")).ok(),
            port: std::env::var(format!("{prefix}_PORT"))
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }

    /// Returns `true` if no environment overrides were detected.
    pub fn is_empty(&self) -> bool {
        self.default_backend.is_none()
            && self.log_level.is_none()
            && self.receipts_dir.is_none()
            && self.workspace_dir.is_none()
            && self.bind_address.is_none()
            && self.port.is_none()
    }

    /// The number of fields that have overrides.
    pub fn len(&self) -> usize {
        [
            self.default_backend.is_some(),
            self.log_level.is_some(),
            self.receipts_dir.is_some(),
            self.workspace_dir.is_some(),
            self.bind_address.is_some(),
            self.port.is_some(),
        ]
        .iter()
        .filter(|&&b| b)
        .count()
    }

    /// Apply these overrides to a mutable config, replacing each field that
    /// has a value.
    pub fn apply(&self, config: &mut BackplaneConfig) {
        if let Some(ref v) = self.default_backend {
            config.default_backend = Some(v.clone());
        }
        if let Some(ref v) = self.log_level {
            config.log_level = Some(v.clone());
        }
        if let Some(ref v) = self.receipts_dir {
            config.receipts_dir = Some(v.clone());
        }
        if let Some(ref v) = self.workspace_dir {
            config.workspace_dir = Some(v.clone());
        }
        if let Some(ref v) = self.bind_address {
            config.bind_address = Some(v.clone());
        }
        if let Some(p) = self.port {
            config.port = Some(p);
        }
    }
}

/// Convenience: read `ABP_*` env vars and apply them to `config`.
///
/// Equivalent to `EnvOverrides::from_env().apply(config)`.
pub fn apply_env_overrides(config: &mut BackplaneConfig) {
    EnvOverrides::from_env().apply(config);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;

    #[test]
    fn empty_overrides() {
        let o = EnvOverrides::default();
        assert!(o.is_empty());
        assert_eq!(o.len(), 0);
    }

    #[test]
    fn manual_overrides_apply() {
        let o = EnvOverrides {
            default_backend: Some("mock".into()),
            log_level: Some("debug".into()),
            receipts_dir: Some("/r".into()),
            workspace_dir: Some("/w".into()),
            bind_address: Some("0.0.0.0".into()),
            port: Some(9090),
        };
        assert!(!o.is_empty());
        assert_eq!(o.len(), 6);

        let mut cfg = BackplaneConfig::default();
        o.apply(&mut cfg);
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
        assert_eq!(cfg.log_level.as_deref(), Some("debug"));
        assert_eq!(cfg.receipts_dir.as_deref(), Some("/r"));
        assert_eq!(cfg.workspace_dir.as_deref(), Some("/w"));
        assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
        assert_eq!(cfg.port, Some(9090));
    }

    #[test]
    fn partial_overrides_leave_others_unchanged() {
        let o = EnvOverrides {
            log_level: Some("warn".into()),
            ..Default::default()
        };
        assert_eq!(o.len(), 1);

        let mut cfg = BackplaneConfig {
            default_backend: Some("existing".into()),
            log_level: Some("info".into()),
            ..Default::default()
        };
        o.apply(&mut cfg);
        assert_eq!(cfg.default_backend.as_deref(), Some("existing"));
        assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    }

    #[test]
    fn from_env_with_prefix_reads_vars() {
        // SAFETY: test-only; no concurrent env access.
        unsafe {
            std::env::set_var("ENVTEST1_DEFAULT_BACKEND", "test-be");
            std::env::set_var("ENVTEST1_LOG_LEVEL", "trace");
            std::env::set_var("ENVTEST1_PORT", "4242");
        }
        let o = EnvOverrides::from_env_with_prefix("ENVTEST1");
        assert_eq!(o.default_backend.as_deref(), Some("test-be"));
        assert_eq!(o.log_level.as_deref(), Some("trace"));
        assert_eq!(o.port, Some(4242));
        unsafe {
            std::env::remove_var("ENVTEST1_DEFAULT_BACKEND");
            std::env::remove_var("ENVTEST1_LOG_LEVEL");
            std::env::remove_var("ENVTEST1_PORT");
        }
    }

    #[test]
    fn from_env_with_prefix_ignores_invalid_port() {
        unsafe {
            std::env::set_var("ENVTEST2_PORT", "not-a-number");
        }
        let o = EnvOverrides::from_env_with_prefix("ENVTEST2");
        assert_eq!(o.port, None);
        unsafe {
            std::env::remove_var("ENVTEST2_PORT");
        }
    }

    #[test]
    fn from_env_with_prefix_all_fields() {
        unsafe {
            std::env::set_var("ENVTEST3_DEFAULT_BACKEND", "be");
            std::env::set_var("ENVTEST3_LOG_LEVEL", "warn");
            std::env::set_var("ENVTEST3_RECEIPTS_DIR", "/rr");
            std::env::set_var("ENVTEST3_WORKSPACE_DIR", "/ww");
            std::env::set_var("ENVTEST3_BIND_ADDRESS", "::1");
            std::env::set_var("ENVTEST3_PORT", "7777");
        }
        let o = EnvOverrides::from_env_with_prefix("ENVTEST3");
        assert_eq!(o.len(), 6);

        let mut cfg = BackplaneConfig::default();
        o.apply(&mut cfg);
        assert_eq!(cfg.default_backend.as_deref(), Some("be"));
        assert_eq!(cfg.log_level.as_deref(), Some("warn"));
        assert_eq!(cfg.receipts_dir.as_deref(), Some("/rr"));
        assert_eq!(cfg.workspace_dir.as_deref(), Some("/ww"));
        assert_eq!(cfg.bind_address.as_deref(), Some("::1"));
        assert_eq!(cfg.port, Some(7777));
        unsafe {
            std::env::remove_var("ENVTEST3_DEFAULT_BACKEND");
            std::env::remove_var("ENVTEST3_LOG_LEVEL");
            std::env::remove_var("ENVTEST3_RECEIPTS_DIR");
            std::env::remove_var("ENVTEST3_WORKSPACE_DIR");
            std::env::remove_var("ENVTEST3_BIND_ADDRESS");
            std::env::remove_var("ENVTEST3_PORT");
        }
    }

    #[test]
    fn overrides_debug_impl() {
        let o = EnvOverrides::default();
        let s = format!("{o:?}");
        assert!(s.contains("EnvOverrides"));
    }

    #[test]
    fn overrides_clone_eq() {
        let a = EnvOverrides {
            log_level: Some("info".into()),
            ..Default::default()
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
