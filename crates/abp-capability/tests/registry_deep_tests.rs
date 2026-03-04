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
//! Deep tests for the capability registry and negotiation system.

use std::collections::BTreeMap;

use abp_capability::negotiate::{NegotiationError, NegotiationPolicy, apply_policy, pre_negotiate};
use abp_capability::{
    CapabilityRegistry, CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel,
    check_capability, default_emulation_strategy, generate_report, negotiate,
    negotiate_capabilities,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};
use abp_sdk_types::Dialect;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

/// Map a [`Dialect`] to the registry key used by `CapabilityRegistry::with_defaults`.
fn dialect_registry_key(d: Dialect) -> &'static str {
    match d {
        Dialect::OpenAi => "openai/gpt-4o",
        Dialect::Claude => "anthropic/claude-3.5-sonnet",
        Dialect::Gemini => "google/gemini-1.5-pro",
        Dialect::Kimi => "moonshot/kimi",
        Dialect::Codex => "openai/codex",
        Dialect::Copilot => "github/copilot",
    }
}

// ===========================================================================
// 1. CapabilityRegistry construction
// ===========================================================================

#[test]
fn registry_default_is_empty() {
    let reg = CapabilityRegistry::default();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.names().is_empty());
}

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
}

#[test]
fn registry_with_defaults_has_all_dialects() {
    let reg = CapabilityRegistry::with_defaults();
    for d in Dialect::all() {
        let key = dialect_registry_key(*d);
        assert!(reg.contains(key), "missing dialect key: {key}");
    }
    assert_eq!(reg.len(), 6);
}

#[test]
fn registry_pre_populated_manifests_non_empty() {
    let reg = CapabilityRegistry::with_defaults();
    for name in reg.names() {
        let m = reg.get(name).unwrap();
        assert!(!m.is_empty(), "manifest for {name} should not be empty");
    }
}

// ===========================================================================
// 2. Capability registration
// ===========================================================================

#[test]
fn register_single_capability_manifest() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("custom", m);
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("custom"));
}

#[test]
fn register_multiple_manifests() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "a",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    reg.register(
        "b",
        manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]),
    );
    assert_eq!(reg.len(), 2);
    let mut names = reg.names();
    names.sort();
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn register_for_dialect_key() {
    let mut reg = CapabilityRegistry::new();
    for d in Dialect::all() {
        reg.register(dialect_registry_key(*d), BTreeMap::new());
    }
    assert_eq!(reg.len(), 6);
}

#[test]
fn register_overwrites_previous() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "x",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    reg.register(
        "x",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]),
    );
    assert_eq!(reg.len(), 1);
    let m = reg.get("x").unwrap();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(CoreSupportLevel::Emulated)
    ));
}

#[test]
fn unregister_returns_true_when_exists() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", BTreeMap::new());
    assert!(reg.unregister("a"));
    assert!(!reg.contains("a"));
}

#[test]
fn unregister_returns_false_when_missing() {
    let mut reg = CapabilityRegistry::new();
    assert!(!reg.unregister("nonexistent"));
}

// ===========================================================================
// 3. Capability lookup
// ===========================================================================

#[test]
fn lookup_by_name_returns_manifest() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
    ]);
    reg.register("backend", m);
    let got = reg.get("backend").unwrap();
    assert_eq!(got.len(), 2);
}

#[test]
fn lookup_by_name_missing_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn lookup_by_dialect_via_registry() {
    let reg = CapabilityRegistry::with_defaults();
    for d in Dialect::all() {
        let key = dialect_registry_key(*d);
        assert!(reg.get(key).is_some(), "missing manifest for {}", d.label());
    }
}

#[test]
fn query_capability_by_support_level() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::ExtendedThinking);
    let native_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .count();
    let unsupported_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Unsupported { .. }))
        .count();
    // Only Claude supports ExtendedThinking natively
    assert_eq!(native_count, 1);
    assert!(unsupported_count >= 4);
}

#[test]
fn query_capability_streaming_all_native() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert!(
        results
            .iter()
            .all(|(_, l)| matches!(l, SupportLevel::Native))
    );
}

// ===========================================================================
// 4. Support levels — Native, Emulated, Unsupported (+ Restricted)
// ===========================================================================

#[test]
fn check_capability_native_variant() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_capability_emulated_variant() {
    let m = manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    assert!(matches!(
        check_capability(&m, &Capability::Vision),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_capability_unsupported_explicit() {
    let m = manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let level = check_capability(&m, &Capability::Audio);
    if let SupportLevel::Unsupported { reason } = &level {
        assert!(reason.contains("explicitly"));
    } else {
        panic!("expected Unsupported, got {level:?}");
    }
}

#[test]
fn check_capability_unsupported_missing() {
    let m: CapabilityManifest = BTreeMap::new();
    let level = check_capability(&m, &Capability::Streaming);
    if let SupportLevel::Unsupported { reason } = &level {
        assert!(reason.contains("not declared"));
    } else {
        panic!("expected Unsupported, got {level:?}");
    }
}

#[test]
fn check_capability_restricted_variant() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    if let SupportLevel::Restricted { reason } = &level {
        assert!(reason.contains("sandboxed"));
    } else {
        panic!("expected Restricted, got {level:?}");
    }
}

#[test]
fn support_level_display_all_variants() {
    assert_eq!(SupportLevel::Native.to_string(), "native");
    assert!(
        SupportLevel::Emulated { method: "x".into() }
            .to_string()
            .contains("emulated")
    );
    assert!(
        SupportLevel::Restricted { reason: "y".into() }
            .to_string()
            .contains("restricted")
    );
    assert!(
        SupportLevel::Unsupported { reason: "z".into() }
            .to_string()
            .contains("unsupported")
    );
}

// ===========================================================================
// 5. Capability matching — requirements against available capabilities
// ===========================================================================

#[test]
fn negotiate_capabilities_all_native() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(r.native.len(), 2);
    assert!(r.is_viable());
}

#[test]
fn negotiate_capabilities_mixed_classification() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
    ]);
    let r = negotiate_capabilities(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.unsupported.len(), 1);
    assert!(!r.is_viable());
}

#[test]
fn negotiate_by_name_returns_result() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
        .unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_by_name_missing_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(
        reg.negotiate_by_name("missing", &[Capability::Streaming])
            .is_none()
    );
}

#[test]
fn negotiate_requirements_struct() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let r = negotiate(&m, &reqs);
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.emulated.len(), 1);
    assert!(r.is_compatible());
}

#[test]
fn negotiate_emulated_gets_default_strategy() {
    let m = manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let r = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(r.emulated[0].1, EmulationStrategy::Approximate);
}

#[test]
fn negotiate_restricted_classified_as_emulated() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert!(r.native.is_empty());
    assert_eq!(r.emulated.len(), 1);
    assert!(r.is_viable());
}

// ===========================================================================
// 6. Negotiation — pre_negotiate() with policies
// ===========================================================================

#[test]
fn pre_negotiate_strict_all_native_passes() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn pre_negotiate_strict_emulated_passes() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn pre_negotiate_strict_unsupported_fails() {
    let m = manifest(&[]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
    assert_eq!(err.unsupported.len(), 1);
}

#[test]
fn pre_negotiate_best_effort_passes_when_all_supported() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
}

#[test]
fn pre_negotiate_best_effort_fails_on_unsupported() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Audio], &m);
    let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::BestEffort);
}

#[test]
fn pre_negotiate_permissive_always_ok() {
    let m = manifest(&[]);
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert!(!r.is_viable());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn pre_negotiate_permissive_empty() {
    let m = manifest(&[]);
    let r = pre_negotiate(&[], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

// ===========================================================================
// 7. apply_policy() — policy filtering/transformation
// ===========================================================================

#[test]
fn apply_policy_strict_rejects_any_unsupported() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::Vision],
    );
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_err());
}

#[test]
fn apply_policy_strict_accepts_emulated_only() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::Streaming], vec![]);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn apply_policy_best_effort_rejects_unsupported() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![Capability::Streaming],
        vec![Capability::Audio],
    );
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_err());
}

#[test]
fn apply_policy_permissive_accepts_everything() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![],
        vec![Capability::Streaming, Capability::Vision, Capability::Audio],
    );
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn apply_policy_error_contains_unsupported_list() {
    let r =
        NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision, Capability::Audio]);
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.unsupported.len(), 2);
    assert_eq!(err.unsupported[0].0, Capability::Vision);
    assert_eq!(err.unsupported[1].0, Capability::Audio);
}

#[test]
fn apply_policy_cascading_strict_then_permissive() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_err());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

// ===========================================================================
// 8. Serde roundtrip
// ===========================================================================

#[test]
fn serde_support_level_all_variants() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated {
            method: "polyfill".into(),
        },
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
        SupportLevel::Unsupported {
            reason: "n/a".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, level);
    }
}

#[test]
fn serde_emulation_strategy_all_variants() {
    for s in [
        EmulationStrategy::ClientSide,
        EmulationStrategy::ServerFallback,
        EmulationStrategy::Approximate,
    ] {
        let json = serde_json::to_string(&s).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}

#[test]
fn serde_negotiation_result_roundtrip() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![
            (Capability::Vision, EmulationStrategy::Approximate),
            (Capability::ToolRead, EmulationStrategy::ClientSide),
        ],
        unsupported: vec![(Capability::Audio, "not available".into())],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, result);
}

#[test]
fn serde_negotiation_policy_all_variants() {
    for policy in [
        NegotiationPolicy::Strict,
        NegotiationPolicy::BestEffort,
        NegotiationPolicy::Permissive,
    ] {
        let json = serde_json::to_string(&policy).unwrap();
        let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}

#[test]
fn serde_compatibility_report_roundtrip() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn serde_negotiation_policy_snake_case_format() {
    let json = serde_json::to_string(&NegotiationPolicy::BestEffort).unwrap();
    assert_eq!(json, "\"best_effort\"");
    let json = serde_json::to_string(&NegotiationPolicy::Strict).unwrap();
    assert_eq!(json, "\"strict\"");
}

// ===========================================================================
// 9. Edge cases
// ===========================================================================

#[test]
fn empty_registry_negotiate_by_name_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(
        reg.negotiate_by_name("anything", &[Capability::Streaming])
            .is_none()
    );
}

#[test]
fn empty_registry_query_capability_returns_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.query_capability(&Capability::Streaming).is_empty());
}

#[test]
fn empty_registry_compare_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(reg.compare("a", "b").is_none());
}

#[test]
fn duplicate_registration_replaces_manifest() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "x",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    reg.register(
        "x",
        manifest(&[(Capability::Vision, CoreSupportLevel::Native)]),
    );
    let m = reg.get("x").unwrap();
    assert!(!m.contains_key(&Capability::Streaming));
    assert!(m.contains_key(&Capability::Vision));
}

#[test]
fn negotiate_duplicate_required_capabilities() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Streaming], &m);
    assert_eq!(r.native.len(), 2);
    assert_eq!(r.total(), 2);
}

#[test]
fn negotiate_unknown_capability_is_unsupported() {
    // A capability not in the manifest is treated as unsupported
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[Capability::Checkpointing], &m);
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn negotiate_empty_required_empty_manifest() {
    let r = negotiate_capabilities(&[], &BTreeMap::new());
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiation_result_from_simple_uses_client_side_strategy() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::ToolRead], vec![]);
    assert_eq!(r.emulated[0].1, EmulationStrategy::ClientSide);
}

#[test]
fn negotiation_result_from_simple_uses_not_available_reason() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    assert_eq!(r.unsupported[0].1, "not available");
}

#[test]
fn negotiation_result_warnings_only_approximate() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::ToolUse, EmulationStrategy::ServerFallback),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let warnings = r.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_display_shows_viable() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let s = r.to_string();
    assert!(s.contains("viable"));
    assert!(!s.contains("not viable"));
}

#[test]
fn negotiation_result_display_shows_not_viable() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    let s = r.to_string();
    assert!(s.contains("not viable"));
}

#[test]
fn negotiation_error_display_format() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![
            (Capability::Vision, "missing".into()),
            (Capability::Audio, "missing".into()),
        ],
        warnings: vec![Capability::ToolRead],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
    assert!(msg.contains("2 unsupported"));
    assert!(msg.contains("Vision"));
    assert!(msg.contains("Audio"));
}

#[test]
fn negotiation_error_implements_std_error() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Streaming, "missing".into())],
        warnings: vec![],
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn negotiation_policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn negotiation_policy_display() {
    assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
    assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
    assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
}

// ===========================================================================
// 10. Cross-dialect capability comparison
// ===========================================================================

#[test]
fn cross_dialect_streaming_universally_native() {
    let reg = CapabilityRegistry::with_defaults();
    for d in Dialect::all() {
        let key = dialect_registry_key(*d);
        let r = reg
            .negotiate_by_name(key, &[Capability::Streaming])
            .unwrap();
        assert!(
            r.is_viable(),
            "{} should support streaming natively",
            d.label()
        );
        assert_eq!(r.native.len(), 1);
    }
}

#[test]
fn cross_dialect_extended_thinking_only_claude() {
    let reg = CapabilityRegistry::with_defaults();
    for d in Dialect::all() {
        let key = dialect_registry_key(*d);
        let r = reg
            .negotiate_by_name(key, &[Capability::ExtendedThinking])
            .unwrap();
        if *d == Dialect::Claude {
            assert!(r.is_viable(), "Claude should support ExtendedThinking");
        } else {
            assert!(
                !r.is_viable(),
                "{} should not support ExtendedThinking natively",
                d.label()
            );
        }
    }
}

#[test]
fn cross_dialect_compare_claude_to_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
        .unwrap();
    // Claude has ExtendedThinking and CacheControl natively; OpenAI lacks them
    assert!(
        result
            .unsupported_caps()
            .contains(&Capability::ExtendedThinking)
    );
}

#[test]
fn cross_dialect_compare_openai_to_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
        .unwrap();
    // OpenAI has Audio natively; Claude marks it unsupported
    assert!(result.unsupported_caps().contains(&Capability::Audio));
}

#[test]
fn cross_dialect_compare_same_backend_is_viable() {
    let reg = CapabilityRegistry::with_defaults();
    for d in Dialect::all() {
        let key = dialect_registry_key(*d);
        let result = reg.compare(key, key).unwrap();
        assert!(
            result.is_viable(),
            "{} compared to itself should be viable",
            d.label()
        );
    }
}

#[test]
fn cross_dialect_codex_copilot_tool_parity() {
    let reg = CapabilityRegistry::with_defaults();
    let tools = [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ];
    let codex = reg.negotiate_by_name("openai/codex", &tools).unwrap();
    let copilot = reg.negotiate_by_name("github/copilot", &tools).unwrap();
    assert!(codex.is_viable());
    assert!(copilot.is_viable());
}

#[test]
fn cross_dialect_audio_support_varies() {
    let reg = CapabilityRegistry::with_defaults();
    let has_audio: Vec<&str> = Dialect::all()
        .iter()
        .filter_map(|d| {
            let key = dialect_registry_key(*d);
            let r = reg.negotiate_by_name(key, &[Capability::Audio]).unwrap();
            if r.is_viable() { Some(d.label()) } else { None }
        })
        .collect();
    // At minimum OpenAI and Gemini support Audio
    assert!(has_audio.contains(&"OpenAI"));
    assert!(has_audio.contains(&"Gemini"));
    assert!(!has_audio.contains(&"Claude"));
}

// ===========================================================================
// Extra: generate_report integration
// ===========================================================================

#[test]
fn generate_report_compatible_summary() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![Capability::Vision],
        vec![],
    );
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_incompatible_summary() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::Audio],
    );
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn generate_report_details_count_matches() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![(Capability::Audio, "missing".into())],
    };
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 3);
}

// ===========================================================================
// Extra: default_emulation_strategy coverage
// ===========================================================================

#[test]
fn default_emulation_strategy_client_side() {
    assert_eq!(
        default_emulation_strategy(&Capability::ToolRead),
        EmulationStrategy::ClientSide
    );
    assert_eq!(
        default_emulation_strategy(&Capability::Checkpointing),
        EmulationStrategy::ClientSide
    );
}

#[test]
fn default_emulation_strategy_server_fallback() {
    assert_eq!(
        default_emulation_strategy(&Capability::FunctionCalling),
        EmulationStrategy::ServerFallback
    );
    assert_eq!(
        default_emulation_strategy(&Capability::McpClient),
        EmulationStrategy::ServerFallback
    );
}

#[test]
fn default_emulation_strategy_approximate() {
    assert_eq!(
        default_emulation_strategy(&Capability::Vision),
        EmulationStrategy::Approximate
    );
    assert_eq!(
        default_emulation_strategy(&Capability::Streaming),
        EmulationStrategy::Approximate
    );
}
