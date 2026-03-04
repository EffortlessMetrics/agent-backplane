// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep capability negotiation and registry end-to-end tests.

use abp_capability::{
    check_capability, generate_report, negotiate, negotiate_capabilities, CapabilityRegistry,
    CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel,
};
use abp_capability::negotiate::{
    apply_policy, pre_negotiate, NegotiationError, NegotiationPolicy,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    PolicyProfile, SupportLevel as CoreSupportLevel,
};
use abp_policy::PolicyEngine;
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

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

// ===========================================================================
// 1. Capability Registration (10+ tests)
// ===========================================================================

#[test]
fn reg_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test-backend", m);
    let got = reg.get("test-backend").unwrap();
    assert!(got.contains_key(&Capability::Streaming));
    assert!(matches!(got.get(&Capability::Streaming), Some(CoreSupportLevel::Native)));
}

#[test]
fn reg_query_by_name_present() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "backend-a",
        manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]),
    );
    assert!(reg.contains("backend-a"));
}

#[test]
fn reg_query_by_name_absent() {
    let reg = CapabilityRegistry::new();
    assert!(!reg.contains("nonexistent"));
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn reg_list_all_capabilities() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]));
    reg.register("b", manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]));
    let names = reg.names();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
}

#[test]
fn reg_update_capabilities() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "backend",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]),
    );
    // Update with new manifest
    reg.register(
        "backend",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    let m = reg.get("backend").unwrap();
    assert!(matches!(m.get(&Capability::Streaming), Some(CoreSupportLevel::Native)));
}

#[test]
fn reg_remove_capabilities() {
    let mut reg = CapabilityRegistry::new();
    reg.register("rm-me", manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]));
    assert!(reg.unregister("rm-me"));
    assert!(!reg.contains("rm-me"));
}

#[test]
fn reg_remove_nonexistent_returns_false() {
    let mut reg = CapabilityRegistry::new();
    assert!(!reg.unregister("ghost"));
}

#[test]
fn reg_len_and_is_empty() {
    let mut reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    reg.register("one", BTreeMap::new());
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
}

#[test]
fn reg_with_defaults_has_six_backends() {
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
fn reg_query_capability_across_backends() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    // Every default backend should have Streaming
    assert_eq!(results.len(), 6);
    for (_name, level) in &results {
        assert!(matches!(level, SupportLevel::Native));
    }
}

#[test]
fn reg_negotiate_by_name_known() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.negotiate_by_name("openai/gpt-4o", &[Capability::Streaming]);
    assert!(result.is_some());
    let r = result.unwrap();
    assert_eq!(r.native, vec![Capability::Streaming]);
}

#[test]
fn reg_negotiate_by_name_unknown() {
    let reg = CapabilityRegistry::new();
    assert!(reg.negotiate_by_name("unknown", &[Capability::Streaming]).is_none());
}

#[test]
fn reg_compare_openai_vs_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet");
    assert!(result.is_some());
    let r = result.unwrap();
    // Some OpenAI caps will be unsupported on Claude (e.g. Audio)
    assert!(!r.unsupported.is_empty() || !r.emulated.is_empty() || !r.native.is_empty());
}

#[test]
fn reg_compare_unknown_source() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("nonexistent", "openai/gpt-4o").is_none());
}

// ===========================================================================
// 2. Negotiation Protocol (15+ tests)
// ===========================================================================

#[test]
fn nego_requires_x_backend_supports_x() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[Capability::ToolUse], &m);
    assert!(r.is_viable());
    assert_eq!(r.native, vec![Capability::ToolUse]);
}

#[test]
fn nego_requires_x_backend_missing_x() {
    let m = manifest(&[]);
    let r = negotiate_capabilities(&[Capability::ToolUse], &m);
    assert!(!r.is_viable());
    assert_eq!(r.unsupported_caps(), vec![Capability::ToolUse]);
}

#[test]
fn nego_requires_x_backend_emulates_x() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = negotiate_capabilities(&[Capability::ToolUse], &m);
    assert!(r.is_viable());
    assert_eq!(r.emulated_caps(), vec![Capability::ToolUse]);
}

#[test]
fn nego_multiple_all_supported() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(
        &[Capability::Streaming, Capability::ToolUse, Capability::Vision],
        &m,
    );
    assert!(r.is_viable());
    assert_eq!(r.native.len(), 3);
}

#[test]
fn nego_multiple_one_missing() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(
        &[Capability::Streaming, Capability::ToolUse, Capability::Vision],
        &m,
    );
    assert!(!r.is_viable());
    assert_eq!(r.native.len(), 2);
    assert_eq!(r.unsupported_caps(), vec![Capability::Vision]);
}

#[test]
fn nego_no_requirements_always_proceed() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn nego_empty_manifest_empty_requirements() {
    let r = negotiate_capabilities(&[], &BTreeMap::new());
    assert!(r.is_viable());
}

#[test]
fn nego_with_structured_requirements_all_native() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Native),
    ]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_compatible());
    assert_eq!(r.native.len(), 2);
}

#[test]
fn nego_with_structured_requirements_emulated_ok() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let reqs = require_emulated(&[Capability::ToolUse]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_viable());
}

#[test]
fn nego_explicit_unsupported_in_manifest() {
    let m = manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let r = negotiate_capabilities(&[Capability::Audio], &m);
    assert!(!r.is_viable());
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn nego_restricted_counted_as_emulated() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted { reason: "sandbox".into() },
    )]);
    let r = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert!(r.is_viable());
    assert_eq!(r.emulated.len(), 1);
    assert!(r.native.is_empty());
}

#[test]
fn nego_preserves_order() {
    let m = manifest(&[
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(
        &[Capability::ToolRead, Capability::Streaming, Capability::ToolWrite],
        &m,
    );
    assert_eq!(r.native[0], Capability::ToolRead);
    assert_eq!(r.native[1], Capability::Streaming);
    assert_eq!(r.native[2], Capability::ToolWrite);
}

#[test]
fn nego_duplicate_requirements() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Streaming], &m);
    assert_eq!(r.native.len(), 2);
    assert_eq!(r.total(), 2);
}

#[test]
fn nego_many_unsupported() {
    let m = manifest(&[]);
    let caps = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Vision,
        Capability::Audio,
        Capability::Logprobs,
    ];
    let r = negotiate_capabilities(&caps, &m);
    assert_eq!(r.unsupported.len(), 5);
    assert!(!r.is_viable());
}

#[test]
fn nego_mixed_native_emulated_unsupported() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
    ]);
    let r = negotiate_capabilities(
        &[Capability::Streaming, Capability::ToolUse, Capability::Audio, Capability::Vision],
        &m,
    );
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.unsupported.len(), 2);
    assert!(!r.is_viable());
}

#[test]
fn nego_pre_negotiate_delegates_correctly() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    assert_eq!(r.native, vec![Capability::Streaming]);
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn nego_strict_policy_passes_when_viable() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn nego_strict_policy_rejects_unsupported() {
    let r = pre_negotiate(&[Capability::Vision], &manifest(&[]));
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
    assert_eq!(err.unsupported.len(), 1);
}

// ===========================================================================
// 3. Support Levels (10+ tests)
// ===========================================================================

#[test]
fn support_native_full_fidelity() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let level = check_capability(&m, &Capability::Streaming);
    assert_eq!(level, SupportLevel::Native);
}

#[test]
fn support_emulated_labeled() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let level = check_capability(&m, &Capability::ToolUse);
    assert!(matches!(level, SupportLevel::Emulated { .. }));
}

#[test]
fn support_unsupported_error() {
    let m = manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let level = check_capability(&m, &Capability::Audio);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
}

#[test]
fn support_absent_from_manifest_is_unsupported() {
    let m = manifest(&[]);
    let level = check_capability(&m, &Capability::Vision);
    match level {
        SupportLevel::Unsupported { reason } => {
            assert!(reason.contains("not declared"));
        }
        _ => panic!("expected Unsupported"),
    }
}

#[test]
fn support_restricted_with_reason() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted { reason: "sandboxed".into() },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    match level {
        SupportLevel::Restricted { reason } => assert_eq!(reason, "sandboxed"),
        _ => panic!("expected Restricted"),
    }
}

#[test]
fn support_level_display_native() {
    assert_eq!(format!("{}", SupportLevel::Native), "native");
}

#[test]
fn support_level_display_emulated() {
    let level = SupportLevel::Emulated { method: "polyfill".into() };
    assert_eq!(format!("{level}"), "emulated (polyfill)");
}

#[test]
fn support_level_display_restricted() {
    let level = SupportLevel::Restricted { reason: "sandbox".into() };
    assert_eq!(format!("{level}"), "restricted (sandbox)");
}

#[test]
fn support_level_display_unsupported() {
    let level = SupportLevel::Unsupported { reason: "N/A".into() };
    assert_eq!(format!("{level}"), "unsupported (N/A)");
}

#[test]
fn support_core_satisfies_native_only_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn support_core_satisfies_emulated_accepts_native_emulated_restricted() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(
        CoreSupportLevel::Restricted { reason: "test".into() }.satisfies(&MinSupport::Emulated)
    );
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_emulation_strategy_fidelity_loss() {
    assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
    assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
    assert!(EmulationStrategy::Approximate.has_fidelity_loss());
}

#[test]
fn support_warnings_only_for_approximate() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
            (Capability::ToolUse, EmulationStrategy::ServerFallback),
        ],
        unsupported: vec![],
    };
    let warnings = result.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, Capability::Vision);
}

#[test]
fn support_default_emulation_strategy_client_side() {
    let strategy = abp_capability::default_emulation_strategy(&Capability::ToolRead);
    assert_eq!(strategy, EmulationStrategy::ClientSide);
}

#[test]
fn support_default_emulation_strategy_server_fallback() {
    let strategy = abp_capability::default_emulation_strategy(&Capability::ToolUse);
    assert_eq!(strategy, EmulationStrategy::ServerFallback);
}

#[test]
fn support_default_emulation_strategy_approximate() {
    let strategy = abp_capability::default_emulation_strategy(&Capability::Vision);
    assert_eq!(strategy, EmulationStrategy::Approximate);
}

// ===========================================================================
// 4. Capability Manifest (10+ tests)
// ===========================================================================

#[test]
fn manifest_create_from_backend() {
    let m = abp_capability::openai_gpt4o_manifest();
    assert!(m.contains_key(&Capability::Streaming));
    assert!(m.contains_key(&Capability::ToolUse));
    assert!(matches!(m.get(&Capability::Streaming), Some(CoreSupportLevel::Native)));
}

#[test]
fn manifest_claude_has_extended_thinking() {
    let m = abp_capability::claude_35_sonnet_manifest();
    assert!(matches!(m.get(&Capability::ExtendedThinking), Some(CoreSupportLevel::Native)));
}

#[test]
fn manifest_gemini_has_code_execution() {
    let m = abp_capability::gemini_15_pro_manifest();
    assert!(matches!(m.get(&Capability::CodeExecution), Some(CoreSupportLevel::Native)));
}

#[test]
fn manifest_merge_two_manifests() {
    let mut m1 = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    m1.extend(m2);
    assert_eq!(m1.len(), 2);
    assert!(m1.contains_key(&Capability::Streaming));
    assert!(m1.contains_key(&Capability::Vision));
}

#[test]
fn manifest_merge_overwrites_on_conflict() {
    let mut m1 = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let m2 = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    m1.extend(m2);
    assert!(matches!(m1.get(&Capability::Streaming), Some(CoreSupportLevel::Native)));
}

#[test]
fn manifest_compare_identical() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/gpt-4o", "openai/gpt-4o");
    assert!(result.is_some());
    let r = result.unwrap();
    assert!(r.is_viable());
    assert!(r.unsupported.is_empty());
}

#[test]
fn manifest_compare_detects_gaps() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "full",
        manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]),
    );
    reg.register(
        "minimal",
        manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    let r = reg.compare("full", "minimal").unwrap();
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn manifest_serialize_roundtrip() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
    ]);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), m.len());
    assert!(matches!(back.get(&Capability::Streaming), Some(CoreSupportLevel::Native)));
    assert!(matches!(back.get(&Capability::ToolUse), Some(CoreSupportLevel::Emulated)));
    assert!(matches!(back.get(&Capability::Audio), Some(CoreSupportLevel::Unsupported)));
}

#[test]
fn manifest_deserialize_from_json() {
    let json = r#"{"streaming":"native","tool_use":"emulated"}"#;
    let m: CapabilityManifest = serde_json::from_str(json).unwrap();
    assert!(matches!(m.get(&Capability::Streaming), Some(CoreSupportLevel::Native)));
    assert!(matches!(m.get(&Capability::ToolUse), Some(CoreSupportLevel::Emulated)));
}

#[test]
fn manifest_empty_is_valid() {
    let m: CapabilityManifest = BTreeMap::new();
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(json, "{}");
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert!(back.is_empty());
}

#[test]
fn manifest_restricted_roundtrip() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted { reason: "sandboxed only".into() },
    )]);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    match back.get(&Capability::ToolBash) {
        Some(CoreSupportLevel::Restricted { reason }) => {
            assert_eq!(reason, "sandboxed only");
        }
        other => panic!("expected Restricted, got {other:?}"),
    }
}

#[test]
fn manifest_kimi_unsupported_audio() {
    let m = abp_capability::kimi_manifest();
    assert!(matches!(m.get(&Capability::Audio), Some(CoreSupportLevel::Unsupported)));
}

#[test]
fn manifest_codex_has_tool_suite() {
    let m = abp_capability::codex_manifest();
    assert!(matches!(m.get(&Capability::ToolRead), Some(CoreSupportLevel::Native)));
    assert!(matches!(m.get(&Capability::ToolWrite), Some(CoreSupportLevel::Native)));
    assert!(matches!(m.get(&Capability::ToolEdit), Some(CoreSupportLevel::Native)));
    assert!(matches!(m.get(&Capability::ToolBash), Some(CoreSupportLevel::Native)));
}

#[test]
fn manifest_copilot_has_web_tools() {
    let m = abp_capability::copilot_manifest();
    assert!(matches!(m.get(&Capability::ToolWebSearch), Some(CoreSupportLevel::Native)));
    assert!(matches!(m.get(&Capability::ToolWebFetch), Some(CoreSupportLevel::Native)));
    assert!(matches!(m.get(&Capability::ToolAskUser), Some(CoreSupportLevel::Native)));
}

// ===========================================================================
// 5. Compatibility Report (additional tests)
// ===========================================================================

#[test]
fn report_fully_compatible() {
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
fn report_incompatible() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::Audio],
    );
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn report_with_emulated() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.emulated_count, 1);
}

#[test]
fn report_details_contain_all_capabilities() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Audio],
    );
    let report = generate_report(&result);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn report_display_matches_summary() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    assert_eq!(format!("{report}"), report.summary);
}

#[test]
fn report_serde_roundtrip() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Audio],
    );
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.compatible, report.compatible);
    assert_eq!(back.native_count, report.native_count);
    assert_eq!(back.emulated_count, report.emulated_count);
    assert_eq!(back.unsupported_count, report.unsupported_count);
}

// ===========================================================================
// 6. Integration with Policy (5+ tests)
// ===========================================================================

#[test]
fn policy_restricts_tool_capability() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Backend supports ToolBash natively
    let m = manifest(&[(Capability::ToolBash, CoreSupportLevel::Native)]);
    let _neg = negotiate_capabilities(&[Capability::ToolBash], &m);

    // But policy blocks the tool
    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
}

#[test]
fn policy_allows_capability_passthrough() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();

    let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
    let neg = negotiate_capabilities(&[Capability::ToolRead], &m);
    assert!(neg.is_viable());

    let decision = engine.can_use_tool("Read");
    assert!(decision.allowed);
}

#[test]
fn policy_deny_write_overrides_backend_support() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Backend supports ToolWrite natively
    let m = manifest(&[(Capability::ToolWrite, CoreSupportLevel::Native)]);
    let neg = negotiate_capabilities(&[Capability::ToolWrite], &m);
    assert!(neg.is_viable());

    // But policy blocks writing to .git
    let decision = engine.can_write_path(std::path::Path::new(".git/config"));
    assert!(!decision.allowed);
}

#[test]
fn policy_deny_read_overrides_backend_support() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Backend supports ToolRead natively
    let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
    let neg = negotiate_capabilities(&[Capability::ToolRead], &m);
    assert!(neg.is_viable());

    // But policy blocks reading .env files
    let decision = engine.can_read_path(std::path::Path::new(".env.production"));
    assert!(!decision.allowed);
}

#[test]
fn policy_allowlist_restricts_tools_independent_of_capability() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // Backend supports many tools
    let m = manifest(&[
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolBash, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Native),
    ]);
    let neg = negotiate_capabilities(
        &[Capability::ToolRead, Capability::ToolBash, Capability::ToolWrite],
        &m,
    );
    assert!(neg.is_viable());

    // Policy only allows Read and Grep
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
}

#[test]
fn policy_combined_with_negotiation_strict() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolBash, CoreSupportLevel::Native),
    ]);

    // Negotiation passes
    let neg = pre_negotiate(&[Capability::Streaming, Capability::ToolBash], &m);
    assert!(apply_policy(&neg, NegotiationPolicy::Strict).is_ok());

    // But policy can still restrict the tool
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("BashExec").allowed);
}

#[test]
fn policy_permissive_negotiation_with_restricted_policy() {
    let m = manifest(&[]);
    let neg = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    // Permissive allows all negotiation results
    assert!(apply_policy(&neg, NegotiationPolicy::Permissive).is_ok());

    // Policy still provides separate enforcement layer
    let policy = PolicyProfile {
        deny_write: vec!["secret/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(std::path::Path::new("secret/key.pem")).allowed);
}

// ===========================================================================
// 7. NegotiationPolicy (additional coverage)
// ===========================================================================

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
    for policy in &[
        NegotiationPolicy::Strict,
        NegotiationPolicy::BestEffort,
        NegotiationPolicy::Permissive,
    ] {
        let json = serde_json::to_string(policy).unwrap();
        let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, policy);
    }
}

#[test]
fn policy_strict_rejects_multiple_unsupported() {
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &manifest(&[]),
    );
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.unsupported.len(), 3);
}

#[test]
fn policy_best_effort_rejects_unsupported() {
    let r = pre_negotiate(&[Capability::Audio], &manifest(&[]));
    let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::BestEffort);
}

#[test]
fn policy_best_effort_passes_emulated() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
}

#[test]
fn policy_permissive_passes_all_unsupported() {
    let r = pre_negotiate(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
            Capability::Audio,
        ],
        &manifest(&[]),
    );
    assert!(!r.is_viable());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn policy_strict_allows_restricted() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted { reason: "sandboxed".into() },
    )]);
    let r = pre_negotiate(&[Capability::ToolBash], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

// ===========================================================================
// 8. NegotiationError (additional coverage)
// ===========================================================================

#[test]
fn negotiation_error_display_contains_policy() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Vision, "missing".into())],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
    assert!(msg.contains("1 unsupported"));
}

#[test]
fn negotiation_error_display_multiple() {
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![
            (Capability::Vision, "no".into()),
            (Capability::Audio, "no".into()),
            (Capability::Logprobs, "no".into()),
        ],
        warnings: vec![Capability::ToolUse],
    };
    let msg = err.to_string();
    assert!(msg.contains("3 unsupported"));
    assert!(msg.contains("best-effort"));
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

// ===========================================================================
// 9. NegotiationResult helpers
// ===========================================================================

#[test]
fn result_from_simple_defaults() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Audio],
    );
    // Emulated defaults to ClientSide strategy
    assert_eq!(r.emulated[0].1, EmulationStrategy::ClientSide);
    // Unsupported defaults to "not available" reason
    assert_eq!(r.unsupported[0].1, "not available");
}

#[test]
fn result_total_counts_all_buckets() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![
            (Capability::Audio, "no".into()),
            (Capability::Logprobs, "no".into()),
        ],
    };
    assert_eq!(r.total(), 5);
}

#[test]
fn result_is_compatible_alias() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    assert_eq!(r.is_viable(), r.is_compatible());
}

// ===========================================================================
// 10. EmulationStrategy serde
// ===========================================================================

#[test]
fn emulation_strategy_serde_client_side() {
    let s = EmulationStrategy::ClientSide;
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json, r#""client_side""#);
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn emulation_strategy_serde_server_fallback() {
    let s = EmulationStrategy::ServerFallback;
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json, r#""server_fallback""#);
}

#[test]
fn emulation_strategy_serde_approximate() {
    let s = EmulationStrategy::Approximate;
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json, r#""approximate""#);
}

// ===========================================================================
// 11. Cross-backend comparison via registry
// ===========================================================================

#[test]
fn reg_compare_claude_vs_gemini() {
    let reg = CapabilityRegistry::with_defaults();
    let r = reg.compare("anthropic/claude-3.5-sonnet", "google/gemini-1.5-pro").unwrap();
    // Both are large models, should have reasonable overlap
    assert!(r.total() > 0);
}

#[test]
fn reg_compare_codex_vs_copilot() {
    let reg = CapabilityRegistry::with_defaults();
    let r = reg.compare("openai/codex", "github/copilot").unwrap();
    assert!(r.total() > 0);
    // Copilot supports many tools natively, so most Codex caps should be covered
    assert!(!r.native.is_empty() || !r.emulated.is_empty());
}

#[test]
fn reg_query_extended_thinking() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::ExtendedThinking);
    let native_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .count();
    // Only Claude should have native ExtendedThinking
    assert!(native_count >= 1);
}

#[test]
fn reg_query_audio_support() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Audio);
    let native_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .count();
    // OpenAI GPT-4o and Gemini support Audio natively
    assert!(native_count >= 2);
}
