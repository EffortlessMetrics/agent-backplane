#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the pre-execution capability negotiation protocol.

use abp_capability::negotiate::{apply_policy, pre_negotiate, NegotiationError, NegotiationPolicy};
use abp_capability::NegotiationResult;
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};

fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

// -----------------------------------------------------------------------
// pre_negotiate tests
// -----------------------------------------------------------------------

#[test]
fn pre_negotiate_all_native_viable() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &m,
    );
    assert!(r.is_viable());
    assert_eq!(r.native.len(), 3);
    assert!(r.emulated.is_empty());
    assert!(r.unsupported.is_empty());
}

#[test]
fn pre_negotiate_all_unsupported_not_viable() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(!r.is_viable());
    assert_eq!(r.unsupported.len(), 2);
}

#[test]
fn pre_negotiate_mixed_native_emulated_unsupported() {
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
    assert!(!r.is_viable());
}

#[test]
fn pre_negotiate_empty_required_is_viable() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn pre_negotiate_empty_manifest_empty_required() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn pre_negotiate_restricted_counts_as_emulated() {
    let m = make_manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed only".into(),
        },
    )]);
    let r = pre_negotiate(&[Capability::ToolBash], &m);
    assert!(r.native.is_empty());
    assert_eq!(r.emulated.len(), 1);
    assert!(r.unsupported.is_empty());
    assert!(r.is_viable());
}

#[test]
fn pre_negotiate_explicit_unsupported_in_manifest() {
    let m = make_manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    let r = pre_negotiate(&[Capability::Vision], &m);
    assert_eq!(r.unsupported.len(), 1);
    assert!(!r.is_viable());
}

#[test]
fn pre_negotiate_preserves_order() {
    let m = make_manifest(&[
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(r.native[0], Capability::Streaming);
    assert_eq!(r.native[1], Capability::ToolUse);
}

#[test]
fn pre_negotiate_duplicate_required_caps() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Streaming], &m);
    assert_eq!(r.native.len(), 2);
    assert_eq!(r.total(), 2);
}

#[test]
fn pre_negotiate_large_manifest_small_requirements() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
        (Capability::Audio, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.total(), 1);
}

// -----------------------------------------------------------------------
// apply_policy tests
// -----------------------------------------------------------------------

#[test]
fn strict_policy_passes_all_native() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn strict_policy_passes_emulated() {
    let m = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn strict_policy_fails_on_unsupported() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
    assert_eq!(err.unsupported.len(), 1);
    assert_eq!(err.unsupported[0].0, Capability::Vision);
}

#[test]
fn strict_policy_passes_restricted() {
    let m = make_manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = pre_negotiate(&[Capability::ToolBash], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn best_effort_passes_all_supported() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
}

#[test]
fn best_effort_fails_on_unsupported() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Audio], &m);
    let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::BestEffort);
    assert_eq!(err.unsupported.len(), 1);
}

#[test]
fn permissive_passes_with_unsupported() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert!(!r.is_viable());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn permissive_passes_empty() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

// -----------------------------------------------------------------------
// NegotiationError tests
// -----------------------------------------------------------------------

#[test]
fn error_display_includes_policy_and_count() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![
            (Capability::Vision, "not available".into()),
            (Capability::Audio, "not available".into()),
        ],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
    assert!(msg.contains("2 unsupported"));
}

#[test]
fn error_display_lists_capability_names() {
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![(Capability::Streaming, "missing".into())],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("Streaming"));
}

// -----------------------------------------------------------------------
// NegotiationPolicy tests
// -----------------------------------------------------------------------

#[test]
fn policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn policy_display_variants() {
    assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
    assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
    assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
}

#[test]
fn policy_serde_roundtrip_all_variants() {
    for policy in [
        NegotiationPolicy::Strict,
        NegotiationPolicy::BestEffort,
        NegotiationPolicy::Permissive,
    ] {
        let json = serde_json::to_string(&policy).unwrap();
        let parsed: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }
}

// -----------------------------------------------------------------------
// End-to-end negotiation + policy workflows
// -----------------------------------------------------------------------

#[test]
fn full_workflow_strict_pass() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Emulated),
    ]);
    let required = &[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ];
    let result = pre_negotiate(required, &m);
    assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn full_workflow_strict_fail_then_permissive_pass() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let required = &[Capability::Streaming, Capability::Vision];
    let result = pre_negotiate(required, &m);

    // Strict fails
    assert!(apply_policy(&result, NegotiationPolicy::Strict).is_err());
    // Permissive passes
    assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn from_simple_roundtrip_with_policy() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Vision],
    );
    assert!(apply_policy(&result, NegotiationPolicy::Strict).is_err());
    assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
}
