// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for capability satisfaction checking and ensure_capability_requirements.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel,
};
use abp_integrations::ensure_capability_requirements;

fn reqs(items: Vec<(Capability, MinSupport)>) -> CapabilityRequirements {
    CapabilityRequirements {
        required: items
            .into_iter()
            .map(|(capability, min_support)| CapabilityRequirement {
                capability,
                min_support,
            })
            .collect(),
    }
}

fn caps(items: Vec<(Capability, SupportLevel)>) -> CapabilityManifest {
    items.into_iter().collect()
}

// ---------------------------------------------------------------------------
// 1. Empty requirements always satisfied (even with empty capabilities)
// ---------------------------------------------------------------------------

#[test]
fn empty_requirements_empty_capabilities() {
    let r = CapabilityRequirements::default();
    let c = CapabilityManifest::default();
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 2. Empty requirements satisfied by non-empty capabilities
// ---------------------------------------------------------------------------

#[test]
fn empty_requirements_nonempty_capabilities() {
    let r = CapabilityRequirements::default();
    let c = caps(vec![(Capability::Streaming, SupportLevel::Native)]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 3. Missing capability fails
// ---------------------------------------------------------------------------

#[test]
fn missing_capability_fails() {
    let r = reqs(vec![(Capability::Streaming, MinSupport::Emulated)]);
    let c = CapabilityManifest::default();
    let err = ensure_capability_requirements(&r, &c).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsatisfied"), "error: {msg}");
}

// ---------------------------------------------------------------------------
// 4. Native satisfies Native requirement
// ---------------------------------------------------------------------------

#[test]
fn native_satisfies_native() {
    let r = reqs(vec![(Capability::Streaming, MinSupport::Native)]);
    let c = caps(vec![(Capability::Streaming, SupportLevel::Native)]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 5. Emulated does NOT satisfy Native requirement
// ---------------------------------------------------------------------------

#[test]
fn emulated_does_not_satisfy_native() {
    let r = reqs(vec![(Capability::Streaming, MinSupport::Native)]);
    let c = caps(vec![(Capability::Streaming, SupportLevel::Emulated)]);
    assert!(ensure_capability_requirements(&r, &c).is_err());
}

// ---------------------------------------------------------------------------
// 6. Native satisfies Emulated requirement
// ---------------------------------------------------------------------------

#[test]
fn native_satisfies_emulated() {
    let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
    let c = caps(vec![(Capability::ToolRead, SupportLevel::Native)]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 7. Emulated satisfies Emulated requirement
// ---------------------------------------------------------------------------

#[test]
fn emulated_satisfies_emulated() {
    let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
    let c = caps(vec![(Capability::ToolRead, SupportLevel::Emulated)]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 8. Unsupported does NOT satisfy Emulated requirement
// ---------------------------------------------------------------------------

#[test]
fn unsupported_fails_emulated() {
    let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
    let c = caps(vec![(Capability::ToolRead, SupportLevel::Unsupported)]);
    assert!(ensure_capability_requirements(&r, &c).is_err());
}

// ---------------------------------------------------------------------------
// 9. Restricted satisfies Emulated requirement
// ---------------------------------------------------------------------------

#[test]
fn restricted_satisfies_emulated() {
    let r = reqs(vec![(Capability::ToolBash, MinSupport::Emulated)]);
    let c = caps(vec![(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 10. Restricted does NOT satisfy Native requirement
// ---------------------------------------------------------------------------

#[test]
fn restricted_does_not_satisfy_native() {
    let r = reqs(vec![(Capability::ToolBash, MinSupport::Native)]);
    let c = caps(vec![(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    assert!(ensure_capability_requirements(&r, &c).is_err());
}

// ---------------------------------------------------------------------------
// 11. Multiple requirements — all satisfied
// ---------------------------------------------------------------------------

#[test]
fn multiple_requirements_all_satisfied() {
    let r = reqs(vec![
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
        (Capability::ToolWrite, MinSupport::Emulated),
    ]);
    let c = caps(vec![
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
    ]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 12. Multiple requirements — partial failure
// ---------------------------------------------------------------------------

#[test]
fn multiple_requirements_partial_failure() {
    let r = reqs(vec![
        (Capability::Streaming, MinSupport::Native),
        (Capability::McpClient, MinSupport::Emulated), // not present
    ]);
    let c = caps(vec![(Capability::Streaming, SupportLevel::Native)]);
    let err = ensure_capability_requirements(&r, &c).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("McpClient"),
        "should mention missing cap: {msg}"
    );
    // Streaming should NOT appear since it's satisfied
    assert!(
        !msg.contains("Streaming"),
        "should not mention satisfied cap: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 13. Error message includes actual level when present but insufficient
// ---------------------------------------------------------------------------

#[test]
fn error_message_shows_actual_level() {
    let r = reqs(vec![(Capability::Streaming, MinSupport::Native)]);
    let c = caps(vec![(Capability::Streaming, SupportLevel::Emulated)]);
    let err = ensure_capability_requirements(&r, &c).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Emulated"), "should show actual level: {msg}");
}

// ---------------------------------------------------------------------------
// 14. Error message shows "missing" for absent capability
// ---------------------------------------------------------------------------

#[test]
fn error_message_shows_missing() {
    let r = reqs(vec![(Capability::McpServer, MinSupport::Emulated)]);
    let c = CapabilityManifest::default();
    let err = ensure_capability_requirements(&r, &c).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("missing"), "should say 'missing': {msg}");
}

// ---------------------------------------------------------------------------
// 15. Superset capabilities still satisfy subset requirements
// ---------------------------------------------------------------------------

#[test]
fn superset_capabilities_satisfy() {
    let r = reqs(vec![(Capability::ToolRead, MinSupport::Emulated)]);
    let c = caps(vec![
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (Capability::ToolEdit, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Emulated),
    ]);
    assert!(ensure_capability_requirements(&r, &c).is_ok());
}

// ---------------------------------------------------------------------------
// 16. Unsupported does NOT satisfy Native requirement
// ---------------------------------------------------------------------------

#[test]
fn unsupported_fails_native() {
    let r = reqs(vec![(Capability::SessionResume, MinSupport::Native)]);
    let c = caps(vec![(Capability::SessionResume, SupportLevel::Unsupported)]);
    assert!(ensure_capability_requirements(&r, &c).is_err());
}
