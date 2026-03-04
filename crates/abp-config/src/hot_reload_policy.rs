// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hot-reload policy — decides which configuration changes can be applied
//! without restarting the process.
//!
//! [`HotReloadPolicy`] is consulted after [`crate::diff_analyzer::ConfigDiffAnalyzer`]
//! has categorised the changes.  It answers one question: *"given this set of
//! categorised changes, should the runtime apply them live?"*

#![allow(dead_code)]

use crate::diff_analyzer::{AnalysisResult, Impact};
use std::fmt;

// ---------------------------------------------------------------------------
// ReloadDecision
// ---------------------------------------------------------------------------

/// The policy's recommendation for a given set of changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadDecision {
    /// All changes may be applied live.
    Apply,
    /// Changes may be applied but affected subsystems need a restart cycle.
    ApplyWithRestart {
        /// Human-readable reasons for the required restart.
        reasons: Vec<String>,
    },
    /// Changes must be rejected — a full process restart is needed.
    Reject {
        /// Human-readable reasons for the rejection.
        reasons: Vec<String>,
    },
}

impl ReloadDecision {
    /// `true` when the decision is [`Apply`](Self::Apply).
    pub fn is_apply(&self) -> bool {
        matches!(self, ReloadDecision::Apply)
    }

    /// `true` when the decision is [`Reject`](Self::Reject).
    pub fn is_reject(&self) -> bool {
        matches!(self, ReloadDecision::Reject { .. })
    }
}

impl fmt::Display for ReloadDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReloadDecision::Apply => f.write_str("apply"),
            ReloadDecision::ApplyWithRestart { reasons } => {
                write!(f, "apply-with-restart: {}", reasons.join("; "))
            }
            ReloadDecision::Reject { reasons } => {
                write!(f, "reject: {}", reasons.join("; "))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HotReloadPolicy
// ---------------------------------------------------------------------------

/// Configurable rules for deciding whether a config diff can be hot-reloaded.
///
/// The default policy:
/// - [`Impact::Safe`] → always apply
/// - [`Impact::RestartRequired`] → apply with restart when `allow_restart` is
///   `true`, reject otherwise
/// - [`Impact::Breaking`] → always reject
///
/// Use [`with_allow_restart`](Self::with_allow_restart) and
/// [`with_allow_breaking`](Self::with_allow_breaking) to relax constraints.
pub struct HotReloadPolicy {
    allow_restart: bool,
    allow_breaking: bool,
    max_changes: Option<usize>,
}

impl Default for HotReloadPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl HotReloadPolicy {
    /// Create a policy with conservative defaults (only safe changes allowed).
    pub fn new() -> Self {
        Self {
            allow_restart: false,
            allow_breaking: false,
            max_changes: None,
        }
    }

    /// Create a permissive policy that allows everything.
    pub fn permissive() -> Self {
        Self {
            allow_restart: true,
            allow_breaking: true,
            max_changes: None,
        }
    }

    /// Allow changes that require a subsystem restart.
    pub fn with_allow_restart(mut self, allow: bool) -> Self {
        self.allow_restart = allow;
        self
    }

    /// Allow breaking changes (bind_address / port).
    pub fn with_allow_breaking(mut self, allow: bool) -> Self {
        self.allow_breaking = allow;
        self
    }

    /// Set a maximum number of simultaneous changes to accept.
    pub fn with_max_changes(mut self, max: usize) -> Self {
        self.max_changes = Some(max);
        self
    }

    /// Evaluate an [`AnalysisResult`] and return a [`ReloadDecision`].
    pub fn evaluate(&self, analysis: &AnalysisResult) -> ReloadDecision {
        if analysis.is_empty() {
            return ReloadDecision::Apply;
        }

        // Check max-changes gate first.
        if let Some(max) = self.max_changes {
            if analysis.len() > max {
                return ReloadDecision::Reject {
                    reasons: vec![format!(
                        "too many changes ({}, max {})",
                        analysis.len(),
                        max
                    )],
                };
            }
        }

        let mut restart_reasons: Vec<String> = Vec::new();
        let mut reject_reasons: Vec<String> = Vec::new();

        for cc in &analysis.categorized {
            match cc.impact {
                Impact::Safe => { /* always OK */ }
                Impact::RestartRequired => {
                    if !self.allow_restart {
                        reject_reasons.push(format!("{} requires restart", cc.change));
                    } else {
                        restart_reasons.push(format!("{}", cc.change));
                    }
                }
                Impact::Breaking => {
                    if !self.allow_breaking {
                        reject_reasons.push(format!("{} is a breaking change", cc.change));
                    } else {
                        restart_reasons.push(format!("{} (breaking)", cc.change));
                    }
                }
            }
        }

        if !reject_reasons.is_empty() {
            ReloadDecision::Reject {
                reasons: reject_reasons,
            }
        } else if !restart_reasons.is_empty() {
            ReloadDecision::ApplyWithRestart {
                reasons: restart_reasons,
            }
        } else {
            ReloadDecision::Apply
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_analyzer::ConfigDiffAnalyzer;
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

    fn analyze(old: &BackplaneConfig, new: &BackplaneConfig) -> AnalysisResult {
        ConfigDiffAnalyzer::new().analyze(old, new)
    }

    #[test]
    fn empty_analysis_always_applies() {
        let r = HotReloadPolicy::new().evaluate(&analyze(&base(), &base()));
        assert!(r.is_apply());
    }

    #[test]
    fn safe_changes_apply_under_default_policy() {
        let mut b = base();
        b.log_level = Some("debug".into());
        let r = HotReloadPolicy::new().evaluate(&analyze(&base(), &b));
        assert!(r.is_apply());
    }

    #[test]
    fn restart_changes_rejected_by_default() {
        let mut b = base();
        b.default_backend = Some("other".into());
        let r = HotReloadPolicy::new().evaluate(&analyze(&base(), &b));
        assert!(r.is_reject());
    }

    #[test]
    fn restart_changes_allowed_when_configured() {
        let mut b = base();
        b.default_backend = Some("other".into());
        let r = HotReloadPolicy::new()
            .with_allow_restart(true)
            .evaluate(&analyze(&base(), &b));
        assert!(matches!(r, ReloadDecision::ApplyWithRestart { .. }));
    }

    #[test]
    fn breaking_changes_rejected_by_default() {
        let mut b = base();
        b.port = Some(9090);
        let r = HotReloadPolicy::new()
            .with_allow_restart(true)
            .evaluate(&analyze(&base(), &b));
        assert!(r.is_reject());
    }

    #[test]
    fn breaking_changes_allowed_when_configured() {
        let mut b = base();
        b.port = Some(9090);
        let r = HotReloadPolicy::permissive().evaluate(&analyze(&base(), &b));
        assert!(!r.is_reject());
    }

    #[test]
    fn max_changes_gate() {
        let mut b = base();
        b.log_level = Some("debug".into());
        b.receipts_dir = Some("/r".into());
        b.workspace_dir = Some("/w".into());
        let r = HotReloadPolicy::new()
            .with_max_changes(2)
            .evaluate(&analyze(&base(), &b));
        assert!(r.is_reject());
    }

    #[test]
    fn max_changes_allows_within_limit() {
        let mut b = base();
        b.log_level = Some("debug".into());
        let r = HotReloadPolicy::new()
            .with_max_changes(5)
            .evaluate(&analyze(&base(), &b));
        assert!(r.is_apply());
    }

    #[test]
    fn permissive_accepts_everything() {
        let mut b = base();
        b.port = Some(9090);
        b.default_backend = Some("other".into());
        b.log_level = Some("debug".into());
        let r = HotReloadPolicy::permissive().evaluate(&analyze(&base(), &b));
        assert!(!r.is_reject());
    }

    #[test]
    fn decision_display_apply() {
        assert_eq!(ReloadDecision::Apply.to_string(), "apply");
    }

    #[test]
    fn decision_display_reject() {
        let d = ReloadDecision::Reject {
            reasons: vec!["port".into()],
        };
        assert!(d.to_string().contains("reject"));
    }

    #[test]
    fn decision_display_restart() {
        let d = ReloadDecision::ApplyWithRestart {
            reasons: vec!["backend".into()],
        };
        let s = d.to_string();
        assert!(s.contains("apply-with-restart"));
    }

    #[test]
    fn mixed_safe_and_restart() {
        let mut b = base();
        b.log_level = Some("debug".into()); // safe
        b.default_backend = Some("other".into()); // restart
        let r = HotReloadPolicy::new()
            .with_allow_restart(true)
            .evaluate(&analyze(&base(), &b));
        assert!(matches!(r, ReloadDecision::ApplyWithRestart { .. }));
    }

    #[test]
    fn backend_addition_safe_under_default_policy() {
        let mut b = base();
        b.backends.insert("m".into(), BackendEntry::Mock {});
        let r = HotReloadPolicy::new().evaluate(&analyze(&base(), &b));
        assert!(r.is_apply());
    }
}
