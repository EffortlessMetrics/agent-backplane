// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep diff analysis with impact categorisation for hot-reload.
//!
//! [`ConfigDiffAnalyzer`] inspects a [`diff::ConfigDiff`] and classifies
//! every change into an [`Impact`] level so that the runtime can decide
//! whether a live reload is safe.

#![allow(dead_code)]

use crate::diff::{self, ConfigChange, ConfigDiff};
use crate::BackplaneConfig;
use std::fmt;

// ---------------------------------------------------------------------------
// Impact
// ---------------------------------------------------------------------------

/// How disruptive a single configuration change is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Impact {
    /// The change can be applied at runtime with no observable disruption.
    Safe,
    /// The change requires a controlled restart of affected subsystems.
    RestartRequired,
    /// The change is structurally incompatible and cannot be applied without
    /// a full restart.
    Breaking,
}

impl fmt::Display for Impact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Impact::Safe => f.write_str("safe"),
            Impact::RestartRequired => f.write_str("restart-required"),
            Impact::Breaking => f.write_str("breaking"),
        }
    }
}

// ---------------------------------------------------------------------------
// CategorizedChange
// ---------------------------------------------------------------------------

/// A [`ConfigChange`] paired with its [`Impact`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorizedChange {
    /// The underlying change.
    pub change: ConfigChange,
    /// Assessed impact level.
    pub impact: Impact,
}

impl fmt::Display for CategorizedChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.impact, self.change)
    }
}

// ---------------------------------------------------------------------------
// AnalysisResult
// ---------------------------------------------------------------------------

/// Result of analysing a full [`ConfigDiff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisResult {
    /// Every change with its impact classification.
    pub categorized: Vec<CategorizedChange>,
}

impl AnalysisResult {
    /// Overall (worst-case) impact across all changes.
    pub fn overall_impact(&self) -> Impact {
        self.categorized
            .iter()
            .map(|c| c.impact)
            .max()
            .unwrap_or(Impact::Safe)
    }

    /// `true` if every change is [`Impact::Safe`].
    pub fn is_safe(&self) -> bool {
        self.overall_impact() == Impact::Safe
    }

    /// `true` when there are no changes at all.
    pub fn is_empty(&self) -> bool {
        self.categorized.is_empty()
    }

    /// Number of changes.
    pub fn len(&self) -> usize {
        self.categorized.len()
    }

    /// Iterator over changes matching a specific impact level.
    pub fn by_impact(&self, impact: Impact) -> impl Iterator<Item = &CategorizedChange> {
        self.categorized.iter().filter(move |c| c.impact == impact)
    }

    /// Count of changes at a given impact level.
    pub fn count_by_impact(&self, impact: Impact) -> usize {
        self.by_impact(impact).count()
    }
}

impl fmt::Display for AnalysisResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.categorized.is_empty() {
            return write!(f, "(no changes)");
        }
        for (i, cc) in self.categorized.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{cc}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ConfigDiffAnalyzer
// ---------------------------------------------------------------------------

/// Analyses a [`ConfigDiff`] and classifies every change by [`Impact`].
///
/// Default classification rules:
/// - `log_level`, `receipts_dir`, `workspace_dir` → [`Impact::Safe`]
/// - `default_backend`, `policy_profiles` → [`Impact::RestartRequired`]
/// - `bind_address`, `port` → [`Impact::Breaking`]
/// - Backend additions → [`Impact::Safe`]
/// - Backend modifications → [`Impact::RestartRequired`]
/// - Backend removals → [`Impact::Breaking`]
pub struct ConfigDiffAnalyzer {
    /// Custom overrides: field-name prefix → impact.
    overrides: Vec<(String, Impact)>,
}

impl Default for ConfigDiffAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigDiffAnalyzer {
    /// Create an analyser with the default classification rules.
    pub fn new() -> Self {
        Self {
            overrides: Vec::new(),
        }
    }

    /// Register a custom override: any field whose name starts with `prefix`
    /// will be classified as `impact` instead of the default.
    pub fn with_override(mut self, prefix: impl Into<String>, impact: Impact) -> Self {
        self.overrides.push((prefix.into(), impact));
        self
    }

    /// Analyse a pre-computed diff.
    pub fn analyze_diff(&self, diff: &ConfigDiff) -> AnalysisResult {
        let categorized = diff
            .changes
            .iter()
            .map(|ch| {
                let field = change_field(ch);
                let impact = self.classify(field, ch);
                CategorizedChange {
                    change: ch.clone(),
                    impact,
                }
            })
            .collect();
        AnalysisResult { categorized }
    }

    /// Diff two configs and analyse in one step.
    pub fn analyze(&self, old: &BackplaneConfig, new: &BackplaneConfig) -> AnalysisResult {
        let d = diff::diff(old, new);
        self.analyze_diff(&d)
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn classify(&self, field: &str, change: &ConfigChange) -> Impact {
        // Check custom overrides first (last registered wins).
        for (prefix, impact) in self.overrides.iter().rev() {
            if field.starts_with(prefix.as_str()) {
                return *impact;
            }
        }
        self.default_classify(field, change)
    }

    fn default_classify(&self, field: &str, change: &ConfigChange) -> Impact {
        match field {
            "log_level" | "receipts_dir" | "workspace_dir" => Impact::Safe,
            "default_backend" | "policy_profiles" => Impact::RestartRequired,
            "bind_address" | "port" => Impact::Breaking,
            f if f.starts_with("backends.") => match change {
                ConfigChange::Added(..) => Impact::Safe,
                ConfigChange::Modified(..) => Impact::RestartRequired,
                ConfigChange::Removed(..) => Impact::Breaking,
            },
            _ => Impact::RestartRequired,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the field name from a `ConfigChange`.
fn change_field(ch: &ConfigChange) -> &str {
    match ch {
        ConfigChange::Added(f, _) => f,
        ConfigChange::Removed(f) => f,
        ConfigChange::Modified(f, _, _) => f,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendEntry, BackplaneConfig};
    use std::collections::BTreeMap;

    fn base() -> BackplaneConfig {
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
    fn identical_configs_are_safe() {
        let a = ConfigDiffAnalyzer::new();
        let r = a.analyze(&base(), &base());
        assert!(r.is_empty());
        assert!(r.is_safe());
    }

    #[test]
    fn log_level_change_is_safe() {
        let mut b = base();
        b.log_level = Some("debug".into());
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::Safe);
    }

    #[test]
    fn receipts_dir_change_is_safe() {
        let mut b = base();
        b.receipts_dir = Some("/new".into());
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::Safe);
    }

    #[test]
    fn workspace_dir_change_is_safe() {
        let mut b = base();
        b.workspace_dir = Some("/ws".into());
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::Safe);
    }

    #[test]
    fn default_backend_change_requires_restart() {
        let mut b = base();
        b.default_backend = Some("other".into());
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::RestartRequired);
    }

    #[test]
    fn policy_profiles_change_requires_restart() {
        let mut b = base();
        b.policy_profiles = vec!["p.toml".into()];
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::RestartRequired);
    }

    #[test]
    fn bind_address_change_is_breaking() {
        let mut old = base();
        old.bind_address = Some("127.0.0.1".into());
        let mut new = base();
        new.bind_address = Some("0.0.0.0".into());
        let r = ConfigDiffAnalyzer::new().analyze(&old, &new);
        assert_eq!(r.overall_impact(), Impact::Breaking);
    }

    #[test]
    fn port_change_is_breaking() {
        let mut old = base();
        old.port = Some(8080);
        let mut new = base();
        new.port = Some(9090);
        let r = ConfigDiffAnalyzer::new().analyze(&old, &new);
        assert_eq!(r.overall_impact(), Impact::Breaking);
    }

    #[test]
    fn backend_addition_is_safe() {
        let mut b = base();
        b.backends.insert("m".into(), BackendEntry::Mock {});
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::Safe);
    }

    #[test]
    fn backend_modification_requires_restart() {
        let mut old = base();
        old.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let mut new = base();
        new.backends.insert(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        );
        let r = ConfigDiffAnalyzer::new().analyze(&old, &new);
        assert_eq!(r.overall_impact(), Impact::RestartRequired);
    }

    #[test]
    fn backend_removal_is_breaking() {
        let mut old = base();
        old.backends.insert("m".into(), BackendEntry::Mock {});
        let r = ConfigDiffAnalyzer::new().analyze(&old, &base());
        assert_eq!(r.overall_impact(), Impact::Breaking);
    }

    #[test]
    fn custom_override_changes_impact() {
        let mut b = base();
        b.log_level = Some("debug".into());
        let a = ConfigDiffAnalyzer::new().with_override("log_level", Impact::Breaking);
        let r = a.analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::Breaking);
    }

    #[test]
    fn last_override_wins() {
        let mut b = base();
        b.log_level = Some("debug".into());
        let a = ConfigDiffAnalyzer::new()
            .with_override("log_level", Impact::Breaking)
            .with_override("log_level", Impact::Safe);
        let r = a.analyze(&base(), &b);
        assert_eq!(r.overall_impact(), Impact::Safe);
    }

    #[test]
    fn overall_impact_is_worst_case() {
        let mut new = base();
        new.log_level = Some("debug".into()); // safe
        new.port = Some(9090); // breaking
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &new);
        assert_eq!(r.overall_impact(), Impact::Breaking);
    }

    #[test]
    fn count_by_impact_works() {
        let mut new = base();
        new.log_level = Some("debug".into()); // safe
        new.receipts_dir = Some("/r".into()); // safe
        new.port = Some(9090); // breaking
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &new);
        assert_eq!(r.count_by_impact(Impact::Safe), 2);
        assert_eq!(r.count_by_impact(Impact::Breaking), 1);
    }

    #[test]
    fn display_format_includes_impact() {
        let mut new = base();
        new.log_level = Some("debug".into());
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &new);
        let text = r.to_string();
        assert!(text.contains("[safe]"));
    }

    #[test]
    fn empty_result_display() {
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &base());
        assert_eq!(r.to_string(), "(no changes)");
    }

    #[test]
    fn impact_display() {
        assert_eq!(Impact::Safe.to_string(), "safe");
        assert_eq!(Impact::RestartRequired.to_string(), "restart-required");
        assert_eq!(Impact::Breaking.to_string(), "breaking");
    }

    #[test]
    fn impact_ordering() {
        assert!(Impact::Safe < Impact::RestartRequired);
        assert!(Impact::RestartRequired < Impact::Breaking);
    }

    #[test]
    fn by_impact_iterator() {
        let mut new = base();
        new.log_level = Some("debug".into()); // safe
        new.default_backend = Some("other".into()); // restart
        let r = ConfigDiffAnalyzer::new().analyze(&base(), &new);
        let safe: Vec<_> = r.by_impact(Impact::Safe).collect();
        assert_eq!(safe.len(), 1);
    }

    #[test]
    fn analyze_diff_same_as_analyze() {
        let mut new = base();
        new.log_level = Some("debug".into());
        let d = diff::diff(&base(), &new);
        let a = ConfigDiffAnalyzer::new();
        let r1 = a.analyze_diff(&d);
        let r2 = a.analyze(&base(), &new);
        assert_eq!(r1, r2);
    }

    #[test]
    fn prefix_override_matches_backends() {
        let mut old = base();
        old.backends.insert("m".into(), BackendEntry::Mock {});
        let a = ConfigDiffAnalyzer::new().with_override("backends.", Impact::Safe);
        // removal is normally breaking, but override makes it safe
        let r = a.analyze(&old, &base());
        assert_eq!(r.overall_impact(), Impact::Safe);
    }
}
