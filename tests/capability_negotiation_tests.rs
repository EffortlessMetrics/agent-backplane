#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Comprehensive tests for capability negotiation and registry.

use abp_capability::negotiate::{NegotiationError, NegotiationPolicy, apply_policy, pre_negotiate};
use abp_capability::{
    CapabilityRegistry, CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel,
    check_capability, default_emulation_strategy, generate_report, negotiate,
    negotiate_capabilities,
};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport as CoreCapabilityReport,
    DialectSupportLevel, NegotiationRequest, check_capabilities, dialect_manifest,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel, WorkOrderBuilder,
};
use abp_integrations::capability::CapabilityMatrix;
use std::collections::BTreeMap;

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

// ===========================================================================
// 1. Capability Declaration
// ===========================================================================

#[test]
fn backend_declares_capabilities_via_manifest() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Unsupported),
    ]);
    assert_eq!(m.len(), 3);
    assert!(matches!(
        m[&Capability::Streaming],
        CoreSupportLevel::Native
    ));
    assert!(matches!(
        m[&Capability::ToolUse],
        CoreSupportLevel::Emulated
    ));
    assert!(matches!(
        m[&Capability::Vision],
        CoreSupportLevel::Unsupported
    ));
}

#[test]
fn registry_stores_backend_manifest() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test-backend", m);
    assert!(reg.contains("test-backend"));
    assert!(reg.get("test-backend").is_some());
}

#[test]
fn registry_with_defaults_has_six_backends() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    assert!(reg.contains("google/gemini-1.5-pro"));
    assert!(reg.contains("moonshot/kimi"));
    assert!(reg.contains("openai/codex"));
    assert!(reg.contains("github/copilot"));
}

// ===========================================================================
// 2. Capability Querying
// ===========================================================================

#[test]
fn query_specific_capability_native() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let level = check_capability(&m, &Capability::Streaming);
    assert!(matches!(level, SupportLevel::Native));
}

#[test]
fn query_specific_capability_emulated() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let level = check_capability(&m, &Capability::ToolUse);
    assert!(matches!(level, SupportLevel::Emulated { .. }));
}

#[test]
fn query_missing_capability_is_unsupported() {
    let m = manifest(&[]);
    let level = check_capability(&m, &Capability::Streaming);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
}

#[test]
fn registry_query_capability_across_all_backends() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    // All 6 default backends should report on streaming
    assert_eq!(results.len(), 6);
    // All should support streaming natively
    for (_, level) in &results {
        assert!(matches!(level, SupportLevel::Native));
    }
}

// ===========================================================================
// 3. Support Levels: Native, Emulated, Unsupported
// ===========================================================================

#[test]
fn native_satisfies_native_requirement() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn native_satisfies_emulated_requirement() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn emulated_does_not_satisfy_native_requirement() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn emulated_satisfies_emulated_requirement() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn unsupported_satisfies_nothing() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_satisfies_emulated_but_not_native() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "sandboxed".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn check_capability_restricted_mapped_correctly() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    assert!(matches!(level, SupportLevel::Restricted { .. }));
}

// ===========================================================================
// 4. Capability Comparison
// ===========================================================================

#[test]
fn compare_two_backends_in_registry() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet");
    assert!(result.is_some());
    let result = result.unwrap();
    // GPT-4o has capabilities Claude may lack or emulate
    assert!(!result.native.is_empty());
}

#[test]
fn compare_unknown_backend_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("nonexistent", "openai/gpt-4o").is_none());
    assert!(reg.compare("openai/gpt-4o", "nonexistent").is_none());
}

#[test]
fn capability_diff_detects_added_capabilities() {
    let old = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let new = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.added[0], Capability::ToolUse);
    assert!(diff.removed.is_empty());
}

#[test]
fn capability_diff_detects_removed_capabilities() {
    let old = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let new = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed.len(), 1);
    assert_eq!(diff.removed[0], Capability::ToolUse);
}

#[test]
fn capability_diff_detects_upgrades() {
    let old = manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let new = manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
    assert!(diff.downgraded.is_empty());
}

#[test]
fn capability_diff_detects_downgrades() {
    let old = manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
    let new = manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.upgraded.is_empty());
    assert_eq!(diff.downgraded.len(), 1);
}

// ===========================================================================
// 5. Best Backend Selection
// ===========================================================================

#[test]
fn best_backend_via_matrix() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::Streaming, Capability::ToolUse]);
    let best = matrix.best_backend(&[Capability::Streaming, Capability::ToolUse]);
    assert_eq!(best.as_deref(), Some("b"));
}

#[test]
fn best_backend_empty_matrix_returns_none() {
    let matrix = CapabilityMatrix::new();
    assert!(matrix.best_backend(&[Capability::Streaming]).is_none());
}

#[test]
fn best_backend_via_negotiator() {
    let m_a = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m_b = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolUse],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::best_match(&request, &[("a", m_a), ("b", m_b)]);
    assert!(result.is_some());
    let (name, _) = result.unwrap();
    assert_eq!(name, "b");
}

#[test]
fn best_match_none_compatible_returns_none() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]);
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    assert!(CapabilityNegotiator::best_match(&request, &[("a", m)]).is_none());
}

#[test]
fn matrix_evaluate_score() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("half", vec![Capability::Streaming]);
    let report = matrix.evaluate("half", &[Capability::Streaming, Capability::ToolUse]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.supported.len(), 1);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn matrix_evaluate_empty_required_scores_one() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("any", vec![Capability::Streaming]);
    let report = matrix.evaluate("any", &[]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
}

// ===========================================================================
// 6. Emulation Labeling
// ===========================================================================

#[test]
fn emulated_capabilities_labeled_in_negotiation_result() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(result.native, vec![Capability::Streaming]);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.emulated[0].0, Capability::ToolUse);
    // Emulation strategy is assigned
    let _ = &result.emulated[0].1;
}

#[test]
fn emulation_labeled_in_compatibility_report() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let result = negotiate_capabilities(&[Capability::ToolUse], &m);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.emulated_count, 1);
    // Detail entry should show emulated
    let detail = &report.details[0];
    assert!(matches!(detail.1, SupportLevel::Emulated { .. }));
}

#[test]
fn emulation_strategy_assigned_per_capability() {
    // Client-side
    assert!(matches!(
        default_emulation_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::ClientSide
    ));
    // Server fallback
    assert!(matches!(
        default_emulation_strategy(&Capability::FunctionCalling),
        EmulationStrategy::ServerFallback
    ));
    // Approximate
    assert!(matches!(
        default_emulation_strategy(&Capability::Vision),
        EmulationStrategy::Approximate
    ));
}

// ===========================================================================
// 7. No Silent Degradation
// ===========================================================================

#[test]
fn strict_policy_rejects_unsupported() {
    let m = manifest(&[]);
    let result = pre_negotiate(&[Capability::Streaming], &m);
    let err = apply_policy(&result, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.unsupported.len(), 1);
    assert_eq!(err.policy, NegotiationPolicy::Strict);
}

#[test]
fn unsupported_fails_ensure_requirements() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Emulated)]);
    let result = abp_backend_core::ensure_capability_requirements(&reqs, &m);
    assert!(result.is_err());
}

#[test]
fn unsupported_capability_in_negotiation_result_flags_not_viable() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![(Capability::Streaming, "missing".into())],
    };
    assert!(!result.is_viable());
    assert!(!result.is_compatible());
}

#[test]
fn negotiation_error_contains_reason() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Vision, "not available".into())],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("Vision"));
    assert!(msg.contains("1 unsupported"));
}

// ===========================================================================
// 8. Capability Intersection
// ===========================================================================

#[test]
fn matrix_common_capabilities() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming, Capability::ToolUse]);
    matrix.register("b", vec![Capability::Streaming, Capability::Vision]);
    let common = matrix.common_capabilities();
    assert_eq!(common.len(), 1);
    assert!(common.contains(&Capability::Streaming));
}

#[test]
fn matrix_common_capabilities_empty_when_disjoint() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::Vision]);
    assert!(matrix.common_capabilities().is_empty());
}

#[test]
fn matrix_common_capabilities_empty_matrix() {
    let matrix = CapabilityMatrix::new();
    assert!(matrix.common_capabilities().is_empty());
}

#[test]
fn matrix_backends_for_capability() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::Streaming, Capability::ToolUse]);
    matrix.register("c", vec![Capability::ToolUse]);
    let backends = matrix.backends_for(&Capability::Streaming);
    assert_eq!(backends.len(), 2);
    assert!(backends.contains(&"a".to_string()));
    assert!(backends.contains(&"b".to_string()));
}

// ===========================================================================
// 9. Capability Serialization
// ===========================================================================

#[test]
fn capability_manifest_json_roundtrip() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Unsupported),
        (
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        ),
    ]);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(m.len(), back.len());
    assert!(matches!(
        back[&Capability::Streaming],
        CoreSupportLevel::Native
    ));
    assert!(matches!(
        back[&Capability::ToolUse],
        CoreSupportLevel::Emulated
    ));
    assert!(matches!(
        back[&Capability::Vision],
        CoreSupportLevel::Unsupported
    ));
    assert!(matches!(
        back[&Capability::ToolBash],
        CoreSupportLevel::Restricted { .. }
    ));
}

#[test]
fn negotiation_result_serde_roundtrip() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolUse, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Vision, "not available".into())],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(result, back);
}

#[test]
fn compatibility_report_serde_roundtrip() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let result = negotiate_capabilities(&[Capability::Streaming], &m);
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, back);
}

#[test]
fn support_level_serde_roundtrip() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated {
            method: "adapter".into(),
        },
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
        SupportLevel::Unsupported {
            reason: "not supported".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(level, &back);
    }
}

// ===========================================================================
// 10. Stream Support
// ===========================================================================

#[test]
fn streaming_capability_native_detection() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    ));
}

#[test]
fn streaming_absent_from_manifest_is_unsupported() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn all_default_backends_support_streaming_natively() {
    let reg = CapabilityRegistry::with_defaults();
    for name in reg.names() {
        let m = reg.get(name).unwrap();
        assert!(
            matches!(
                m.get(&Capability::Streaming),
                Some(CoreSupportLevel::Native)
            ),
            "backend {name} should natively support streaming"
        );
    }
}

// ===========================================================================
// 11. Tool Support
// ===========================================================================

#[test]
fn tool_use_capability_detection() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::ToolUse),
        SupportLevel::Native
    ));
}

#[test]
fn tool_specific_capabilities() {
    let tools = [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ];
    let m: CapabilityManifest = tools
        .iter()
        .map(|t| (t.clone(), CoreSupportLevel::Native))
        .collect();
    for tool in &tools {
        assert!(matches!(check_capability(&m, tool), SupportLevel::Native));
    }
}

#[test]
fn matrix_tool_capability_query() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register(
        "codex",
        vec![
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolBash,
        ],
    );
    matrix.register("gpt4", vec![Capability::ToolUse]);
    assert!(matrix.supports("codex", &Capability::ToolRead));
    assert!(matrix.supports("codex", &Capability::ToolBash));
    assert!(!matrix.supports("gpt4", &Capability::ToolRead));
}

// ===========================================================================
// 12. Vision Support
// ===========================================================================

#[test]
fn vision_capability_detection() {
    let m = manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::Vision),
        SupportLevel::Native
    ));
}

#[test]
fn image_input_capability_detection() {
    let m = manifest(&[(Capability::ImageInput, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::ImageInput),
        SupportLevel::Native
    ));
}

#[test]
fn multimodal_capabilities_in_default_manifests() {
    let reg = CapabilityRegistry::with_defaults();
    // GPT-4o supports vision natively
    let gpt4o = reg.get("openai/gpt-4o").unwrap();
    assert!(matches!(
        gpt4o.get(&Capability::Vision),
        Some(CoreSupportLevel::Native)
    ));
    // Gemini supports vision and PDF natively
    let gemini = reg.get("google/gemini-1.5-pro").unwrap();
    assert!(matches!(
        gemini.get(&Capability::Vision),
        Some(CoreSupportLevel::Native)
    ));
    assert!(matches!(
        gemini.get(&Capability::PdfInput),
        Some(CoreSupportLevel::Native)
    ));
}

// ===========================================================================
// 13. Context Window / MaxTokens
// ===========================================================================

#[test]
fn max_tokens_capability_native() {
    let m = manifest(&[(Capability::MaxTokens, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::MaxTokens),
        SupportLevel::Native
    ));
}

#[test]
fn max_tokens_in_default_backends() {
    let reg = CapabilityRegistry::with_defaults();
    // All default backends should support MaxTokens natively
    for name in reg.names() {
        let m = reg.get(name).unwrap();
        assert!(
            matches!(
                m.get(&Capability::MaxTokens),
                Some(CoreSupportLevel::Native)
            ),
            "backend {name} should support MaxTokens"
        );
    }
}

// ===========================================================================
// 14. Function Calling Styles
// ===========================================================================

#[test]
fn function_calling_native_for_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let gpt4o = reg.get("openai/gpt-4o").unwrap();
    assert!(matches!(
        gpt4o.get(&Capability::FunctionCalling),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn function_calling_emulated_for_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let claude = reg.get("anthropic/claude-3.5-sonnet").unwrap();
    assert!(matches!(
        claude.get(&Capability::FunctionCalling),
        Some(CoreSupportLevel::Emulated)
    ));
}

#[test]
fn function_calling_vs_tool_use_distinction() {
    let m = manifest(&[
        (Capability::FunctionCalling, CoreSupportLevel::Emulated),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    // They are distinct capabilities
    assert!(matches!(
        check_capability(&m, &Capability::FunctionCalling),
        SupportLevel::Emulated { .. }
    ));
    assert!(matches!(
        check_capability(&m, &Capability::ToolUse),
        SupportLevel::Native
    ));
}

#[test]
fn function_calling_emulation_strategy_is_server_fallback() {
    assert!(matches!(
        default_emulation_strategy(&Capability::FunctionCalling),
        EmulationStrategy::ServerFallback
    ));
}

// ===========================================================================
// 15. Capability Negotiation Flow
// ===========================================================================

#[test]
fn pre_execution_negotiation_validates_feasibility() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let result = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(result.is_viable());
    assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn negotiation_with_work_order_requirements() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolUse, MinSupport::Emulated),
    ]);
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 2);
}

#[test]
fn dialect_aware_check_capabilities_with_work_order() {
    let wo = WorkOrderBuilder::new("test task")
        .requirements(require(&[
            (Capability::Streaming, MinSupport::Native),
            (Capability::ToolUse, MinSupport::Native),
        ]))
        .build();

    let report = check_capabilities(&wo, "claude", "openai");
    assert!(!report.entries.is_empty());
    // Streaming should be native in OpenAI
    let streaming_entry = report
        .entries
        .iter()
        .find(|e| e.capability == Capability::Streaming);
    assert!(streaming_entry.is_some());
    assert!(matches!(
        streaming_entry.unwrap().support,
        DialectSupportLevel::Native
    ));
}

#[test]
fn dialect_manifest_known_dialects() {
    let claude = dialect_manifest("claude");
    assert!(!claude.is_empty());
    let openai = dialect_manifest("openai");
    assert!(!openai.is_empty());
    let gemini = dialect_manifest("gemini");
    assert!(!gemini.is_empty());
}

#[test]
fn dialect_manifest_unknown_returns_empty() {
    let unknown = dialect_manifest("nonexistent");
    assert!(unknown.is_empty());
}

#[test]
fn capability_report_all_satisfiable() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(require(&[(Capability::Streaming, MinSupport::Native)]))
        .build();
    let report = check_capabilities(&wo, "claude", "claude");
    assert!(report.all_satisfiable());
}

#[test]
fn capability_report_unsupported_detected() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(require(&[(Capability::Logprobs, MinSupport::Native)]))
        .build();
    let report = check_capabilities(&wo, "openai", "claude");
    assert!(!report.all_satisfiable());
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

#[test]
fn capability_report_metadata_roundtrip() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(require(&[(Capability::Streaming, MinSupport::Native)]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    let metadata = report.to_receipt_metadata();
    assert!(!metadata.is_null());
    // Should be deserializable back
    let back: CoreCapabilityReport = serde_json::from_value(metadata).unwrap();
    assert_eq!(back.source_dialect, "claude");
    assert_eq!(back.target_dialect, "openai");
}

// ===========================================================================
// Additional Coverage: NegotiationResult helpers
// ===========================================================================

#[test]
fn negotiation_result_total_count() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolUse, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Vision, "not available".into())],
    };
    assert_eq!(result.total(), 3);
}

#[test]
fn negotiation_result_emulated_caps_helper() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolUse, EmulationStrategy::ClientSide),
            (Capability::JsonMode, EmulationStrategy::ServerFallback),
        ],
        unsupported: vec![],
    };
    let caps = result.emulated_caps();
    assert_eq!(caps.len(), 2);
    assert!(caps.contains(&Capability::ToolUse));
    assert!(caps.contains(&Capability::JsonMode));
}

#[test]
fn negotiation_result_unsupported_caps_helper() {
    let result =
        NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision, Capability::Audio]);
    let caps = result.unsupported_caps();
    assert_eq!(caps.len(), 2);
}

#[test]
fn negotiation_result_from_simple() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Vision],
    );
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
    // from_simple assigns ClientSide for emulated
    assert!(matches!(
        result.emulated[0].1,
        EmulationStrategy::ClientSide
    ));
}

#[test]
fn negotiation_result_warnings_with_approximate() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolUse, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let warnings = result.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_display() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolUse, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    let display = format!("{result}");
    assert!(display.contains("1 native"));
    assert!(display.contains("1 emulated"));
    assert!(display.contains("viable"));
}

// ===========================================================================
// Additional: Registry operations
// ===========================================================================

#[test]
fn registry_unregister() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test", m);
    assert!(reg.unregister("test"));
    assert!(!reg.contains("test"));
    assert!(!reg.unregister("test"));
}

#[test]
fn registry_negotiate_by_name() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "backend",
        manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]),
    );
    let result = reg.negotiate_by_name("backend", &[Capability::Streaming, Capability::ToolUse]);
    assert!(result.is_some());
    let result = result.unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn registry_negotiate_by_name_unknown_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(
        reg.negotiate_by_name("unknown", &[Capability::Streaming])
            .is_none()
    );
}

#[test]
fn registry_names_list() {
    let mut reg = CapabilityRegistry::new();
    reg.register("alpha", BTreeMap::new());
    reg.register("beta", BTreeMap::new());
    let names = reg.names();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[test]
fn registry_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

// ===========================================================================
// Additional: Matrix operations
// ===========================================================================

#[test]
fn matrix_register_merges() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("a", vec![Capability::ToolUse]);
    let caps = matrix.all_capabilities("a").unwrap();
    assert_eq!(caps.len(), 2);
    assert!(caps.contains(&Capability::Streaming));
    assert!(caps.contains(&Capability::ToolUse));
}

#[test]
fn matrix_supports_missing_backend() {
    let matrix = CapabilityMatrix::new();
    assert!(!matrix.supports("nonexistent", &Capability::Streaming));
}

#[test]
fn matrix_backend_count() {
    let mut matrix = CapabilityMatrix::new();
    assert!(matrix.is_empty());
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::ToolUse]);
    assert_eq!(matrix.backend_count(), 2);
    assert!(!matrix.is_empty());
}

// ===========================================================================
// Additional: Negotiator
// ===========================================================================

#[test]
fn negotiator_emulated_satisfies_emulated_minimum() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let request = NegotiationRequest {
        required: vec![Capability::ToolUse],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Emulated,
    };
    let result = CapabilityNegotiator::negotiate(&request, &m);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 1);
}

#[test]
fn negotiator_emulated_fails_native_minimum() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let request = NegotiationRequest {
        required: vec![Capability::ToolUse],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&request, &m);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied.len(), 1);
}

#[test]
fn negotiator_preferred_as_bonus() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolUse],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&request, &m);
    assert!(result.is_compatible);
    assert_eq!(result.bonus.len(), 1);
    assert_eq!(result.bonus[0], Capability::ToolUse);
}

// ===========================================================================
// Additional: Permissive policy
// ===========================================================================

#[test]
fn permissive_policy_always_succeeds() {
    let m = manifest(&[]);
    let result = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert!(!result.is_viable());
    assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
}

// ===========================================================================
// Additional: generate_report details
// ===========================================================================

#[test]
fn generate_report_summary_compatible() {
    let result = negotiate_capabilities(
        &[Capability::Streaming],
        &manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert!(report.summary.contains("fully compatible"));
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
}

#[test]
fn generate_report_summary_incompatible() {
    let result = negotiate_capabilities(&[Capability::Streaming], &manifest(&[]));
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
    assert_eq!(report.unsupported_count, 1);
}

#[test]
fn generate_report_display_matches_summary() {
    let result = negotiate_capabilities(
        &[Capability::Streaming],
        &manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    let report = generate_report(&result);
    assert_eq!(format!("{report}"), report.summary);
}
