// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sensible defaults for [`BackplaneConfig`] fields.
//!
//! [`ConfigDefaults`] documents the canonical default for every field.
//! [`with_defaults`] takes a partial JSON [`Value`] and fills in any
//! missing fields from those defaults before deserializing into a full
//! [`BackplaneConfig`].

use crate::{BackplaneConfig, ConfigError};
use serde_json::Value;

// ---------------------------------------------------------------------------
// ConfigDefaults
// ---------------------------------------------------------------------------

/// Documents the canonical default value for each [`BackplaneConfig`] field.
pub struct ConfigDefaults;

impl ConfigDefaults {
    /// Default log level.
    pub const LOG_LEVEL: &'static str = "info";

    /// Default port (none — daemon not started unless configured).
    pub const PORT: Option<u16> = None;

    /// Default bind address (none).
    pub const BIND_ADDRESS: Option<&'static str> = None;

    /// Default backend name (none).
    pub const DEFAULT_BACKEND: Option<&'static str> = None;

    /// Default workspace directory (none).
    pub const WORKSPACE_DIR: Option<&'static str> = None;

    /// Default receipts directory (none).
    pub const RECEIPTS_DIR: Option<&'static str> = None;

    /// Return a [`BackplaneConfig`] with all canonical defaults applied.
    pub fn config() -> BackplaneConfig {
        BackplaneConfig::default()
    }

    /// Return a JSON [`Value`] representing the full set of defaults.
    pub fn as_value() -> Value {
        serde_json::to_value(BackplaneConfig::default()).expect("default config should serialize")
    }
}

// ---------------------------------------------------------------------------
// with_defaults
// ---------------------------------------------------------------------------

/// Merge `partial` (a possibly-incomplete JSON object) with the canonical
/// defaults and deserialize into a [`BackplaneConfig`].
///
/// Fields present in `partial` take precedence; any absent fields are
/// populated from [`ConfigDefaults`].
pub fn with_defaults(partial: &Value) -> Result<BackplaneConfig, ConfigError> {
    let defaults = ConfigDefaults::as_value();
    let merged = merge_json(&defaults, partial);
    serde_json::from_value(merged).map_err(|e| ConfigError::ParseError {
        reason: e.to_string(),
    })
}

/// Recursively merge `overlay` into `base`. Overlay values win.
fn merge_json(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            let mut merged = b.clone();
            for (key, val) in o {
                let new_val = match (merged.get(key), val) {
                    (Some(existing), val) if existing.is_object() && val.is_object() => {
                        merge_json(existing, val)
                    }
                    _ => val.clone(),
                };
                merged.insert(key.clone(), new_val);
            }
            Value::Object(merged)
        }
        // Non-object overlay replaces base entirely.
        (_, overlay) => overlay.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_config_matches_backplane_default() {
        assert_eq!(ConfigDefaults::config(), BackplaneConfig::default());
    }

    #[test]
    fn defaults_as_value_is_object() {
        assert!(ConfigDefaults::as_value().is_object());
    }

    #[test]
    fn with_defaults_empty_object_gives_defaults() {
        let cfg = with_defaults(&json!({})).unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
        assert!(cfg.backends.is_empty());
    }

    #[test]
    fn with_defaults_preserves_overrides() {
        let partial = json!({ "log_level": "debug", "default_backend": "mock" });
        let cfg = with_defaults(&partial).unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("debug"));
        assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    }

    #[test]
    fn with_defaults_fills_missing_log_level() {
        let partial = json!({ "default_backend": "mock" });
        let cfg = with_defaults(&partial).unwrap();
        // log_level should be filled from defaults
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
    }

    #[test]
    fn with_defaults_with_backends() {
        let partial = json!({
            "backends": { "mock": { "type": "mock" } }
        });
        let cfg = with_defaults(&partial).unwrap();
        assert_eq!(cfg.backends.len(), 1);
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
    }

    #[test]
    fn with_defaults_port_override() {
        let partial = json!({ "port": 8080 });
        let cfg = with_defaults(&partial).unwrap();
        assert_eq!(cfg.port, Some(8080));
    }

    #[test]
    fn with_defaults_invalid_type_returns_error() {
        let partial = json!({ "log_level": 42 });
        assert!(with_defaults(&partial).is_err());
    }

    #[test]
    fn merge_json_deep_objects() {
        let base = json!({ "a": { "x": 1, "y": 2 } });
        let overlay = json!({ "a": { "y": 3, "z": 4 } });
        let merged = merge_json(&base, &overlay);
        assert_eq!(merged["a"]["x"], 1);
        assert_eq!(merged["a"]["y"], 3);
        assert_eq!(merged["a"]["z"], 4);
    }

    #[test]
    fn merge_json_overlay_wins_for_scalars() {
        let base = json!({ "key": "old" });
        let overlay = json!({ "key": "new" });
        let merged = merge_json(&base, &overlay);
        assert_eq!(merged["key"], "new");
    }

    #[test]
    fn config_defaults_constants() {
        assert_eq!(ConfigDefaults::LOG_LEVEL, "info");
        assert_eq!(ConfigDefaults::PORT, None);
        assert_eq!(ConfigDefaults::BIND_ADDRESS, None);
        assert_eq!(ConfigDefaults::DEFAULT_BACKEND, None);
        assert_eq!(ConfigDefaults::WORKSPACE_DIR, None);
        assert_eq!(ConfigDefaults::RECEIPTS_DIR, None);
    }
}
