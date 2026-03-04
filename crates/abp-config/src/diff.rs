// SPDX-License-Identifier: MIT OR Apache-2.0
//! Configuration diffing — detect what changed between two configs.
//!
//! This module provides [`ConfigChange`], a tagged enum describing individual
//! field-level mutations, and [`ConfigDiff`] which collects all changes
//! between two [`BackplaneConfig`] snapshots into a human-readable report.

#![allow(dead_code, unused_imports)]

use crate::{BackendEntry, BackplaneConfig};
use std::collections::BTreeSet;
use std::fmt;

// ---------------------------------------------------------------------------
// ConfigChange
// ---------------------------------------------------------------------------

/// A single change detected between two configuration snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChange {
    /// A key/value was added (not present in old config).
    Added(String, String),
    /// A key was removed (present in old config but not new).
    Removed(String),
    /// A key's value changed from `old` to `new`.
    Modified(String, String, String),
}

impl fmt::Display for ConfigChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigChange::Added(key, value) => write!(f, "+ {key} = {value}"),
            ConfigChange::Removed(key) => write!(f, "- {key}"),
            ConfigChange::Modified(key, old, new) => {
                write!(f, "~ {key}: {old} -> {new}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigDiff
// ---------------------------------------------------------------------------

/// Aggregated set of changes between two [`BackplaneConfig`] values.
///
/// Obtain one via [`diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDiff {
    /// Individual changes detected.
    pub changes: Vec<ConfigChange>,
}

impl ConfigDiff {
    /// Returns `true` when the two configs are identical.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Number of individual changes.
    pub fn len(&self) -> usize {
        self.changes.len()
    }
}

impl fmt::Display for ConfigDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.changes.is_empty() {
            return write!(f, "(no changes)");
        }
        for (i, change) in self.changes.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{change}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// diff()
// ---------------------------------------------------------------------------

/// Compare two configs and return a [`ConfigDiff`] describing all changes.
pub fn diff(old: &BackplaneConfig, new: &BackplaneConfig) -> ConfigDiff {
    let mut changes = Vec::new();

    diff_option(
        &mut changes,
        "default_backend",
        &old.default_backend,
        &new.default_backend,
    );
    diff_option(
        &mut changes,
        "workspace_dir",
        &old.workspace_dir,
        &new.workspace_dir,
    );
    diff_option(&mut changes, "log_level", &old.log_level, &new.log_level);
    diff_option(
        &mut changes,
        "receipts_dir",
        &old.receipts_dir,
        &new.receipts_dir,
    );
    diff_option(
        &mut changes,
        "bind_address",
        &old.bind_address,
        &new.bind_address,
    );

    // Port
    let port_old = old.port.map(|p| p.to_string());
    let port_new = new.port.map(|p| p.to_string());
    diff_option(&mut changes, "port", &port_old, &port_new);

    // Policy profiles
    if old.policy_profiles != new.policy_profiles {
        let old_val = format!("{:?}", old.policy_profiles);
        let new_val = format!("{:?}", new.policy_profiles);
        changes.push(ConfigChange::Modified(
            "policy_profiles".into(),
            old_val,
            new_val,
        ));
    }

    // Backends
    let all_keys: BTreeSet<&String> = old.backends.keys().chain(new.backends.keys()).collect();
    for key in all_keys {
        let path = format!("backends.{key}");
        match (old.backends.get(key), new.backends.get(key)) {
            (None, Some(entry)) => {
                changes.push(ConfigChange::Added(path, format_backend(entry)));
            }
            (Some(_), None) => {
                changes.push(ConfigChange::Removed(path));
            }
            (Some(a), Some(b)) if a != b => {
                changes.push(ConfigChange::Modified(
                    path,
                    format_backend(a),
                    format_backend(b),
                ));
            }
            _ => {}
        }
    }

    ConfigDiff { changes }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn diff_option(
    changes: &mut Vec<ConfigChange>,
    name: &str,
    old: &Option<String>,
    new: &Option<String>,
) {
    match (old, new) {
        (None, Some(v)) => changes.push(ConfigChange::Added(name.into(), v.clone())),
        (Some(_), None) => changes.push(ConfigChange::Removed(name.into())),
        (Some(a), Some(b)) if a != b => {
            changes.push(ConfigChange::Modified(name.into(), a.clone(), b.clone()));
        }
        _ => {}
    }
}

fn format_backend(entry: &BackendEntry) -> String {
    match entry {
        BackendEntry::Mock {} => "mock".into(),
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            let mut s = format!("sidecar(command={command:?}");
            if !args.is_empty() {
                s.push_str(&format!(", args={args:?}"));
            }
            if let Some(t) = timeout_secs {
                s.push_str(&format!(", timeout={t}s"));
            }
            s.push(')');
            s
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BackendEntry;
    use std::collections::BTreeMap;

    fn base_config() -> BackplaneConfig {
        BackplaneConfig {
            default_backend: Some("mock".into()),
            workspace_dir: None,
            log_level: Some("info".into()),
            receipts_dir: None,
            bind_address: None,
            port: None,
            policy_profiles: Vec::new(),
            backends: BTreeMap::new(),
        }
    }

    #[test]
    fn identical_configs_produce_empty_diff() {
        let a = base_config();
        let d = diff(&a, &a);
        assert!(d.is_empty());
        assert_eq!(d.len(), 0);
    }

    #[test]
    fn modified_field_detected() {
        let old = base_config();
        let mut new = base_config();
        new.log_level = Some("debug".into());
        let d = diff(&old, &new);
        assert_eq!(d.len(), 1);
        assert!(matches!(&d.changes[0], ConfigChange::Modified(k, ..) if k == "log_level"));
    }

    #[test]
    fn added_field_detected() {
        let old = base_config();
        let mut new = base_config();
        new.workspace_dir = Some("/work".into());
        let d = diff(&old, &new);
        assert!(
            d.changes
                .iter()
                .any(|c| matches!(c, ConfigChange::Added(k, _) if k == "workspace_dir"))
        );
    }

    #[test]
    fn removed_field_detected() {
        let old = base_config();
        let mut new = base_config();
        new.default_backend = None;
        let d = diff(&old, &new);
        assert!(
            d.changes
                .iter()
                .any(|c| matches!(c, ConfigChange::Removed(k) if k == "default_backend"))
        );
    }

    #[test]
    fn backend_added_detected() {
        let old = base_config();
        let mut new = base_config();
        new.backends.insert("m".into(), BackendEntry::Mock {});
        let d = diff(&old, &new);
        assert!(
            d.changes
                .iter()
                .any(|c| matches!(c, ConfigChange::Added(k, _) if k == "backends.m"))
        );
    }

    #[test]
    fn backend_removed_detected() {
        let mut old = base_config();
        old.backends.insert("m".into(), BackendEntry::Mock {});
        let new = base_config();
        let d = diff(&old, &new);
        assert!(
            d.changes
                .iter()
                .any(|c| matches!(c, ConfigChange::Removed(k) if k == "backends.m"))
        );
    }

    #[test]
    fn backend_modified_detected() {
        let mut old = base_config();
        old.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let mut new = base_config();
        new.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let d = diff(&old, &new);
        assert!(
            d.changes
                .iter()
                .any(|c| matches!(c, ConfigChange::Modified(k, ..) if k == "backends.sc"))
        );
    }

    #[test]
    fn display_format_is_human_readable() {
        let old = base_config();
        let mut new = base_config();
        new.log_level = Some("debug".into());
        new.backends.insert("m".into(), BackendEntry::Mock {});
        let d = diff(&old, &new);
        let text = d.to_string();
        assert!(text.contains("~"));
        assert!(text.contains("+"));
    }

    #[test]
    fn multiple_changes_tracked() {
        let old = base_config();
        let mut new = base_config();
        new.log_level = Some("debug".into());
        new.port = Some(8080);
        new.default_backend = None;
        let d = diff(&old, &new);
        assert!(d.len() >= 3);
    }

    #[test]
    fn empty_diff_display() {
        let a = base_config();
        let d = diff(&a, &a);
        assert_eq!(d.to_string(), "(no changes)");
    }
}
