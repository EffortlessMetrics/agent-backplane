// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validate subcommand implementation.
//!
//! Provides config file validation with error/warning reporting.

#![allow(dead_code, unused_imports)]

use anyhow::{Context, Result};
use std::path::Path;

/// Result of validating a configuration file.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// True when the configuration is valid (no errors).
    pub valid: bool,
    /// Hard errors that prevent the config from being used.
    pub errors: Vec<String>,
    /// Advisory warnings that do not prevent operation.
    pub warnings: Vec<String>,
}

/// Validate a backplane configuration file.
///
/// If `path` is `None`, attempts to validate `backplane.toml` in the current
/// directory, falling back to the built-in defaults.
pub fn validate_config(path: Option<&Path>) -> Result<ValidationResult> {
    let effective_path = path.map(|p| p.to_path_buf()).or_else(|| {
        let p = std::path::PathBuf::from("backplane.toml");
        if p.exists() { Some(p) } else { None }
    });

    let config = match abp_config::load_config(effective_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            return Ok(ValidationResult {
                valid: false,
                errors: vec![format!("{e}")],
                warnings: vec![],
            });
        }
    };

    match abp_config::validate_config(&config) {
        Ok(warnings) => {
            let warning_strs: Vec<String> = warnings.iter().map(|w| format!("{w}")).collect();
            Ok(ValidationResult {
                valid: true,
                errors: vec![],
                warnings: warning_strs,
            })
        }
        Err(config_err) => {
            let errors = match config_err {
                abp_config::ConfigError::ValidationError { reasons } => reasons,
                other => vec![format!("{other}")],
            };
            Ok(ValidationResult {
                valid: false,
                errors,
                warnings: vec![],
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_defaults_is_valid() {
        let result = validate_config(None).unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn validate_bad_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "not valid [toml =").unwrap();
        let result = validate_config(Some(&path)).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn validate_valid_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("good.toml");
        std::fs::write(
            &path,
            r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"
"#,
        )
        .unwrap();
        let result = validate_config(Some(&path)).unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn validate_invalid_backend_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.toml");
        std::fs::write(
            &path,
            r#"
[backends.bad]
type = "sidecar"
command = "  "
"#,
        )
        .unwrap();
        let result = validate_config(Some(&path)).unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("command")));
    }

    #[test]
    fn validate_missing_file_returns_error() {
        let result = validate_config(Some(Path::new("/nonexistent/path.toml"))).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn validate_reports_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("warn.toml");
        // Config without default_backend or receipts_dir triggers advisory warnings.
        std::fs::write(
            &path,
            r#"
log_level = "info"
"#,
        )
        .unwrap();
        let result = validate_config(Some(&path)).unwrap();
        assert!(result.valid);
        // Should have warnings about missing optional fields.
        assert!(!result.warnings.is_empty());
    }
}
