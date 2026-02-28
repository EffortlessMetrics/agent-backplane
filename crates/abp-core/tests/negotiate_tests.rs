// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the capability negotiation module.

use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, NegotiationRequest,
};
use abp_core::{Capability, CapabilityManifest, SupportLevel};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn manifest(pairs: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    pairs.iter().cloned().collect()
}

fn req(
    required: Vec<Capability>,
    preferred: Vec<Capability>,
    minimum_support: SupportLevel,
) -> NegotiationRequest {
    NegotiationRequest {
        required,
        preferred,
        minimum_support,
    }
}

// ---------------------------------------------------------------------------
// negotiate — basic satisfaction
// ---------------------------------------------------------------------------

#[test]
fn negotiate_all_required_satisfied() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let r = req(
        vec![Capability::Streaming, Capability::ToolRead],
        vec![],
        SupportLevel::Emulated,
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 2);
    assert!(result.unsatisfied.is_empty());
}

#[test]
fn negotiate_missing_required_is_unsatisfied() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let r = req(
        vec![Capability::Streaming, Capability::ToolWrite],
        vec![],
        SupportLevel::Emulated,
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied, vec![Capability::ToolWrite]);
}

#[test]
fn negotiate_empty_required_always_compatible() {
    let m = manifest(&[]);
    let r = req(vec![], vec![], SupportLevel::Native);
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.is_compatible);
    assert!(result.satisfied.is_empty());
}

// ---------------------------------------------------------------------------
// negotiate — minimum support thresholds
// ---------------------------------------------------------------------------

#[test]
fn negotiate_native_minimum_rejects_emulated() {
    let m = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let r = req(vec![Capability::ToolRead], vec![], SupportLevel::Native);
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied, vec![Capability::ToolRead]);
}

#[test]
fn negotiate_emulated_minimum_accepts_native() {
    let m = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
    let r = req(vec![Capability::ToolRead], vec![], SupportLevel::Emulated);
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.is_compatible);
}

#[test]
fn negotiate_emulated_minimum_accepts_restricted() {
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    )]);
    let r = req(
        vec![Capability::ToolBash],
        vec![],
        SupportLevel::Restricted {
            reason: String::new(),
        },
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.is_compatible);
}

#[test]
fn negotiate_restricted_minimum_rejects_unsupported() {
    let m = manifest(&[(Capability::ToolBash, SupportLevel::Unsupported)]);
    let r = req(
        vec![Capability::ToolBash],
        vec![],
        SupportLevel::Restricted {
            reason: String::new(),
        },
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(!result.is_compatible);
}

#[test]
fn negotiate_unsupported_minimum_accepts_anything() {
    let m = manifest(&[(Capability::ToolBash, SupportLevel::Unsupported)]);
    let r = req(
        vec![Capability::ToolBash],
        vec![],
        SupportLevel::Unsupported,
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.is_compatible);
}

// ---------------------------------------------------------------------------
// negotiate — preferred / bonus
// ---------------------------------------------------------------------------

#[test]
fn negotiate_preferred_appear_as_bonus() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::McpClient, SupportLevel::Native),
    ]);
    let r = req(
        vec![Capability::Streaming],
        vec![Capability::McpClient, Capability::McpServer],
        SupportLevel::Emulated,
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.is_compatible);
    assert_eq!(result.bonus, vec![Capability::McpClient]);
}

#[test]
fn negotiate_preferred_below_minimum_not_bonus() {
    let m = manifest(&[(Capability::McpClient, SupportLevel::Emulated)]);
    let r = req(
        vec![],
        vec![Capability::McpClient],
        SupportLevel::Native,
    );
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.bonus.is_empty());
}

#[test]
fn negotiate_preferred_missing_not_bonus() {
    let m = manifest(&[]);
    let r = req(vec![], vec![Capability::Streaming], SupportLevel::Emulated);
    let result = CapabilityNegotiator::negotiate(&r, &m);
    assert!(result.bonus.is_empty());
}

// ---------------------------------------------------------------------------
// best_match
// ---------------------------------------------------------------------------

#[test]
fn best_match_returns_none_when_none_compatible() {
    let m1 = manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]);
    let manifests: Vec<(&str, CapabilityManifest)> = vec![("a", m1)];
    let r = req(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
    assert!(CapabilityNegotiator::best_match(&r, &manifests).is_none());
}

#[test]
fn best_match_selects_highest_score() {
    let m1 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
    ]);
    let m2 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let manifests: Vec<(&str, CapabilityManifest)> =
        vec![("basic", m1), ("rich", m2.clone())];
    let r = req(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        SupportLevel::Emulated,
    );
    let (name, result) = CapabilityNegotiator::best_match(&r, &manifests).unwrap();
    assert_eq!(name, "rich");
    assert!(result.is_compatible);
    assert_eq!(result.bonus.len(), 1);
}

#[test]
fn best_match_empty_manifests() {
    let r = req(vec![], vec![], SupportLevel::Native);
    assert!(CapabilityNegotiator::best_match(&r, &[]).is_none());
}

#[test]
fn best_match_all_compatible_picks_best() {
    let m1 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let m2 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let manifests: Vec<(&str, CapabilityManifest)> =
        vec![("full", m1), ("partial", m2)];
    let r = req(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        SupportLevel::Emulated,
    );
    let (name, _) = CapabilityNegotiator::best_match(&r, &manifests).unwrap();
    assert_eq!(name, "full");
}

#[test]
fn best_match_tie_broken_deterministically() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let manifests: Vec<(&str, CapabilityManifest)> =
        vec![("alpha", m.clone()), ("beta", m)];
    let r = req(vec![Capability::Streaming], vec![], SupportLevel::Emulated);
    let (name, _) = CapabilityNegotiator::best_match(&r, &manifests).unwrap();
    // Deterministic — same result every call
    let (name2, _) = CapabilityNegotiator::best_match(&r, &manifests).unwrap();
    assert_eq!(name, name2);
}

// ---------------------------------------------------------------------------
// CapabilityDiff — added / removed
// ---------------------------------------------------------------------------

#[test]
fn diff_added_capabilities() {
    let old = manifest(&[]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.added, vec![Capability::Streaming]);
    assert!(d.removed.is_empty());
}

#[test]
fn diff_removed_capabilities() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new = manifest(&[]);
    let d = CapabilityDiff::diff(&old, &new);
    assert!(d.added.is_empty());
    assert_eq!(d.removed, vec![Capability::Streaming]);
}

#[test]
fn diff_no_change() {
    let m = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
    let d = CapabilityDiff::diff(&m, &m);
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.upgraded.is_empty());
    assert!(d.downgraded.is_empty());
}

// ---------------------------------------------------------------------------
// CapabilityDiff — upgrades / downgrades
// ---------------------------------------------------------------------------

#[test]
fn diff_upgrade_detected() {
    let old = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let new = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.upgraded.len(), 1);
    assert_eq!(d.upgraded[0].0, Capability::ToolRead);
}

#[test]
fn diff_downgrade_detected() {
    let old = manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
    let new = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.downgraded.len(), 1);
    assert_eq!(d.downgraded[0].0, Capability::ToolRead);
}

#[test]
fn diff_mixed_changes() {
    let old = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let new = manifest(&[
        (Capability::Streaming, SupportLevel::Emulated),  // downgrade
        (Capability::ToolRead, SupportLevel::Native),      // upgrade
        (Capability::McpClient, SupportLevel::Native),     // added
        // ToolWrite removed
    ]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.added, vec![Capability::McpClient]);
    assert_eq!(d.removed, vec![Capability::ToolWrite]);
    assert_eq!(d.upgraded.len(), 1);
    assert_eq!(d.upgraded[0].0, Capability::ToolRead);
    assert_eq!(d.downgraded.len(), 1);
    assert_eq!(d.downgraded[0].0, Capability::Streaming);
}

#[test]
fn diff_restricted_to_native_is_upgrade() {
    let old = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let new = manifest(&[(Capability::ToolBash, SupportLevel::Native)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.upgraded.len(), 1);
}

#[test]
fn diff_both_empty() {
    let d = CapabilityDiff::diff(&manifest(&[]), &manifest(&[]));
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.upgraded.is_empty());
    assert!(d.downgraded.is_empty());
}
