// SPDX-License-Identifier: MIT OR Apache-2.0
//! Version-to-version configuration migration.
//!
//! Provides [`ConfigMigration`], [`detect_version`], and [`apply_migrations`]
//! for upgrading older config formats to the latest schema.

use crate::{BackplaneConfig, ConfigError};
use serde_json::Value;

// ---------------------------------------------------------------------------
// ConfigVersion
// ---------------------------------------------------------------------------

/// Known config schema versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigVersion {
    /// V1: original flat schema (no `config_version` key).
    V1,
    /// V2: adds `config_version = 2`, renames `log_level` → `log_level`
    /// (no actual rename — kept for demonstration), and nests daemon
    /// fields under `[daemon]`.
    V2,
}

impl ConfigVersion {
    /// The latest version that [`apply_migrations`] will target.
    pub const LATEST: ConfigVersion = ConfigVersion::V2;
}

impl std::fmt::Display for ConfigVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigVersion::V1 => f.write_str("v1"),
            ConfigVersion::V2 => f.write_str("v2"),
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigMigration
// ---------------------------------------------------------------------------

/// A single migration step between two adjacent versions.
#[derive(Debug, Clone)]
pub struct ConfigMigration {
    /// Source version.
    pub from: ConfigVersion,
    /// Target version.
    pub to: ConfigVersion,
    /// Human-readable description.
    pub description: &'static str,
}

// ---------------------------------------------------------------------------
// detect_version
// ---------------------------------------------------------------------------

/// Detect the schema version of a raw config [`Value`].
///
/// * If `config_version` is present and equals `2`, returns [`ConfigVersion::V2`].
/// * Otherwise returns [`ConfigVersion::V1`].
pub fn detect_version(value: &Value) -> ConfigVersion {
    match value.get("config_version").and_then(|v| v.as_u64()) {
        Some(2) => ConfigVersion::V2,
        _ => ConfigVersion::V1,
    }
}

// ---------------------------------------------------------------------------
// migrate_v1_to_v2
// ---------------------------------------------------------------------------

/// Migrate a V1 config value to V2 format.
///
/// Changes applied:
/// - Adds `config_version = 2`.
/// - Moves top-level `bind_address` and `port` into a `[daemon]` sub-table
///   (the top-level keys are removed).
pub fn migrate_v1_to_v2(mut value: Value) -> Value {
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return value,
    };

    // Set version marker.
    obj.insert("config_version".into(), Value::Number(2.into()));

    // Nest daemon fields.
    let mut daemon = serde_json::Map::new();
    if let Some(addr) = obj.remove("bind_address") {
        daemon.insert("bind_address".into(), addr);
    }
    if let Some(port) = obj.remove("port") {
        daemon.insert("port".into(), port);
    }
    if !daemon.is_empty() {
        obj.insert("daemon".into(), Value::Object(daemon));
    }

    value
}

// ---------------------------------------------------------------------------
// apply_migrations
// ---------------------------------------------------------------------------

/// Apply all necessary migrations to bring `value` up to
/// [`ConfigVersion::LATEST`].
///
/// Returns the migrated value together with the list of migrations that were
/// applied (empty if the value was already at the latest version).
pub fn apply_migrations(value: Value) -> (Value, Vec<ConfigMigration>) {
    let mut current = value;
    let mut applied = Vec::new();

    loop {
        let version = detect_version(&current);
        if version >= ConfigVersion::LATEST {
            break;
        }
        match version {
            ConfigVersion::V1 => {
                current = migrate_v1_to_v2(current);
                applied.push(ConfigMigration {
                    from: ConfigVersion::V1,
                    to: ConfigVersion::V2,
                    description: "nest bind_address/port into [daemon], add config_version = 2",
                });
            }
            ConfigVersion::V2 => break,
        }
    }

    (current, applied)
}

/// Parse a migrated V2 value back into a [`BackplaneConfig`].
///
/// After migration, the `daemon` sub-table (if any) is flattened back into
/// the top-level fields that [`BackplaneConfig`] expects, and transient
/// migration-only keys (`config_version`, `daemon`) are stripped.
pub fn parse_migrated(mut value: Value) -> Result<BackplaneConfig, ConfigError> {
    if let Some(obj) = value.as_object_mut() {
        // Flatten daemon sub-table back into top-level.
        if let Some(daemon) = obj.remove("daemon") {
            if let Some(d) = daemon.as_object() {
                for (k, v) in d {
                    obj.entry(k.clone()).or_insert(v.clone());
                }
            }
        }
        // Remove migration-only key.
        obj.remove("config_version");
    }

    serde_json::from_value(value).map_err(|e| ConfigError::ParseError {
        reason: e.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_version_v1_when_no_marker() {
        let v = json!({ "log_level": "info" });
        assert_eq!(detect_version(&v), ConfigVersion::V1);
    }

    #[test]
    fn detect_version_v2_with_marker() {
        let v = json!({ "config_version": 2, "log_level": "info" });
        assert_eq!(detect_version(&v), ConfigVersion::V2);
    }

    #[test]
    fn detect_version_v1_when_marker_is_wrong_type() {
        let v = json!({ "config_version": "two" });
        assert_eq!(detect_version(&v), ConfigVersion::V1);
    }

    #[test]
    fn migrate_v1_to_v2_adds_version() {
        let v = json!({ "log_level": "info" });
        let migrated = migrate_v1_to_v2(v);
        assert_eq!(migrated["config_version"], 2);
    }

    #[test]
    fn migrate_v1_to_v2_nests_daemon_fields() {
        let v = json!({ "bind_address": "127.0.0.1", "port": 8080 });
        let migrated = migrate_v1_to_v2(v);
        assert_eq!(migrated["daemon"]["bind_address"], "127.0.0.1");
        assert_eq!(migrated["daemon"]["port"], 8080);
        assert!(migrated.get("bind_address").is_none());
        assert!(migrated.get("port").is_none());
    }

    #[test]
    fn migrate_v1_to_v2_no_daemon_when_no_daemon_fields() {
        let v = json!({ "log_level": "debug" });
        let migrated = migrate_v1_to_v2(v);
        assert!(migrated.get("daemon").is_none());
    }

    #[test]
    fn apply_migrations_from_v1() {
        let v = json!({ "log_level": "info", "bind_address": "0.0.0.0", "port": 3000 });
        let (migrated, steps) = apply_migrations(v);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].from, ConfigVersion::V1);
        assert_eq!(steps[0].to, ConfigVersion::V2);
        assert_eq!(detect_version(&migrated), ConfigVersion::V2);
    }

    #[test]
    fn apply_migrations_noop_when_already_latest() {
        let v = json!({ "config_version": 2, "log_level": "info" });
        let (migrated, steps) = apply_migrations(v.clone());
        assert!(steps.is_empty());
        assert_eq!(migrated, v);
    }

    #[test]
    fn migration_roundtrip_parse() {
        let v = json!({
            "log_level": "debug",
            "bind_address": "127.0.0.1",
            "port": 9090,
            "default_backend": "mock",
            "backends": { "mock": { "type": "mock" } }
        });
        let (migrated, _) = apply_migrations(v);
        let cfg = parse_migrated(migrated).expect("should parse");
        assert_eq!(cfg.log_level.as_deref(), Some("debug"));
        assert_eq!(cfg.bind_address.as_deref(), Some("127.0.0.1"));
        assert_eq!(cfg.port, Some(9090));
    }

    #[test]
    fn config_version_display() {
        assert_eq!(ConfigVersion::V1.to_string(), "v1");
        assert_eq!(ConfigVersion::V2.to_string(), "v2");
    }

    #[test]
    fn config_version_ordering() {
        assert!(ConfigVersion::V1 < ConfigVersion::V2);
    }

    #[test]
    fn migration_description_not_empty() {
        let v = json!({ "log_level": "info" });
        let (_, steps) = apply_migrations(v);
        for step in &steps {
            assert!(!step.description.is_empty());
        }
    }

    #[test]
    fn parse_migrated_strips_config_version() {
        let v = json!({ "config_version": 2, "log_level": "info" });
        let cfg = parse_migrated(v).unwrap();
        assert_eq!(cfg.log_level.as_deref(), Some("info"));
    }

    #[test]
    fn migrate_non_object_is_passthrough() {
        let v = Value::String("not an object".into());
        let migrated = migrate_v1_to_v2(v.clone());
        assert_eq!(migrated, v);
    }
}
