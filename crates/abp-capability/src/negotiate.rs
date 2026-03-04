// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pre-execution capability negotiation with policy-based enforcement.
//!
//! This module provides [`pre_negotiate`] to validate backend support before
//! running a work order, and [`apply_policy`] to enforce a [`NegotiationPolicy`]
//! against the negotiation result.

use crate::{NegotiationResult, negotiate_capabilities};
use abp_core::{Capability, CapabilityManifest};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Policy that controls how negotiation failures are handled.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::NegotiationPolicy;
///
/// let policy = NegotiationPolicy::Strict;
/// assert!(matches!(policy, NegotiationPolicy::Strict));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NegotiationPolicy {
    /// Any unsupported capability causes a hard failure.
    #[default]
    Strict,
    /// Proceed with emulation; warn on unsupported capabilities.
    BestEffort,
    /// Proceed regardless; document all limitations.
    Permissive,
}

impl fmt::Display for NegotiationPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::BestEffort => write!(f, "best-effort"),
            Self::Permissive => write!(f, "permissive"),
        }
    }
}

/// Error returned when [`apply_policy`] rejects a negotiation result.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::{NegotiationError, NegotiationPolicy};
/// use abp_core::Capability;
///
/// let err = NegotiationError {
///     policy: NegotiationPolicy::Strict,
///     unsupported: vec![(Capability::Streaming, "not declared in manifest".into())],
///     warnings: vec![],
/// };
/// assert!(err.to_string().contains("strict"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiationError {
    /// The policy that was applied.
    pub policy: NegotiationPolicy,
    /// Capabilities that are unsupported.
    pub unsupported: Vec<(Capability, String)>,
    /// Capabilities that triggered warnings (emulated with fidelity loss).
    pub warnings: Vec<Capability>,
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capability negotiation failed (policy: {}): {} unsupported",
            self.policy,
            self.unsupported.len(),
        )?;
        if !self.unsupported.is_empty() {
            let names: Vec<String> = self
                .unsupported
                .iter()
                .map(|(c, _)| format!("{c:?}"))
                .collect();
            write!(f, " [{}]", names.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for NegotiationError {}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Pre-negotiate required capabilities against a backend manifest.
///
/// Checks each required capability against the manifest and returns a
/// [`NegotiationResult`] with supported/emulated/unsupported classifications.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::pre_negotiate;
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
/// manifest.insert(Capability::ToolUse, CoreSupportLevel::Emulated);
///
/// let result = pre_negotiate(
///     &[Capability::Streaming, Capability::ToolUse, Capability::Vision],
///     &manifest,
/// );
/// assert_eq!(result.native.len(), 1);
/// assert_eq!(result.emulated.len(), 1);
/// assert_eq!(result.unsupported.len(), 1);
/// ```
#[must_use]
pub fn pre_negotiate(required: &[Capability], manifest: &CapabilityManifest) -> NegotiationResult {
    negotiate_capabilities(required, manifest)
}

/// Apply a [`NegotiationPolicy`] to a negotiation result.
///
/// - **Strict**: returns `Err` if any capabilities are unsupported.
/// - **BestEffort**: returns `Err` only if there are unsupported capabilities
///   that cannot be emulated (i.e., truly unsupported). Warnings are included
///   for emulated capabilities with fidelity loss.
/// - **Permissive**: always returns `Ok(())`.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::{pre_negotiate, apply_policy, NegotiationPolicy};
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let result = pre_negotiate(&[Capability::Streaming], &manifest);
/// assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
/// ```
pub fn apply_policy(
    result: &NegotiationResult,
    policy: NegotiationPolicy,
) -> Result<(), NegotiationError> {
    let warning_caps: Vec<Capability> = result
        .warnings()
        .into_iter()
        .map(|(c, _)| c.clone())
        .collect();

    match policy {
        NegotiationPolicy::Strict => {
            if result.unsupported.is_empty() {
                Ok(())
            } else {
                Err(NegotiationError {
                    policy,
                    unsupported: result.unsupported.clone(),
                    warnings: warning_caps,
                })
            }
        }
        NegotiationPolicy::BestEffort => {
            if result.unsupported.is_empty() {
                Ok(())
            } else {
                Err(NegotiationError {
                    policy,
                    unsupported: result.unsupported.clone(),
                    warnings: warning_caps,
                })
            }
        }
        NegotiationPolicy::Permissive => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::SupportLevel as CoreSupportLevel;

    fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
        entries.iter().cloned().collect()
    }

    #[test]
    fn pre_negotiate_all_native() {
        let m = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
        assert_eq!(r.native.len(), 2);
        assert!(r.emulated.is_empty());
        assert!(r.unsupported.is_empty());
        assert!(r.is_viable());
    }

    #[test]
    fn pre_negotiate_all_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
        assert!(r.native.is_empty());
        assert!(r.emulated.is_empty());
        assert_eq!(r.unsupported.len(), 2);
        assert!(!r.is_viable());
    }

    #[test]
    fn pre_negotiate_mixed() {
        let m = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let r = pre_negotiate(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &m,
        );
        assert_eq!(r.native.len(), 1);
        assert_eq!(r.emulated.len(), 1);
        assert_eq!(r.unsupported.len(), 1);
    }

    #[test]
    fn pre_negotiate_empty_required() {
        let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = pre_negotiate(&[], &m);
        assert!(r.native.is_empty());
        assert!(r.emulated.is_empty());
        assert!(r.unsupported.is_empty());
        assert!(r.is_viable());
    }

    #[test]
    fn pre_negotiate_empty_manifest() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[], &m);
        assert!(r.is_viable());
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn policy_strict_passes_all_native() {
        let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = pre_negotiate(&[Capability::Streaming], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn policy_strict_fails_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Streaming], &m);
        let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
        assert_eq!(err.policy, NegotiationPolicy::Strict);
        assert_eq!(err.unsupported.len(), 1);
    }

    #[test]
    fn policy_strict_passes_emulated() {
        let m = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
        let r = pre_negotiate(&[Capability::ToolUse], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn policy_best_effort_passes_all_native() {
        let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = pre_negotiate(&[Capability::Streaming], &m);
        assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
    }

    #[test]
    fn policy_best_effort_fails_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Vision], &m);
        let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
        assert_eq!(err.policy, NegotiationPolicy::BestEffort);
        assert_eq!(err.unsupported.len(), 1);
    }

    #[test]
    fn policy_permissive_always_passes() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn policy_permissive_with_all_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &m,
        );
        assert!(!r.is_viable());
        assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn negotiation_error_display() {
        let err = NegotiationError {
            policy: NegotiationPolicy::Strict,
            unsupported: vec![(Capability::Vision, "not available".into())],
            warnings: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("strict"));
        assert!(msg.contains("1 unsupported"));
        assert!(msg.contains("Vision"));
    }

    #[test]
    fn negotiation_error_multiple_unsupported() {
        let err = NegotiationError {
            policy: NegotiationPolicy::BestEffort,
            unsupported: vec![
                (Capability::Vision, "not available".into()),
                (Capability::Audio, "not available".into()),
            ],
            warnings: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("2 unsupported"));
        assert!(msg.contains("Vision"));
        assert!(msg.contains("Audio"));
    }

    #[test]
    fn policy_default_is_strict() {
        assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
    }

    #[test]
    fn policy_display() {
        assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
        assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
        assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
    }

    #[test]
    fn pre_negotiate_restricted_classified_as_emulated() {
        let m = make_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        )]);
        let r = pre_negotiate(&[Capability::ToolBash], &m);
        assert!(r.native.is_empty());
        assert_eq!(r.emulated.len(), 1);
        assert!(r.unsupported.is_empty());
    }

    #[test]
    fn pre_negotiate_explicit_unsupported_in_manifest() {
        let m = make_manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
        let r = pre_negotiate(&[Capability::Vision], &m);
        assert!(r.native.is_empty());
        assert!(r.emulated.is_empty());
        assert_eq!(r.unsupported.len(), 1);
    }

    #[test]
    fn strict_policy_restricted_passes() {
        let m = make_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        )]);
        let r = pre_negotiate(&[Capability::ToolBash], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn negotiation_error_is_std_error() {
        let err = NegotiationError {
            policy: NegotiationPolicy::Strict,
            unsupported: vec![(Capability::Streaming, "missing".into())],
            warnings: vec![],
        };
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn policy_serde_roundtrip() {
        let policy = NegotiationPolicy::BestEffort;
        let json = serde_json::to_string(&policy).unwrap();
        let parsed: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }
}
