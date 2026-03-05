// SPDX-License-Identifier: MIT OR Apache-2.0
//! Focused configuration validation for hot-reload scenarios.
//!
//! Provides [`ValidationResult`] and [`validate_config`] — a self-contained
//! validator that returns structured warnings and errors without short-circuiting,
//! suitable for use with [`crate::store::ConfigStore::update`].

#![allow(dead_code, unused_imports)]

use crate::{BackendEntry, BackplaneConfig, VALID_LOG_LEVELS};
use std::fmt;

// ---------------------------------------------------------------------------
// ValidationResult
// ---------------------------------------------------------------------------

/// Outcome of validating a [`BackplaneConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// Hard errors — the config must not be applied while any exist.
    pub errors: Vec<String>,
    /// Soft warnings — config can still be applied.
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// `true` when there are zero errors.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Total number of findings (errors + warnings).
    pub fn len(&self) -> usize {
        self.errors.len() + self.warnings.len()
    }

    /// `true` when there are no findings at all.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }
}

impl fmt::Display for ValidationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "config valid (no issues)");
        }
        for e in &self.errors {
            writeln!(f, "ERROR: {e}")?;
        }
        for w in &self.warnings {
            writeln!(f, "WARN:  {w}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// validate_config
// ---------------------------------------------------------------------------

/// Validate a [`BackplaneConfig`], returning a [`ValidationResult`].
///
/// Unlike [`crate::validate_config`] this function never returns `Err` —
/// all findings are collected into the result struct.
pub fn validate_config(config: &BackplaneConfig) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // --- backend references ---
    if let Some(ref name) = config.default_backend {
        if !config.backends.is_empty() && !config.backends.contains_key(name) {
            warnings.push(format!(
                "default_backend '{name}' does not match any configured backend"
            ));
        }
    }

    // --- policy profiles ---
    for (i, path) in config.policy_profiles.iter().enumerate() {
        if path.trim().is_empty() {
            errors.push(format!("policy_profiles[{i}] is empty"));
        }
    }

    // --- model names (log_level as proxy for model validation) ---
    if let Some(ref level) = config.log_level {
        if !VALID_LOG_LEVELS.contains(&level.as_str()) {
            errors.push(format!("invalid log_level '{level}'"));
        }
    }

    // --- backend entries ---
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
                    if *t == 0 || *t > crate::MAX_TIMEOUT_SECS {
                        errors.push(format!(
                            "backend '{name}': timeout {t}s out of range (1..{})",
                            crate::MAX_TIMEOUT_SECS
                        ));
                    } else if *t > crate::LARGE_TIMEOUT_THRESHOLD {
                        warnings.push(format!("backend '{name}': large timeout ({t}s)"));
                    }
                }
            }
            BackendEntry::Mock {} => {}
        }
    }

    // --- port ---
    if let Some(p) = config.port {
        if p == 0 {
            errors.push("port must be between 1 and 65535".into());
        }
    }

    ValidationResult { errors, warnings }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn good_config() -> BackplaneConfig {
        BackplaneConfig {
            default_backend: Some("mock".into()),
            workspace_dir: None,
            log_level: Some("info".into()),
            receipts_dir: None,
            bind_address: None,
            port: None,
            policy_profiles: Vec::new(),
            backends: BTreeMap::from([("mock".into(), BackendEntry::Mock {})]),
        }
    }

    #[test]
    fn valid_config_passes() {
        let r = validate_config(&good_config());
        assert!(r.is_valid());
    }

    #[test]
    fn invalid_log_level_is_error() {
        let mut cfg = good_config();
        cfg.log_level = Some("LOUD".into());
        let r = validate_config(&cfg);
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("log_level")));
    }

    #[test]
    fn empty_sidecar_command_is_error() {
        let mut cfg = good_config();
        cfg.backends.insert(
            "bad".into(),
            BackendEntry::Sidecar {
                command: "".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let r = validate_config(&cfg);
        assert!(!r.is_valid());
        assert!(r.errors.iter().any(|e| e.contains("command")));
    }

    #[test]
    fn empty_policy_profile_is_error() {
        let mut cfg = good_config();
        cfg.policy_profiles = vec!["  ".into()];
        let r = validate_config(&cfg);
        assert!(!r.is_valid());
    }

    #[test]
    fn unresolved_default_backend_is_warning() {
        let mut cfg = good_config();
        cfg.default_backend = Some("nonexistent".into());
        let r = validate_config(&cfg);
        assert!(r.is_valid()); // warning, not error
        assert!(r.warnings.iter().any(|w| w.contains("nonexistent")));
    }

    #[test]
    fn zero_port_is_error() {
        let mut cfg = good_config();
        cfg.port = Some(0);
        let r = validate_config(&cfg);
        assert!(!r.is_valid());
    }

    #[test]
    fn display_format_works() {
        let mut cfg = good_config();
        cfg.log_level = Some("BAD".into());
        let r = validate_config(&cfg);
        let text = r.to_string();
        assert!(text.contains("ERROR"));
    }

    #[test]
    fn clean_config_display() {
        let r = validate_config(&good_config());
        if r.is_empty() {
            assert!(r.to_string().contains("no issues"));
        }
    }
}
