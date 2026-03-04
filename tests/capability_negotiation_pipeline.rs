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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Deep capability registry tests verifying the full capability negotiation
//! pipeline: registry operations, manifest merging, negotiation algorithm,
//! support-level transitions, emulation planning, error catalog, backend
//! selection, feature flags, version compatibility, concurrent negotiation,
//! caching, and report generation.

use std::sync::{Arc, Mutex};
use std::thread;

use abp_capability::negotiate::{NegotiationError, NegotiationPolicy, apply_policy, pre_negotiate};
use abp_capability::{
    CapabilityRegistry, CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel,
    check_capability, generate_report, negotiate, negotiate_capabilities,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Native))
            .collect::<Vec<_>>(),
    )
}

// ===========================================================================
// 1. Registry operations
// ===========================================================================

mod registry_operations {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = CapabilityRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn register_and_retrieve() {
        let mut reg = CapabilityRegistry::new();
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("test-backend", m.clone());
        assert!(reg.contains("test-backend"));
        let got = reg.get("test-backend").unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn unregister_returns_true_when_present() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "a",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        assert!(reg.unregister("a"));
        assert!(!reg.contains("a"));
    }

    #[test]
    fn unregister_returns_false_when_absent() {
        let mut reg = CapabilityRegistry::new();
        assert!(!reg.unregister("nonexistent"));
    }

    #[test]
    fn query_capability_across_all_backends() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "alpha",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        reg.register(
            "beta",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]),
        );
        reg.register(
            "gamma",
            manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]),
        );
        let results = reg.query_capability(&Capability::Streaming);
        assert_eq!(results.len(), 3);
        let alpha = results.iter().find(|(n, _)| *n == "alpha").unwrap();
        assert!(matches!(alpha.1, SupportLevel::Native));
        let beta = results.iter().find(|(n, _)| *n == "beta").unwrap();
        assert!(matches!(beta.1, SupportLevel::Emulated { .. }));
        let gamma = results.iter().find(|(n, _)| *n == "gamma").unwrap();
        assert!(matches!(gamma.1, SupportLevel::Unsupported { .. }));
    }

    #[test]
    fn names_returns_all_registered() {
        let mut reg = CapabilityRegistry::new();
        reg.register("x", CapabilityManifest::new());
        reg.register("y", CapabilityManifest::new());
        let names = reg.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"x"));
        assert!(names.contains(&"y"));
    }

    #[test]
    fn register_overwrites_existing() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "b",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        reg.register(
            "b",
            manifest(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]),
        );
        assert_eq!(reg.len(), 1);
        let m = reg.get("b").unwrap();
        assert!(!m.contains_key(&Capability::Streaming));
        assert!(m.contains_key(&Capability::ToolRead));
    }

    #[test]
    fn with_defaults_has_six_entries() {
        let reg = CapabilityRegistry::with_defaults();
        assert_eq!(reg.len(), 6);
        assert!(reg.contains("openai/gpt-4o"));
        assert!(reg.contains("anthropic/claude-3.5-sonnet"));
        assert!(reg.contains("google/gemini-1.5-pro"));
        assert!(reg.contains("moonshot/kimi"));
        assert!(reg.contains("openai/codex"));
        assert!(reg.contains("github/copilot"));
    }

    #[test]
    fn negotiate_by_name_returns_none_for_unknown() {
        let reg = CapabilityRegistry::new();
        assert!(
            reg.negotiate_by_name("ghost", &[Capability::Streaming])
                .is_none()
        );
    }

    #[test]
    fn negotiate_by_name_returns_result_for_known() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "test",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        let result = reg
            .negotiate_by_name("test", &[Capability::Streaming])
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.native, vec![Capability::Streaming]);
    }
}

// ===========================================================================
// 2. Manifest merging
// ===========================================================================

mod manifest_merging {
    use super::*;

    fn merge_manifests(
        base: &CapabilityManifest,
        overlay: &CapabilityManifest,
    ) -> CapabilityManifest {
        let mut merged = base.clone();
        for (cap, level) in overlay {
            merged.insert(cap.clone(), level.clone());
        }
        merged
    }

    #[test]
    fn merge_disjoint_manifests() {
        let a = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&a, &b);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_overlay_overrides_base() {
        let base = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let overlay = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&base, &overlay);
        assert!(matches!(
            merged.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn merge_empty_base_equals_overlay() {
        let overlay = manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&CapabilityManifest::new(), &overlay);
        assert_eq!(merged.len(), overlay.len());
    }

    #[test]
    fn merge_empty_overlay_preserves_base() {
        let base = manifest(&[(Capability::ToolBash, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&base, &CapabilityManifest::new());
        assert_eq!(merged.len(), base.len());
    }

    #[test]
    fn merge_multiple_sources_sequential() {
        let a = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let b = manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let c = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&merge_manifests(&a, &b), &c);
        assert_eq!(merged.len(), 2);
        assert!(matches!(
            merged.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }
}

// ===========================================================================
// 3. Negotiation algorithm — detailed step-by-step
// ===========================================================================

mod negotiation_algorithm {
    use super::*;

    #[test]
    fn all_native_produces_viable_result() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 2);
        assert!(result.emulated.is_empty());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn emulated_cap_goes_to_emulated_bucket() {
        let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        let result = negotiate_capabilities(&[Capability::ToolRead], &m);
        assert!(result.is_viable());
        assert!(result.native.is_empty());
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.emulated[0].0, Capability::ToolRead);
    }

    #[test]
    fn missing_cap_goes_to_unsupported() {
        let m = manifest(&[]);
        let result = negotiate_capabilities(&[Capability::Vision], &m);
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn explicitly_unsupported_goes_to_unsupported() {
        let m = manifest(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
        let result = negotiate_capabilities(&[Capability::Logprobs], &m);
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn restricted_classified_as_emulated() {
        let m = manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let result = negotiate_capabilities(&[Capability::ToolBash], &m);
        assert!(result.is_viable());
        assert_eq!(result.emulated.len(), 1);
    }

    #[test]
    fn mixed_native_emulated_unsupported() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &m,
        );
        assert!(!result.is_viable());
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn total_counts_all_categories() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::Vision,
            ],
            &m,
        );
        assert_eq!(result.total(), 3);
    }

    #[test]
    fn empty_requirements_always_viable() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = negotiate_capabilities(&[], &m);
        assert!(result.is_viable());
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn negotiate_with_min_support_native_rejects_emulated() {
        let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        let reqs = require_native(&[Capability::ToolRead]);
        let result = negotiate(&m, &reqs);
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn negotiate_with_min_support_emulated_accepts_native() {
        let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let reqs = require_emulated(&[Capability::ToolRead]);
        let result = negotiate(&m, &reqs);
        assert!(result.is_viable());
        assert_eq!(result.native, vec![Capability::ToolRead]);
    }
}

// ===========================================================================
// 4. Support level transitions
// ===========================================================================

mod support_level_transitions {
    use super::*;

    #[test]
    fn native_satisfies_native_min() {
        assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated_min() {
        assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_does_not_satisfy_native_min() {
        assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn emulated_satisfies_emulated_min() {
        assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_satisfies_emulated_min() {
        let r = CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        };
        assert!(r.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_does_not_satisfy_native_min() {
        let r = CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        };
        assert!(!r.satisfies(&MinSupport::Native));
    }

    #[test]
    fn unsupported_satisfies_nothing() {
        assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
        assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn upgrade_emulated_to_native_changes_negotiation() {
        let emulated_m = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let native_m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);

        let reqs = require_native(&[Capability::Streaming]);
        let r1 = negotiate(&emulated_m, &reqs);
        let r2 = negotiate(&native_m, &reqs);
        assert!(!r1.is_viable());
        assert!(r2.is_viable());
    }

    #[test]
    fn downgrade_native_to_unsupported_breaks_negotiation() {
        let native_m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let unsupported_m = manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]);

        let reqs = require_emulated(&[Capability::Streaming]);
        let r1 = negotiate(&native_m, &reqs);
        let r2 = negotiate(&unsupported_m, &reqs);
        assert!(r1.is_viable());
        assert!(!r2.is_viable());
    }
}

// ===========================================================================
// 5. Emulation planning
// ===========================================================================

mod emulation_planning {
    use super::*;
    use abp_capability::default_emulation_strategy;

    #[test]
    fn tool_read_uses_client_side_emulation() {
        assert_eq!(
            default_emulation_strategy(&Capability::ToolRead),
            EmulationStrategy::ClientSide
        );
    }

    #[test]
    fn function_calling_uses_server_fallback() {
        assert_eq!(
            default_emulation_strategy(&Capability::FunctionCalling),
            EmulationStrategy::ServerFallback
        );
    }

    #[test]
    fn vision_uses_approximate() {
        assert_eq!(
            default_emulation_strategy(&Capability::Vision),
            EmulationStrategy::Approximate
        );
    }

    #[test]
    fn emulated_capability_includes_strategy() {
        let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        let result = negotiate_capabilities(&[Capability::ToolRead], &m);
        assert_eq!(result.emulated.len(), 1);
        let (cap, strategy) = &result.emulated[0];
        assert_eq!(cap, &Capability::ToolRead);
        assert_eq!(strategy, &EmulationStrategy::ClientSide);
    }

    #[test]
    fn emulation_plan_from_multiple_capabilities() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolEdit, CoreSupportLevel::Emulated),
            (Capability::Vision, CoreSupportLevel::Emulated),
        ]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolEdit,
                Capability::Vision,
            ],
            &m,
        );
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.emulated.len(), 2);
        let strategies: Vec<_> = result.emulated.iter().map(|(_, s)| s.clone()).collect();
        assert!(strategies.contains(&EmulationStrategy::ClientSide));
        assert!(strategies.contains(&EmulationStrategy::Approximate));
    }

    #[test]
    fn client_side_has_no_fidelity_loss() {
        assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
    }

    #[test]
    fn approximate_has_fidelity_loss() {
        assert!(EmulationStrategy::Approximate.has_fidelity_loss());
    }

    #[test]
    fn warnings_only_include_approximate_strategies() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![
                (Capability::ToolRead, EmulationStrategy::ClientSide),
                (Capability::Vision, EmulationStrategy::Approximate),
            ],
            unsupported: vec![],
        };
        let warnings = result.warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, Capability::Vision);
    }

    #[test]
    fn from_simple_assigns_client_side_to_emulated() {
        let result = NegotiationResult::from_simple(
            vec![],
            vec![Capability::ToolRead, Capability::ToolWrite],
            vec![],
        );
        for (_, strategy) in &result.emulated {
            assert_eq!(strategy, &EmulationStrategy::ClientSide);
        }
    }
}

// ===========================================================================
// 6. Error catalog — all negotiation error scenarios
// ===========================================================================

mod error_catalog {
    use super::*;

    #[test]
    fn strict_policy_fails_on_unsupported() {
        let m = manifest(&[]);
        let result = pre_negotiate(&[Capability::Streaming], &m);
        let err = apply_policy(&result, NegotiationPolicy::Strict).unwrap_err();
        assert_eq!(err.policy, NegotiationPolicy::Strict);
        assert_eq!(err.unsupported.len(), 1);
    }

    #[test]
    fn best_effort_policy_fails_on_unsupported() {
        let m = manifest(&[]);
        let result = pre_negotiate(&[Capability::Vision], &m);
        let err = apply_policy(&result, NegotiationPolicy::BestEffort).unwrap_err();
        assert_eq!(err.policy, NegotiationPolicy::BestEffort);
    }

    #[test]
    fn permissive_policy_never_fails() {
        let m = manifest(&[]);
        let result = pre_negotiate(
            &[Capability::Streaming, Capability::Vision, Capability::Audio],
            &m,
        );
        assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn strict_passes_when_all_native() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = pre_negotiate(&[Capability::Streaming], &m);
        assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn strict_passes_when_emulated() {
        let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
        let result = pre_negotiate(&[Capability::ToolUse], &m);
        assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn error_display_contains_policy() {
        let err = NegotiationError {
            policy: NegotiationPolicy::Strict,
            unsupported: vec![(Capability::Vision, "not available".into())],
            warnings: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("strict"));
        assert!(msg.contains("1 unsupported"));
    }

    #[test]
    fn error_display_lists_capabilities() {
        let err = NegotiationError {
            policy: NegotiationPolicy::BestEffort,
            unsupported: vec![
                (Capability::Vision, "n/a".into()),
                (Capability::Audio, "n/a".into()),
            ],
            warnings: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("Vision"));
        assert!(msg.contains("Audio"));
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
    fn negotiate_missing_capability_reason_string() {
        let m = manifest(&[]);
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert_eq!(result.unsupported.len(), 1);
        let (_, reason) = &result.unsupported[0];
        assert!(reason.contains("not declared"));
    }

    #[test]
    fn negotiate_explicit_unsupported_reason_string() {
        let m = manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
        let result = negotiate_capabilities(&[Capability::Vision], &m);
        assert_eq!(result.unsupported.len(), 1);
        let (_, reason) = &result.unsupported[0];
        assert!(reason.contains("explicitly marked unsupported"));
    }
}

// ===========================================================================
// 7. Backend selection
// ===========================================================================

mod backend_selection {
    use super::*;

    #[test]
    fn compare_two_backends_finds_gaps() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "source",
            manifest(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolRead, CoreSupportLevel::Native),
                (Capability::Vision, CoreSupportLevel::Native),
            ]),
        );
        reg.register(
            "target",
            manifest(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolRead, CoreSupportLevel::Emulated),
            ]),
        );
        let result = reg.compare("source", "target").unwrap();
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn compare_returns_none_for_unknown_source() {
        let mut reg = CapabilityRegistry::new();
        reg.register("target", CapabilityManifest::new());
        assert!(reg.compare("unknown", "target").is_none());
    }

    #[test]
    fn compare_returns_none_for_unknown_target() {
        let mut reg = CapabilityRegistry::new();
        reg.register("source", CapabilityManifest::new());
        assert!(reg.compare("source", "unknown").is_none());
    }

    #[test]
    fn best_backend_selection_via_negotiation() {
        let reg = CapabilityRegistry::with_defaults();
        let required = &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ];

        let mut best: Option<(&str, usize)> = None;
        for name in reg.names() {
            let result = reg.negotiate_by_name(name, required).unwrap();
            if result.is_viable() {
                let score = result.native.len();
                if best.is_none() || score > best.unwrap().1 {
                    best = Some((name, score));
                }
            }
        }
        assert!(best.is_some());
    }

    #[test]
    fn codex_supports_all_tool_caps_natively() {
        let reg = CapabilityRegistry::with_defaults();
        let tool_caps = vec![
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
            Capability::ToolGlob,
            Capability::ToolGrep,
        ];
        let result = reg.negotiate_by_name("openai/codex", &tool_caps).unwrap();
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 6);
    }

    #[test]
    fn copilot_supports_web_tools_natively() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg
            .negotiate_by_name(
                "github/copilot",
                &[Capability::ToolWebSearch, Capability::ToolWebFetch],
            )
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 2);
    }

    #[test]
    fn claude_extended_thinking_native() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg
            .negotiate_by_name(
                "anthropic/claude-3.5-sonnet",
                &[Capability::ExtendedThinking],
            )
            .unwrap();
        assert_eq!(result.native, vec![Capability::ExtendedThinking]);
    }

    #[test]
    fn openai_logprobs_native() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg
            .negotiate_by_name("openai/gpt-4o", &[Capability::Logprobs])
            .unwrap();
        assert_eq!(result.native, vec![Capability::Logprobs]);
    }
}

// ===========================================================================
// 8. Feature flags — enable/disable via config
// ===========================================================================

mod feature_flags {
    use super::*;

    fn apply_feature_flags(m: &CapabilityManifest, disabled: &[Capability]) -> CapabilityManifest {
        let mut result = m.clone();
        for cap in disabled {
            result.insert(cap.clone(), CoreSupportLevel::Unsupported);
        }
        result
    }

    #[test]
    fn disable_single_capability() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        let restricted = apply_feature_flags(&m, &[Capability::ToolBash]);
        assert!(matches!(
            restricted.get(&Capability::ToolBash),
            Some(CoreSupportLevel::Unsupported)
        ));
        assert!(matches!(
            restricted.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn disable_multiple_capabilities() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let restricted = apply_feature_flags(&m, &[Capability::ToolBash, Capability::ToolRead]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolBash,
                Capability::ToolRead,
            ],
            &restricted,
        );
        assert!(!result.is_viable());
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.unsupported.len(), 2);
    }

    #[test]
    fn disabled_then_re_enabled() {
        let mut m = manifest(&[(Capability::ToolBash, CoreSupportLevel::Native)]);
        m.insert(Capability::ToolBash, CoreSupportLevel::Unsupported);
        let result = negotiate_capabilities(&[Capability::ToolBash], &m);
        assert!(!result.is_viable());

        m.insert(Capability::ToolBash, CoreSupportLevel::Native);
        let result = negotiate_capabilities(&[Capability::ToolBash], &m);
        assert!(result.is_viable());
    }

    #[test]
    fn feature_flag_affects_negotiation_but_not_registry() {
        let mut reg = CapabilityRegistry::new();
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        reg.register("backend", m);

        // Negotiate with flag-disabled manifest (not modifying registry)
        let flagged = apply_feature_flags(reg.get("backend").unwrap(), &[Capability::ToolBash]);
        let result = negotiate_capabilities(&[Capability::ToolBash], &flagged);
        assert!(!result.is_viable());

        // Registry original is untouched
        let original = reg.get("backend").unwrap();
        assert!(matches!(
            original.get(&Capability::ToolBash),
            Some(CoreSupportLevel::Native)
        ));
    }
}

// ===========================================================================
// 9. Version compatibility
// ===========================================================================

mod version_compatibility {
    use super::*;

    #[test]
    fn same_requirements_different_manifests_produce_different_results() {
        let m_v1 = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let m_v2 = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);

        let reqs = require_native(&[Capability::Streaming]);
        let r1 = negotiate(&m_v1, &reqs);
        let r2 = negotiate(&m_v2, &reqs);
        assert!(!r1.is_viable());
        assert!(r2.is_viable());
    }

    #[test]
    fn manifest_superset_is_backward_compatible() {
        let m_v1 = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let m_v2 = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let reqs = require_emulated(&[Capability::Streaming]);
        let r1 = negotiate(&m_v1, &reqs);
        let r2 = negotiate(&m_v2, &reqs);
        assert!(r1.is_viable());
        assert!(r2.is_viable());
    }

    #[test]
    fn requirement_subset_always_satisfied_when_superset_is() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let full = require_emulated(&[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ]);
        let subset = require_emulated(&[Capability::Streaming, Capability::ToolUse]);

        let r_full = negotiate(&m, &full);
        let r_subset = negotiate(&m, &subset);
        assert!(r_full.is_viable());
        assert!(r_subset.is_viable());
        assert!(r_subset.native.len() <= r_full.native.len());
    }

    #[test]
    fn mixed_min_support_requirements() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let reqs = require(&[
            (Capability::Streaming, MinSupport::Native),
            (Capability::ToolRead, MinSupport::Emulated),
        ]);
        let result = negotiate(&m, &reqs);
        assert!(result.is_viable());
    }

    #[test]
    fn native_requirement_blocks_emulated_support() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let reqs = require_native(&[Capability::Streaming]);
        let result = negotiate(&m, &reqs);
        assert!(!result.is_viable());
        assert_eq!(result.unsupported.len(), 1);
    }
}

// ===========================================================================
// 10. Concurrent negotiation
// ===========================================================================

mod concurrent_negotiation {
    use super::*;

    #[test]
    fn parallel_negotiation_produces_consistent_results() {
        let m = Arc::new(manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Emulated),
        ]));
        let required: Arc<Vec<Capability>> = Arc::new(vec![
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ]);
        let results: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));

        let mut handles = vec![];
        for _ in 0..10 {
            let m = Arc::clone(&m);
            let required = Arc::clone(&required);
            let results = Arc::clone(&results);
            handles.push(thread::spawn(move || {
                let r = negotiate_capabilities(&required, &m);
                results.lock().unwrap().push(r.is_viable());
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let results = results.lock().unwrap();
        assert_eq!(results.len(), 10);
        assert!(results.iter().all(|v| *v));
    }

    #[test]
    fn concurrent_registry_reads_are_safe() {
        let reg = Arc::new(CapabilityRegistry::with_defaults());
        let mut handles = vec![];
        for _ in 0..8 {
            let reg = Arc::clone(&reg);
            handles.push(thread::spawn(move || {
                let r = reg.negotiate_by_name("openai/gpt-4o", &[Capability::Streaming]);
                r.unwrap().is_viable()
            }));
        }
        for h in handles {
            assert!(h.join().unwrap());
        }
    }

    #[test]
    fn concurrent_negotiate_different_caps() {
        let m = Arc::new(manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]));
        let caps_list: Vec<Vec<Capability>> = vec![
            vec![Capability::Streaming],
            vec![Capability::ToolUse],
            vec![Capability::Vision],
            vec![Capability::Streaming, Capability::ToolUse],
        ];
        let handles: Vec<_> = caps_list
            .into_iter()
            .map(|caps| {
                let m = Arc::clone(&m);
                thread::spawn(move || negotiate_capabilities(&caps, &m))
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(results[0].is_viable());
        assert!(results[1].is_viable());
        assert!(!results[2].is_viable());
        assert!(results[3].is_viable());
    }
}

// ===========================================================================
// 11. Caching
// ===========================================================================

mod caching {
    use super::*;

    #[test]
    fn same_inputs_produce_identical_results() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let caps = vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::Vision,
        ];
        let r1 = negotiate_capabilities(&caps, &m);
        let r2 = negotiate_capabilities(&caps, &m);
        assert_eq!(r1.native, r2.native);
        assert_eq!(r1.emulated, r2.emulated);
        assert_eq!(r1.unsupported, r2.unsupported);
    }

    #[test]
    fn result_is_serializable_for_caching() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Vision],
        );
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: NegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.native, result.native);
        assert_eq!(deserialized.is_viable(), result.is_viable());
    }

    #[test]
    fn compatibility_report_is_serializable() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        let report = generate_report(&result);
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.compatible, report.compatible);
        assert_eq!(deserialized.native_count, report.native_count);
    }

    #[test]
    fn serde_roundtrip_preserves_negotiation_result() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &m,
        );
        let json = serde_json::to_string(&result).unwrap();
        let back: NegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.native.len(), result.native.len());
        assert_eq!(back.emulated.len(), result.emulated.len());
        assert_eq!(back.unsupported.len(), result.unsupported.len());
    }

    #[test]
    fn policy_serde_roundtrip() {
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
}

// ===========================================================================
// 12. Report generation
// ===========================================================================

mod report_generation {
    use super::*;

    #[test]
    fn fully_compatible_report() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming, Capability::ToolUse],
            vec![],
            vec![],
        );
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 2);
        assert_eq!(report.emulated_count, 0);
        assert_eq!(report.unsupported_count, 0);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn incompatible_report() {
        let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert_eq!(report.unsupported_count, 1);
        assert!(report.summary.contains("incompatible"));
    }

    #[test]
    fn mixed_report_details() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Vision],
        );
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert_eq!(report.native_count, 1);
        assert_eq!(report.emulated_count, 1);
        assert_eq!(report.unsupported_count, 1);
        assert_eq!(report.details.len(), 3);
    }

    #[test]
    fn report_display_matches_summary() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        let report = generate_report(&result);
        assert_eq!(format!("{report}"), report.summary);
    }

    #[test]
    fn empty_result_produces_compatible_report() {
        let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 0);
    }

    #[test]
    fn report_details_have_correct_support_levels() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Vision],
        );
        let report = generate_report(&result);

        let native_detail = report
            .details
            .iter()
            .find(|(name, _)| name.contains("Streaming"))
            .unwrap();
        assert!(matches!(native_detail.1, SupportLevel::Native));

        let emulated_detail = report
            .details
            .iter()
            .find(|(name, _)| name.contains("ToolRead"))
            .unwrap();
        assert!(matches!(emulated_detail.1, SupportLevel::Emulated { .. }));

        let unsupported_detail = report
            .details
            .iter()
            .find(|(name, _)| name.contains("Vision"))
            .unwrap();
        assert!(matches!(
            unsupported_detail.1,
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn negotiation_result_display() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Vision],
        );
        let display = format!("{result}");
        assert!(display.contains("1 native"));
        assert!(display.contains("1 emulated"));
        assert!(display.contains("1 unsupported"));
        assert!(display.contains("not viable"));
    }

    #[test]
    fn viable_result_display() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        let display = format!("{result}");
        assert!(display.contains("viable"));
    }
}

// ===========================================================================
// Cross-cutting: check_capability, default manifests, registry compare
// ===========================================================================

mod cross_cutting {
    use super::*;

    #[test]
    fn check_capability_native() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        assert!(matches!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Native
        ));
    }

    #[test]
    fn check_capability_emulated() {
        let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        assert!(matches!(
            check_capability(&m, &Capability::ToolRead),
            SupportLevel::Emulated { .. }
        ));
    }

    #[test]
    fn check_capability_restricted() {
        let m = manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        assert!(matches!(
            check_capability(&m, &Capability::ToolBash),
            SupportLevel::Restricted { .. }
        ));
    }

    #[test]
    fn check_capability_explicit_unsupported() {
        let m = manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
        let level = check_capability(&m, &Capability::Vision);
        if let SupportLevel::Unsupported { reason } = level {
            assert!(reason.contains("explicitly"));
        } else {
            panic!("expected Unsupported");
        }
    }

    #[test]
    fn check_capability_missing() {
        let m = manifest(&[]);
        let level = check_capability(&m, &Capability::Vision);
        if let SupportLevel::Unsupported { reason } = level {
            assert!(reason.contains("not declared"));
        } else {
            panic!("expected Unsupported");
        }
    }

    #[test]
    fn registry_compare_both_present() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "a",
            manifest(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolUse, CoreSupportLevel::Native),
            ]),
        );
        reg.register(
            "b",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        let result = reg.compare("a", "b").unwrap();
        // ToolUse in a is Native, but missing in b → unsupported
        assert!(!result.is_viable());
    }

    #[test]
    fn default_openai_has_native_streaming() {
        let reg = CapabilityRegistry::with_defaults();
        let m = reg.get("openai/gpt-4o").unwrap();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn default_claude_has_native_extended_thinking() {
        let reg = CapabilityRegistry::with_defaults();
        let m = reg.get("anthropic/claude-3.5-sonnet").unwrap();
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn default_gemini_has_native_pdf() {
        let reg = CapabilityRegistry::with_defaults();
        let m = reg.get("google/gemini-1.5-pro").unwrap();
        assert!(matches!(
            m.get(&Capability::PdfInput),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn support_level_display_variants() {
        assert_eq!(format!("{}", SupportLevel::Native), "native");
        assert!(
            format!(
                "{}",
                SupportLevel::Emulated {
                    method: "polyfill".into()
                }
            )
            .contains("polyfill")
        );
        assert!(
            format!(
                "{}",
                SupportLevel::Restricted {
                    reason: "sandbox".into()
                }
            )
            .contains("sandbox")
        );
        assert!(
            format!(
                "{}",
                SupportLevel::Unsupported {
                    reason: "no api".into()
                }
            )
            .contains("no api")
        );
    }

    #[test]
    fn emulated_caps_helper() {
        let result = NegotiationResult::from_simple(
            vec![],
            vec![Capability::ToolRead, Capability::ToolWrite],
            vec![],
        );
        let caps = result.emulated_caps();
        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&Capability::ToolRead));
        assert!(caps.contains(&Capability::ToolWrite));
    }

    #[test]
    fn unsupported_caps_helper() {
        let result = NegotiationResult::from_simple(
            vec![],
            vec![],
            vec![Capability::Vision, Capability::Audio],
        );
        let caps = result.unsupported_caps();
        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&Capability::Vision));
        assert!(caps.contains(&Capability::Audio));
    }
}
