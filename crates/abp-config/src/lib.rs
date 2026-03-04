// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! Configuration loading, validation, and merging for the Agent Backplane.
//!
//! This crate provides [`BackplaneConfig`] — the top-level runtime settings —
//! together with helpers for loading from TOML files, merging overlays, and
//! producing advisory [`ConfigWarning`]s.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod defaults;
pub mod diff;
pub mod env;
pub mod hot_validate;
pub mod migrate;
pub mod schema;
pub mod store;
pub mod validate;
pub mod watcher;

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

    /// Network bind address (e.g. `"127.0.0.1"`, `"0.0.0.0"`, `"::1"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,

    /// Network port number (1–65 535).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Paths to policy profile files that should be loaded at startup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_profiles: Vec<String>,

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
            bind_address: None,
            port: None,
            policy_profiles: Vec::new(),
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
pub const MAX_TIMEOUT_SECS: u64 = 86_400;

/// Threshold above which a timeout generates a warning.
pub const LARGE_TIMEOUT_THRESHOLD: u64 = 3_600;

/// Recognised log levels.
pub const VALID_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

/// Default environment variable prefix for overrides.
pub const DEFAULT_ENV_PREFIX: &str = "ABP";

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

/// Load a [`BackplaneConfig`] from a TOML file at `path`.
///
/// This is a convenience wrapper around [`load_config`] that always requires
/// a path.  Environment variable overrides are **not** applied automatically;
/// call [`apply_env_overrides`] afterwards if needed.
pub fn load_from_file(path: &Path) -> Result<BackplaneConfig, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|_| ConfigError::FileNotFound {
        path: path.display().to_string(),
    })?;
    load_from_str(&content)
}

/// Parse a TOML string into a [`BackplaneConfig`].
///
/// This is a convenience alias for [`parse_toml`] with a more discoverable
/// name.  Environment variable overrides are **not** applied automatically.
pub fn load_from_str(toml_str: &str) -> Result<BackplaneConfig, ConfigError> {
    parse_toml(toml_str)
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
/// - `ABP_BIND_ADDRESS`
/// - `ABP_PORT`
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
    if let Ok(val) = std::env::var("ABP_BIND_ADDRESS") {
        config.bind_address = Some(val);
    }
    if let Ok(val) = std::env::var("ABP_PORT") {
        if let Ok(p) = val.parse::<u16>() {
            config.port = Some(p);
        }
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

    // Validate port (u16 already guarantees <= 65535, but 0 is invalid).
    if let Some(p) = config.port {
        if p == 0 {
            errors.push("port must be between 1 and 65535".into());
        }
    }

    // Validate bind_address (must parse as an IP address or be a non-empty
    // hostname-like string).
    if let Some(ref addr) = config.bind_address {
        if addr.trim().is_empty() {
            errors.push("bind_address must not be empty".into());
        } else if addr.parse::<std::net::IpAddr>().is_err() && !is_valid_hostname(addr) {
            errors.push(format!(
                "bind_address '{addr}' is not a valid IP address or hostname"
            ));
        }
    }

    // Validate policy profile paths exist on disk (when specified).
    for path_str in &config.policy_profiles {
        if path_str.trim().is_empty() {
            errors.push("policy profile path must not be empty".into());
        } else if !Path::new(path_str).exists() {
            errors.push(format!("policy profile path does not exist: {path_str}"));
        }
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
    let policy_profiles = if overlay.policy_profiles.is_empty() {
        base.policy_profiles
    } else {
        overlay.policy_profiles
    };
    BackplaneConfig {
        default_backend: overlay.default_backend.or(base.default_backend),
        workspace_dir: overlay.workspace_dir.or(base.workspace_dir),
        log_level: overlay.log_level.or(base.log_level),
        receipts_dir: overlay.receipts_dir.or(base.receipts_dir),
        bind_address: overlay.bind_address.or(base.bind_address),
        port: overlay.port.or(base.port),
        policy_profiles,
        backends,
    }
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Serialize a [`BackplaneConfig`] to a TOML string.
pub fn to_toml(config: &BackplaneConfig) -> Result<String, ConfigError> {
    toml::to_string(config).map_err(|e| ConfigError::ParseError {
        reason: e.to_string(),
    })
}

/// Serialize a [`BackplaneConfig`] to a pretty-printed TOML string.
pub fn to_toml_pretty(config: &BackplaneConfig) -> Result<String, ConfigError> {
    toml::to_string_pretty(config).map_err(|e| ConfigError::ParseError {
        reason: e.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Default generation
// ---------------------------------------------------------------------------

/// Generate a commented default TOML configuration string.
///
/// This is useful for `--init` style CLI commands that write a starter
/// configuration file. Each field includes a comment explaining its purpose.
pub fn generate_default_toml() -> String {
    let lines = vec![
        "# Agent Backplane configuration file",
        "# See https://github.com/anthropics/agent-backplane for details",
        "",
        "# Default backend when none is specified on the command line.",
        "# default_backend = \"mock\"",
        "",
        "# Working directory used for staged workspaces.",
        "# workspace_dir = \"/tmp/abp-workspaces\"",
        "",
        "# Log level: error, warn, info, debug, trace",
        "log_level = \"info\"",
        "",
        "# Directory for persisting receipt JSON files.",
        "# receipts_dir = \"/tmp/abp-receipts\"",
        "",
        "# Network bind address (e.g. \"127.0.0.1\", \"0.0.0.0\", \"::1\").",
        "# bind_address = \"127.0.0.1\"",
        "",
        "# Network port number (1-65535).",
        "# port = 8080",
        "",
        "# Paths to policy profile files loaded at startup.",
        "# policy_profiles = [\"policies/default.toml\"]",
        "",
        "# Backend definitions:",
        "# [backends.mock]",
        "# type = \"mock\"",
        "#",
        "# [backends.node]",
        "# type = \"sidecar\"",
        "# command = \"node\"",
        "# args = [\"hosts/node/index.js\"]",
        "# timeout_secs = 300",
    ];
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Multi-merge
// ---------------------------------------------------------------------------

/// Merge a series of configs in order, left to right.
///
/// The first element is treated as the base. Each subsequent element is
/// overlaid on the accumulated result. An empty slice returns the default
/// config.
pub fn merge_many(configs: &[BackplaneConfig]) -> BackplaneConfig {
    configs
        .iter()
        .cloned()
        .reduce(merge_configs)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Combined load + validate
// ---------------------------------------------------------------------------

/// Load, validate, and return a config together with any advisory warnings.
///
/// This is the recommended single entry point for production use: it loads
/// from an optional path, applies environment overrides, and validates in
/// one call.
pub fn load_and_validate(
    path: Option<&Path>,
) -> Result<(BackplaneConfig, Vec<ConfigWarning>), ConfigError> {
    let config = load_config(path)?;
    let warnings = validate_config(&config)?;
    Ok((config, warnings))
}

// ---------------------------------------------------------------------------
// Custom-prefix env overrides
// ---------------------------------------------------------------------------

/// Apply environment variable overrides with a custom prefix.
///
/// For example, with prefix `"MY_APP"`, the function reads
/// `MY_APP_DEFAULT_BACKEND`, `MY_APP_LOG_LEVEL`, etc.
pub fn apply_env_overrides_with_prefix(config: &mut BackplaneConfig, prefix: &str) {
    if let Ok(val) = std::env::var(format!("{prefix}_DEFAULT_BACKEND")) {
        config.default_backend = Some(val);
    }
    if let Ok(val) = std::env::var(format!("{prefix}_LOG_LEVEL")) {
        config.log_level = Some(val);
    }
    if let Ok(val) = std::env::var(format!("{prefix}_RECEIPTS_DIR")) {
        config.receipts_dir = Some(val);
    }
    if let Ok(val) = std::env::var(format!("{prefix}_WORKSPACE_DIR")) {
        config.workspace_dir = Some(val);
    }
    if let Ok(val) = std::env::var(format!("{prefix}_BIND_ADDRESS")) {
        config.bind_address = Some(val);
    }
    if let Ok(val) = std::env::var(format!("{prefix}_PORT")) {
        if let Ok(p) = val.parse::<u16>() {
            config.port = Some(p);
        }
    }
}

// ---------------------------------------------------------------------------
// BackplaneConfig methods
// ---------------------------------------------------------------------------

impl BackplaneConfig {
    /// Returns `true` if the config is entirely at default values (no
    /// backends, no overrides beyond `log_level = "info"`).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Serialize this config to a TOML string.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        to_toml(self)
    }

    /// Serialize this config to a pretty-printed TOML string.
    pub fn to_toml_pretty(&self) -> Result<String, ConfigError> {
        to_toml_pretty(self)
    }

    /// Overlay another config on top of this one, returning the merged result.
    pub fn merge(self, overlay: BackplaneConfig) -> BackplaneConfig {
        merge_configs(self, overlay)
    }

    /// Validate this config, returning warnings on success.
    pub fn validate(&self) -> Result<Vec<ConfigWarning>, ConfigError> {
        validate_config(self)
    }

    /// Create a builder for programmatic construction.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// ConfigBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for [`BackplaneConfig`].
///
/// All fields start at their default values. Call `.build()` to get the
/// finished config, or `.build_and_validate()` to also run validation.
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    config: BackplaneConfig,
}

impl ConfigBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        Self {
            config: BackplaneConfig::default(),
        }
    }

    /// Set the default backend name.
    pub fn default_backend(mut self, name: impl Into<String>) -> Self {
        self.config.default_backend = Some(name.into());
        self
    }

    /// Set the workspace directory.
    pub fn workspace_dir(mut self, dir: impl Into<String>) -> Self {
        self.config.workspace_dir = Some(dir.into());
        self
    }

    /// Set the log level.
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.log_level = Some(level.into());
        self
    }

    /// Set the receipts directory.
    pub fn receipts_dir(mut self, dir: impl Into<String>) -> Self {
        self.config.receipts_dir = Some(dir.into());
        self
    }

    /// Set the bind address.
    pub fn bind_address(mut self, addr: impl Into<String>) -> Self {
        self.config.bind_address = Some(addr.into());
        self
    }

    /// Set the port.
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = Some(port);
        self
    }

    /// Add a policy profile path.
    pub fn policy_profile(mut self, path: impl Into<String>) -> Self {
        self.config.policy_profiles.push(path.into());
        self
    }

    /// Add a backend entry.
    pub fn backend(mut self, name: impl Into<String>, entry: BackendEntry) -> Self {
        self.config.backends.insert(name.into(), entry);
        self
    }

    /// Consume the builder and return the config without validation.
    pub fn build(self) -> BackplaneConfig {
        self.config
    }

    /// Consume the builder, validate, and return config with warnings.
    pub fn build_and_validate(self) -> Result<(BackplaneConfig, Vec<ConfigWarning>), ConfigError> {
        let warnings = validate_config(&self.config)?;
        Ok((self.config, warnings))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check whether `s` looks like a valid hostname (RFC 952 / RFC 1123).
pub(crate) fn is_valid_hostname(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    // Allow `localhost` and dotted labels like `my-host.example.com`.
    s.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
#[allow(unsafe_code)]
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
            ..Default::default()
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
            ..Default::default()
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

    // -- 26. to_toml produces valid TOML that roundtrips ----------------------

    #[test]
    fn to_toml_roundtrip() {
        let cfg = BackplaneConfig::builder()
            .default_backend("mock")
            .log_level("debug")
            .receipts_dir("/r")
            .workspace_dir("/ws")
            .bind_address("127.0.0.1")
            .port(8080)
            .backend("m", BackendEntry::Mock {})
            .build();
        let toml_str = to_toml(&cfg).unwrap();
        let parsed = parse_toml(&toml_str).unwrap();
        assert_eq!(cfg, parsed);
    }

    // -- 27. to_toml_pretty produces parseable output -------------------------

    #[test]
    fn to_toml_pretty_roundtrips() {
        let cfg = BackplaneConfig::builder()
            .default_backend("sc")
            .backend(
                "sc",
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(120),
                },
            )
            .build();
        let pretty = to_toml_pretty(&cfg).unwrap();
        let parsed = parse_toml(&pretty).unwrap();
        assert_eq!(cfg, parsed);
    }

    // -- 28. generate_default_toml contains commented fields ------------------

    #[test]
    fn generate_default_toml_contains_expected_content() {
        let output = generate_default_toml();
        assert!(output.contains("log_level = \"info\""));
        assert!(output.contains("# default_backend"));
        assert!(output.contains("# workspace_dir"));
        assert!(output.contains("# receipts_dir"));
        assert!(output.contains("# bind_address"));
        assert!(output.contains("# port"));
        assert!(output.contains("# policy_profiles"));
        assert!(output.contains("[backends.mock]"));
        assert!(output.contains("[backends.node]"));
    }

    // -- 29. generate_default_toml uncommented lines parse --------------------

    #[test]
    fn generate_default_toml_active_lines_parse() {
        let output = generate_default_toml();
        // Extract only non-comment, non-empty lines
        let active: String = output
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let cfg = parse_toml(&active).unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
    }

    // -- 30. merge_many with empty slice returns default ----------------------

    #[test]
    fn merge_many_empty_returns_default() {
        let result = merge_many(&[]);
        assert!(result.is_empty());
    }

    // -- 31. merge_many single element returns that element -------------------

    #[test]
    fn merge_many_single_element() {
        let cfg = BackplaneConfig::builder().default_backend("mock").build();
        let result = merge_many(std::slice::from_ref(&cfg));
        assert_eq!(result.default_backend.as_deref(), Some("mock"));
    }

    // -- 32. merge_many chains correctly --------------------------------------

    #[test]
    fn merge_many_chains_three_configs() {
        let base = BackplaneConfig::builder()
            .default_backend("a")
            .log_level("error")
            .workspace_dir("/base")
            .build();
        let mid = BackplaneConfig {
            default_backend: Some("b".into()),
            log_level: None,
            ..Default::default()
        };
        let top = BackplaneConfig {
            port: Some(9090),
            log_level: None,
            ..Default::default()
        };
        let result = merge_many(&[base, mid, top]);
        assert_eq!(result.default_backend.as_deref(), Some("b"));
        assert_eq!(result.workspace_dir.as_deref(), Some("/base"));
        assert_eq!(result.port, Some(9090));
    }

    // -- 33. BackplaneConfig::is_empty on default ----------------------------

    #[test]
    fn config_is_empty_on_default() {
        assert!(BackplaneConfig::default().is_empty());
    }

    // -- 34. BackplaneConfig::is_empty false when modified --------------------

    #[test]
    fn config_is_not_empty_when_modified() {
        let cfg = BackplaneConfig::builder().default_backend("mock").build();
        assert!(!cfg.is_empty());
    }

    // -- 35. BackplaneConfig::merge method ------------------------------------

    #[test]
    fn config_merge_method() {
        let base = BackplaneConfig::builder().log_level("info").build();
        let overlay = BackplaneConfig::builder()
            .default_backend("mock")
            .log_level("debug")
            .build();
        let result = base.merge(overlay);
        assert_eq!(result.default_backend.as_deref(), Some("mock"));
        assert_eq!(result.log_level.as_deref(), Some("debug"));
    }

    // -- 36. BackplaneConfig::validate method ---------------------------------

    #[test]
    fn config_validate_method() {
        let cfg = BackplaneConfig::default();
        let warnings = cfg.validate().expect("default should be valid");
        assert!(!warnings.is_empty());
    }

    // -- 37. BackplaneConfig::to_toml method ----------------------------------

    #[test]
    fn config_to_toml_method() {
        let cfg = BackplaneConfig::builder().log_level("warn").build();
        let s = cfg.to_toml().unwrap();
        assert!(s.contains("warn"));
    }

    // -- 38. BackplaneConfig::to_toml_pretty method ---------------------------

    #[test]
    fn config_to_toml_pretty_method() {
        let cfg = BackplaneConfig::builder()
            .log_level("debug")
            .backend("m", BackendEntry::Mock {})
            .build();
        let s = cfg.to_toml_pretty().unwrap();
        assert!(s.contains("debug"));
        assert!(s.contains("[backends.m]"));
    }

    // -- 39. Builder sets all fields -----------------------------------------

    #[test]
    fn builder_sets_all_fields() {
        let cfg = BackplaneConfig::builder()
            .default_backend("mock")
            .workspace_dir("/ws")
            .log_level("trace")
            .receipts_dir("/r")
            .bind_address("0.0.0.0")
            .port(3000)
            .policy_profile("pol.toml")
            .backend("m", BackendEntry::Mock {})
            .build();
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
        assert_eq!(cfg.workspace_dir.as_deref(), Some("/ws"));
        assert_eq!(cfg.log_level.as_deref(), Some("trace"));
        assert_eq!(cfg.receipts_dir.as_deref(), Some("/r"));
        assert_eq!(cfg.bind_address.as_deref(), Some("0.0.0.0"));
        assert_eq!(cfg.port, Some(3000));
        assert_eq!(cfg.policy_profiles, vec!["pol.toml"]);
        assert!(cfg.backends.contains_key("m"));
    }

    // -- 40. Builder build_and_validate catches errors ------------------------

    #[test]
    fn builder_build_and_validate_catches_error() {
        let result = BackplaneConfig::builder()
            .log_level("bogus")
            .build_and_validate();
        assert!(result.is_err());
    }

    // -- 41. Builder build_and_validate returns warnings ----------------------

    #[test]
    fn builder_build_and_validate_returns_warnings() {
        let (cfg, warnings) = BackplaneConfig::builder()
            .log_level("info")
            .build_and_validate()
            .unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
        assert!(!warnings.is_empty());
    }

    // -- 42. Builder multiple policy profiles ---------------------------------

    #[test]
    fn builder_multiple_policy_profiles() {
        let cfg = BackplaneConfig::builder()
            .policy_profile("a.toml")
            .policy_profile("b.toml")
            .build();
        assert_eq!(cfg.policy_profiles.len(), 2);
    }

    // -- 43. Builder multiple backends ----------------------------------------

    #[test]
    fn builder_multiple_backends() {
        let cfg = BackplaneConfig::builder()
            .backend("mock", BackendEntry::Mock {})
            .backend(
                "sc",
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec![],
                    timeout_secs: None,
                },
            )
            .build();
        assert_eq!(cfg.backends.len(), 2);
    }

    // -- 44. load_and_validate with file on disk -----------------------------

    #[test]
    fn load_and_validate_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "default_backend = \"mock\"\nlog_level = \"info\"").unwrap();
        writeln!(f, "receipts_dir = \"/tmp\"").unwrap();
        let (cfg, warnings) = load_and_validate(Some(&path)).unwrap();
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
        // With default_backend and receipts_dir set, no missing-optional warnings for those
        assert!(
            !warnings
                .iter()
                .any(|w| matches!(w, ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"))
        );
    }

    // -- 45. load_and_validate with None returns default ----------------------

    #[test]
    fn load_and_validate_none_returns_default() {
        let (cfg, warnings) = load_and_validate(None).unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
        assert!(!warnings.is_empty());
    }

    // -- 46. load_and_validate catches validation errors ----------------------

    #[test]
    fn load_and_validate_catches_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "log_level = \"oops\"").unwrap();
        let err = load_and_validate(Some(&path)).unwrap_err();
        assert!(matches!(err, ConfigError::ValidationError { .. }));
    }

    // -- 47. apply_env_overrides_with_prefix works ----------------------------

    #[test]
    fn env_overrides_with_custom_prefix() {
        // Use a unique prefix to avoid collisions
        // SAFETY: Test-only; no concurrent env access in this test.
        unsafe {
            std::env::set_var("T47_DEFAULT_BACKEND", "custom-be");
            std::env::set_var("T47_LOG_LEVEL", "trace");
            std::env::set_var("T47_PORT", "4321");
        }
        let mut cfg = BackplaneConfig::default();
        apply_env_overrides_with_prefix(&mut cfg, "T47");
        assert_eq!(cfg.default_backend.as_deref(), Some("custom-be"));
        assert_eq!(cfg.log_level.as_deref(), Some("trace"));
        assert_eq!(cfg.port, Some(4321));
        // Clean up
        unsafe {
            std::env::remove_var("T47_DEFAULT_BACKEND");
            std::env::remove_var("T47_LOG_LEVEL");
            std::env::remove_var("T47_PORT");
        }
    }

    // -- 48. apply_env_overrides_with_prefix ignores invalid port -------------

    #[test]
    fn env_overrides_custom_prefix_invalid_port() {
        // SAFETY: Test-only; no concurrent env access in this test.
        unsafe {
            std::env::set_var("T48_PORT", "not-a-number");
        }
        let mut cfg = BackplaneConfig::default();
        apply_env_overrides_with_prefix(&mut cfg, "T48");
        assert_eq!(cfg.port, None);
        unsafe {
            std::env::remove_var("T48_PORT");
        }
    }

    // -- 49. apply_env_overrides_with_prefix sets all fields ------------------

    #[test]
    fn env_overrides_custom_prefix_all_fields() {
        // SAFETY: Test-only; no concurrent env access in this test.
        unsafe {
            std::env::set_var("T49_DEFAULT_BACKEND", "be");
            std::env::set_var("T49_LOG_LEVEL", "warn");
            std::env::set_var("T49_RECEIPTS_DIR", "/rr");
            std::env::set_var("T49_WORKSPACE_DIR", "/ww");
            std::env::set_var("T49_BIND_ADDRESS", "::1");
            std::env::set_var("T49_PORT", "9999");
        }
        let mut cfg = BackplaneConfig::default();
        apply_env_overrides_with_prefix(&mut cfg, "T49");
        assert_eq!(cfg.default_backend.as_deref(), Some("be"));
        assert_eq!(cfg.log_level.as_deref(), Some("warn"));
        assert_eq!(cfg.receipts_dir.as_deref(), Some("/rr"));
        assert_eq!(cfg.workspace_dir.as_deref(), Some("/ww"));
        assert_eq!(cfg.bind_address.as_deref(), Some("::1"));
        assert_eq!(cfg.port, Some(9999));
        unsafe {
            std::env::remove_var("T49_DEFAULT_BACKEND");
            std::env::remove_var("T49_LOG_LEVEL");
            std::env::remove_var("T49_RECEIPTS_DIR");
            std::env::remove_var("T49_WORKSPACE_DIR");
            std::env::remove_var("T49_BIND_ADDRESS");
            std::env::remove_var("T49_PORT");
        }
    }

    // -- 50. TOML roundtrip with all field types -----------------------------

    #[test]
    fn full_toml_roundtrip_all_fields() {
        let cfg = BackplaneConfig::builder()
            .default_backend("sc")
            .workspace_dir("/ws")
            .log_level("debug")
            .receipts_dir("/receipts")
            .bind_address("0.0.0.0")
            .port(8080)
            .policy_profile("policies/default.toml")
            .backend("m", BackendEntry::Mock {})
            .backend(
                "sc",
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["--experimental".into(), "host.js".into()],
                    timeout_secs: Some(600),
                },
            )
            .build();
        // Roundtrip via TOML string
        let toml_str = cfg.to_toml_pretty().unwrap();
        let parsed = parse_toml(&toml_str).unwrap();
        assert_eq!(cfg, parsed);

        // Roundtrip via JSON
        let json = serde_json::to_string(&cfg).unwrap();
        let from_json: BackplaneConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, from_json);
    }

    // -- 51. Config diff detects all field changes ----------------------------

    #[test]
    fn config_diff_detects_all_changes() {
        use crate::validate::diff_configs;

        let a = BackplaneConfig::default();
        let b = BackplaneConfig::builder()
            .default_backend("mock")
            .workspace_dir("/ws")
            .log_level("debug")
            .receipts_dir("/r")
            .bind_address("0.0.0.0")
            .port(9090)
            .backend("m", BackendEntry::Mock {})
            .build();
        let diffs = diff_configs(&a, &b);
        let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"default_backend"));
        assert!(paths.contains(&"workspace_dir"));
        assert!(paths.contains(&"log_level"));
        assert!(paths.contains(&"receipts_dir"));
        assert!(paths.contains(&"bind_address"));
        assert!(paths.contains(&"port"));
        assert!(paths.contains(&"backends.m"));
    }

    // -- 52. Config diff empty when identical ---------------------------------

    #[test]
    fn config_diff_empty_when_identical() {
        use crate::validate::diff_configs;

        let cfg = BackplaneConfig::builder().default_backend("mock").build();
        let diffs = diff_configs(&cfg, &cfg);
        assert!(diffs.is_empty());
    }

    // -- 53. ConfigDiff Display trait -----------------------------------------

    #[test]
    fn config_diff_display() {
        use crate::validate::ConfigDiff;

        let d = ConfigDiff {
            path: "log_level".into(),
            old_value: "\"info\"".into(),
            new_value: "\"debug\"".into(),
        };
        let s = d.to_string();
        assert!(s.contains("log_level"));
        assert!(s.contains("info"));
        assert!(s.contains("debug"));
    }

    // -- 54. Constants are publicly accessible --------------------------------

    #[test]
    fn constants_accessible() {
        assert_eq!(MAX_TIMEOUT_SECS, 86_400);
        assert_eq!(LARGE_TIMEOUT_THRESHOLD, 3_600);
        assert!(VALID_LOG_LEVELS.contains(&"info"));
        assert_eq!(DEFAULT_ENV_PREFIX, "ABP");
    }

    // -- 55. Merge many accumulates backends ----------------------------------

    #[test]
    fn merge_many_accumulates_backends() {
        let a = BackplaneConfig::builder()
            .backend("a", BackendEntry::Mock {})
            .build();
        let b = BackplaneConfig::builder()
            .backend("b", BackendEntry::Mock {})
            .build();
        let c = BackplaneConfig::builder()
            .backend("c", BackendEntry::Mock {})
            .build();
        let result = merge_many(&[a, b, c]);
        assert_eq!(result.backends.len(), 3);
    }

    // -- 56. Validation catches port zero -------------------------------------

    #[test]
    fn validation_catches_port_zero() {
        let cfg = BackplaneConfig::builder().port(0).build();
        let err = validate_config(&cfg).unwrap_err();
        match err {
            ConfigError::ValidationError { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("port")));
            }
            other => panic!("expected ValidationError, got {other:?}"),
        }
    }

    // -- 57. Validation catches empty bind_address ----------------------------

    #[test]
    fn validation_catches_empty_bind_address() {
        let cfg = BackplaneConfig::builder().bind_address("").build();
        let err = validate_config(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::ValidationError { .. }));
    }

    // -- 58. Validation catches invalid bind_address --------------------------

    #[test]
    fn validation_catches_invalid_bind_address() {
        let cfg = BackplaneConfig::builder()
            .bind_address("not valid!")
            .build();
        let err = validate_config(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::ValidationError { .. }));
    }

    // -- 59. Validation accepts IPv6 bind_address -----------------------------

    #[test]
    fn validation_accepts_ipv6_bind_address() {
        let cfg = BackplaneConfig::builder().bind_address("::1").build();
        validate_config(&cfg).expect("IPv6 should be valid");
    }

    // -- 60. Validation accepts hostname bind_address -------------------------

    #[test]
    fn validation_accepts_hostname_bind_address() {
        let cfg = BackplaneConfig::builder().bind_address("localhost").build();
        validate_config(&cfg).expect("localhost should be valid");
    }

    // -- 61. Merge preserves policy profiles from base when overlay empty -----

    #[test]
    fn merge_preserves_policy_profiles() {
        let base = BackplaneConfig::builder()
            .policy_profile("base.toml")
            .build();
        let overlay = BackplaneConfig::default();
        let merged = merge_configs(base, overlay);
        assert_eq!(merged.policy_profiles, vec!["base.toml"]);
    }

    // -- 62. Merge replaces policy profiles when overlay non-empty ------------

    #[test]
    fn merge_replaces_policy_profiles() {
        let base = BackplaneConfig::builder()
            .policy_profile("base.toml")
            .build();
        let overlay = BackplaneConfig::builder()
            .policy_profile("overlay.toml")
            .build();
        let merged = merge_configs(base, overlay);
        assert_eq!(merged.policy_profiles, vec!["overlay.toml"]);
    }

    // -- 63. ConfigBuilder default is equivalent to BackplaneConfig::default --

    #[test]
    fn builder_default_matches_config_default() {
        let from_builder = ConfigBuilder::default().build();
        let direct = BackplaneConfig::default();
        assert_eq!(from_builder, direct);
    }

    // -- 64. load_from_str delegates to parse_toml ----------------------------

    #[test]
    fn load_from_str_works() {
        let cfg = load_from_str("log_level = \"warn\"").unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    }

    // -- 65. load_from_file reads from disk -----------------------------------

    #[test]
    fn load_from_file_reads_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "default_backend = \"x\"").unwrap();
        let cfg = load_from_file(&path).unwrap();
        assert_eq!(cfg.default_backend.as_deref(), Some("x"));
    }

    // -- 66. load_from_file missing file errors -------------------------------

    #[test]
    fn load_from_file_missing() {
        let err = load_from_file(Path::new("/no/such/file.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::FileNotFound { .. }));
    }

    // -- 67. All valid log levels pass validation -----------------------------

    #[test]
    fn all_valid_log_levels_pass() {
        for level in VALID_LOG_LEVELS {
            let cfg = BackplaneConfig::builder().log_level(*level).build();
            validate_config(&cfg).unwrap_or_else(|_| panic!("level '{level}' should be valid"));
        }
    }
}
