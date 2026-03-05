#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive capability negotiation tests.
//!
//! Covers: capability declarations per dialect, comparisons between dialects,
//! feature support levels, registry operations, capability queries, negotiation
//! between source and target, graceful degradation, manifest generation,
//! runtime capability checks, and transition matrix behaviour.

use abp_capability::negotiate::{apply_policy, pre_negotiate, NegotiationPolicy};
use abp_capability::{
    check_capability, classify_transition, copilot_manifest, default_emulation_strategy,
    generate_report, negotiate, negotiate_capabilities, negotiate_dialects, CapabilityRegistry,
    CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel, TransitionKind,
};
use abp_capability::{
    check_runtime_capabilities, claude_35_sonnet_manifest, codex_manifest, gemini_15_pro_manifest,
    kimi_manifest, openai_gpt4o_manifest, report_mismatches, select_emulation_strategy,
    CapabilityMismatch, CapabilityTransition, DialectNegotiationResult, RuntimeCheckResult,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};
use std::collections::BTreeMap;

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
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

fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ]
}

type ManifestFn = fn() -> CapabilityManifest;

fn all_dialect_manifests() -> Vec<(&'static str, ManifestFn)> {
    vec![
        ("openai/gpt-4o", openai_gpt4o_manifest as ManifestFn),
        (
            "anthropic/claude-3.5-sonnet",
            claude_35_sonnet_manifest as ManifestFn,
        ),
        (
            "google/gemini-1.5-pro",
            gemini_15_pro_manifest as ManifestFn,
        ),
        ("moonshot/kimi", kimi_manifest as ManifestFn),
        ("openai/codex", codex_manifest as ManifestFn),
        ("github/copilot", copilot_manifest as ManifestFn),
    ]
}

// ===========================================================================
// 1. Capability declaration per dialect
// ===========================================================================

#[test]
fn openai_manifest_has_streaming_native() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        m.get(&Capability::Streaming),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn openai_manifest_has_logprobs() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        m.get(&Capability::Logprobs),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn openai_manifest_has_seed_determinism() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        m.get(&Capability::SeedDeterminism),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn openai_manifest_extended_thinking_unsupported() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        m.get(&Capability::ExtendedThinking),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn claude_manifest_has_extended_thinking() {
    let m = claude_35_sonnet_manifest();
    assert_eq!(
        m.get(&Capability::ExtendedThinking),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn claude_manifest_has_cache_control() {
    let m = claude_35_sonnet_manifest();
    assert_eq!(
        m.get(&Capability::CacheControl),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn claude_manifest_logprobs_unsupported() {
    let m = claude_35_sonnet_manifest();
    assert_eq!(
        m.get(&Capability::Logprobs),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn claude_manifest_audio_unsupported() {
    let m = claude_35_sonnet_manifest();
    assert_eq!(
        m.get(&Capability::Audio),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn gemini_manifest_has_pdf_input() {
    let m = gemini_15_pro_manifest();
    assert_eq!(
        m.get(&Capability::PdfInput),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn gemini_manifest_has_code_execution() {
    let m = gemini_15_pro_manifest();
    assert_eq!(
        m.get(&Capability::CodeExecution),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn gemini_manifest_batch_mode_unsupported() {
    let m = gemini_15_pro_manifest();
    assert_eq!(
        m.get(&Capability::BatchMode),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn kimi_manifest_has_vision() {
    let m = kimi_manifest();
    assert_eq!(m.get(&Capability::Vision), Some(&CoreSupportLevel::Native));
}

#[test]
fn kimi_manifest_audio_unsupported() {
    let m = kimi_manifest();
    assert_eq!(
        m.get(&Capability::Audio),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn codex_manifest_has_tool_suite() {
    let m = codex_manifest();
    for cap in [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ] {
        assert_eq!(m.get(&cap), Some(&CoreSupportLevel::Native), "{cap:?}");
    }
}

#[test]
fn copilot_manifest_has_web_tools() {
    let m = copilot_manifest();
    assert_eq!(
        m.get(&Capability::ToolWebSearch),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(
        m.get(&Capability::ToolWebFetch),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(
        m.get(&Capability::ToolAskUser),
        Some(&CoreSupportLevel::Native)
    );
}

#[test]
fn copilot_manifest_extended_thinking_unsupported() {
    let m = copilot_manifest();
    assert_eq!(
        m.get(&Capability::ExtendedThinking),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn every_dialect_manifest_is_nonempty() {
    for (name, factory) in all_dialect_manifests() {
        let m = factory();
        assert!(!m.is_empty(), "{name} manifest should not be empty");
    }
}

#[test]
fn every_dialect_declares_streaming() {
    for (name, factory) in all_dialect_manifests() {
        let m = factory();
        assert!(
            m.contains_key(&Capability::Streaming),
            "{name} must declare streaming"
        );
        assert_eq!(
            m.get(&Capability::Streaming),
            Some(&CoreSupportLevel::Native),
            "{name} streaming should be native"
        );
    }
}

#[test]
fn every_dialect_declares_tool_use() {
    for (name, factory) in all_dialect_manifests() {
        let m = factory();
        assert!(
            m.contains_key(&Capability::ToolUse),
            "{name} must declare tool_use"
        );
    }
}

// ===========================================================================
// 2. Capability comparison between dialects
// ===========================================================================

#[test]
fn compare_openai_vs_claude_logprobs() {
    let oai = openai_gpt4o_manifest();
    let claude = claude_35_sonnet_manifest();
    assert_eq!(
        oai.get(&Capability::Logprobs),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(
        claude.get(&Capability::Logprobs),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn compare_claude_vs_openai_extended_thinking() {
    let oai = openai_gpt4o_manifest();
    let claude = claude_35_sonnet_manifest();
    assert_eq!(
        claude.get(&Capability::ExtendedThinking),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(
        oai.get(&Capability::ExtendedThinking),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn compare_gemini_vs_openai_pdf_input() {
    let gemini = gemini_15_pro_manifest();
    let oai = openai_gpt4o_manifest();
    assert_eq!(
        gemini.get(&Capability::PdfInput),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(
        oai.get(&Capability::PdfInput),
        Some(&CoreSupportLevel::Unsupported)
    );
}

#[test]
fn compare_codex_vs_kimi_tool_read() {
    let codex = codex_manifest();
    let kimi = kimi_manifest();
    assert_eq!(
        codex.get(&Capability::ToolRead),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(kimi.get(&Capability::ToolRead), None);
}

#[test]
fn compare_copilot_vs_codex_web_search() {
    let copilot = copilot_manifest();
    let codex = codex_manifest();
    assert_eq!(
        copilot.get(&Capability::ToolWebSearch),
        Some(&CoreSupportLevel::Native)
    );
    assert_eq!(codex.get(&Capability::ToolWebSearch), None);
}

#[test]
fn compare_registry_based_openai_vs_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet");
    assert!(result.is_some());
    let result = result.unwrap();
    // OpenAI has logprobs native; Claude does not
    assert!(result
        .unsupported
        .iter()
        .any(|(c, _)| *c == Capability::Logprobs));
}

#[test]
fn compare_registry_based_claude_vs_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o");
    assert!(result.is_some());
    let result = result.unwrap();
    // Claude has extended thinking native; OpenAI does not
    assert!(result
        .unsupported
        .iter()
        .any(|(c, _)| *c == Capability::ExtendedThinking));
}

#[test]
fn compare_missing_source_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("nonexistent", "openai/gpt-4o").is_none());
}

#[test]
fn compare_missing_target_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("openai/gpt-4o", "nonexistent").is_none());
}

// ===========================================================================
// 3. Feature support levels (native, emulated, unsupported)
// ===========================================================================

#[test]
fn check_capability_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert!(matches!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    ));
}

#[test]
fn check_capability_emulated() {
    let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    assert!(matches!(
        check_capability(&m, &Capability::ToolUse),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_capability_restricted() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    match check_capability(&m, &Capability::ToolBash) {
        SupportLevel::Restricted { reason } => assert!(reason.contains("sandboxed")),
        other => panic!("expected Restricted, got {other:?}"),
    }
}

#[test]
fn check_capability_explicit_unsupported() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    assert!(matches!(
        check_capability(&m, &Capability::Vision),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn check_capability_absent_from_manifest() {
    let m = manifest_from(&[]);
    let level = check_capability(&m, &Capability::Audio);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
}

#[test]
fn support_level_satisfies_native_only_for_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_satisfies_emulated_for_native_and_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_restricted_satisfies_emulated() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
    assert!(!restricted.satisfies(&MinSupport::Native));
}

#[test]
fn emulation_strategy_client_side_no_fidelity_loss() {
    assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
}

#[test]
fn emulation_strategy_server_fallback_no_fidelity_loss() {
    assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
}

#[test]
fn emulation_strategy_approximate_has_fidelity_loss() {
    assert!(EmulationStrategy::Approximate.has_fidelity_loss());
}

// ===========================================================================
// 4. Capability registry operations
// ===========================================================================

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_with_defaults_has_six_dialects() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
}

#[test]
fn registry_with_defaults_known_names() {
    let reg = CapabilityRegistry::with_defaults();
    let names = reg.names();
    assert!(names.contains(&"openai/gpt-4o"));
    assert!(names.contains(&"anthropic/claude-3.5-sonnet"));
    assert!(names.contains(&"google/gemini-1.5-pro"));
    assert!(names.contains(&"moonshot/kimi"));
    assert!(names.contains(&"openai/codex"));
    assert!(names.contains(&"github/copilot"));
}

#[test]
fn registry_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test-backend", m.clone());
    assert!(reg.contains("test-backend"));
    assert_eq!(reg.get("test-backend"), Some(&m));
}

#[test]
fn registry_unregister() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test-backend", m);
    assert!(reg.unregister("test-backend"));
    assert!(!reg.contains("test-backend"));
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_unregister_nonexistent() {
    let mut reg = CapabilityRegistry::new();
    assert!(!reg.unregister("nope"));
}

#[test]
fn registry_register_overwrites() {
    let mut reg = CapabilityRegistry::new();
    let m1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    reg.register("backend", m1);
    reg.register("backend", m2.clone());
    assert_eq!(reg.get("backend"), Some(&m2));
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_get_missing_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("missing").is_none());
}

#[test]
fn registry_contains_missing_is_false() {
    let reg = CapabilityRegistry::new();
    assert!(!reg.contains("missing"));
}

// ===========================================================================
// 5. Capability queries (does X support Y?)
// ===========================================================================

#[test]
fn query_capability_across_all_defaults() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert_eq!(results.len(), 6);
    for (_, level) in &results {
        assert!(matches!(level, SupportLevel::Native));
    }
}

#[test]
fn query_capability_logprobs_mixed_support() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Logprobs);
    let native_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .count();
    let unsupported_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Unsupported { .. }))
        .count();
    assert!(native_count > 0);
    assert!(unsupported_count > 0);
}

#[test]
fn query_capability_extended_thinking() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::ExtendedThinking);
    let native_names: Vec<&str> = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .map(|(n, _)| *n)
        .collect();
    assert!(native_names.contains(&"anthropic/claude-3.5-sonnet"));
}

#[test]
fn negotiate_by_name_openai_streaming() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
        .unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_by_name_missing_backend() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg
        .negotiate_by_name("nonexistent", &[Capability::Streaming])
        .is_none());
}

#[test]
fn negotiate_by_name_with_unsupported_cap() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::ExtendedThinking])
        .unwrap();
    assert!(!result.is_viable());
    assert!(result
        .unsupported
        .iter()
        .any(|(c, _)| *c == Capability::ExtendedThinking));
}

#[test]
fn negotiate_by_name_empty_requirements() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.negotiate_by_name("openai/gpt-4o", &[]).unwrap();
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

// ===========================================================================
// 6. Negotiation between source and target dialect
// ===========================================================================

#[test]
fn negotiate_capabilities_all_native() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(r.is_viable());
    assert_eq!(r.native.len(), 2);
    assert!(r.emulated.is_empty());
    assert!(r.unsupported.is_empty());
}

#[test]
fn negotiate_capabilities_mixed() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let r = negotiate_capabilities(
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
fn negotiate_capabilities_empty_manifest() {
    let m = CapabilityManifest::new();
    let r = negotiate_capabilities(&[Capability::Streaming], &m);
    assert!(!r.is_viable());
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn negotiate_capabilities_empty_required() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiate_with_requirements_native_met() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Native)]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_compatible());
    assert_eq!(r.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_with_requirements_emulated_when_native_required() {
    let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let reqs = require(&[(Capability::ToolUse, MinSupport::Native)]);
    let r = negotiate(&m, &reqs);
    assert!(!r.is_compatible());
    assert!(!r.unsupported.is_empty());
}

#[test]
fn negotiate_with_requirements_emulated_acceptable() {
    let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let reqs = require(&[(Capability::ToolUse, MinSupport::Emulated)]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_compatible());
    assert_eq!(r.emulated.len(), 1);
}

#[test]
fn negotiate_dialects_claude_to_openai() {
    let claude = claude_35_sonnet_manifest();
    let openai = openai_gpt4o_manifest();
    let result = negotiate_dialects("claude", &claude, "openai", &openai);
    assert_eq!(result.source, "claude");
    assert_eq!(result.target, "openai");
    assert!(
        result.losses > 0,
        "some Claude caps should be lost in OpenAI"
    );
}

#[test]
fn negotiate_dialects_openai_to_claude() {
    let openai = openai_gpt4o_manifest();
    let claude = claude_35_sonnet_manifest();
    let result = negotiate_dialects("openai", &openai, "claude", &claude);
    assert!(
        result.losses > 0,
        "some OpenAI caps should be lost in Claude"
    );
}

#[test]
fn negotiate_dialects_same_manifest_no_changes() {
    let m = openai_gpt4o_manifest();
    let result = negotiate_dialects("a", &m, "b", &m);
    assert_eq!(result.upgrades, 0);
    assert_eq!(result.downgrades, 0);
    assert_eq!(result.losses, 0);
    assert!(result.is_viable());
}

#[test]
fn negotiate_dialects_result_has_transitions() {
    let claude = claude_35_sonnet_manifest();
    let openai = openai_gpt4o_manifest();
    let result = negotiate_dialects("claude", &claude, "openai", &openai);
    assert!(!result.transitions.is_empty());
}

#[test]
fn negotiate_dialects_result_emulation_plan_nonempty_on_loss() {
    let claude = claude_35_sonnet_manifest();
    let openai = openai_gpt4o_manifest();
    let result = negotiate_dialects("claude", &claude, "openai", &openai);
    if result.losses > 0 {
        assert!(!result.emulation_plan.is_empty());
    }
}

#[test]
fn negotiate_dialects_display_format() {
    let m = openai_gpt4o_manifest();
    let result = negotiate_dialects("a", &m, "b", &m);
    let display = format!("{result}");
    assert!(display.contains("a → b"));
}

#[test]
fn negotiate_dialects_regressions_only_downgrades_and_losses() {
    let claude = claude_35_sonnet_manifest();
    let openai = openai_gpt4o_manifest();
    let result = negotiate_dialects("claude", &claude, "openai", &openai);
    for t in result.regressions() {
        assert!(matches!(
            t.kind,
            TransitionKind::Downgrade | TransitionKind::Lost
        ));
    }
}

#[test]
fn negotiate_dialects_via_registry() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_dialects("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
        .unwrap();
    assert_eq!(result.source, "openai/gpt-4o");
    assert_eq!(result.target, "anthropic/claude-3.5-sonnet");
}

#[test]
fn negotiate_dialects_via_registry_missing_source() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.negotiate_dialects("nope", "openai/gpt-4o").is_none());
}

// ===========================================================================
// 7. Graceful degradation paths
// ===========================================================================

#[test]
fn default_emulation_strategy_tool_read_is_client_side() {
    assert_eq!(
        default_emulation_strategy(&Capability::ToolRead),
        EmulationStrategy::ClientSide
    );
}

#[test]
fn default_emulation_strategy_tool_use_is_server_fallback() {
    assert_eq!(
        default_emulation_strategy(&Capability::ToolUse),
        EmulationStrategy::ServerFallback
    );
}

#[test]
fn default_emulation_strategy_vision_is_approximate() {
    assert_eq!(
        default_emulation_strategy(&Capability::Vision),
        EmulationStrategy::Approximate
    );
}

#[test]
fn every_capability_has_an_emulation_strategy() {
    for cap in all_capabilities() {
        let _ = default_emulation_strategy(&cap);
    }
}

#[test]
fn select_emulation_strategy_emulated_in_target_prefers_server() {
    let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    assert_eq!(
        select_emulation_strategy(&Capability::ToolUse, &m),
        EmulationStrategy::ServerFallback
    );
}

#[test]
fn select_emulation_strategy_restricted_in_target_prefers_server() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    assert_eq!(
        select_emulation_strategy(&Capability::ToolBash, &m),
        EmulationStrategy::ServerFallback
    );
}

#[test]
fn select_emulation_strategy_absent_falls_back_to_default() {
    let m = CapabilityManifest::new();
    assert_eq!(
        select_emulation_strategy(&Capability::ToolRead, &m),
        default_emulation_strategy(&Capability::ToolRead)
    );
}

#[test]
fn select_emulation_strategy_native_falls_back_to_default() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        select_emulation_strategy(&Capability::Streaming, &m),
        default_emulation_strategy(&Capability::Streaming)
    );
}

#[test]
fn policy_permissive_never_blocks() {
    let m = CapabilityManifest::new();
    let r = pre_negotiate(&all_capabilities(), &m);
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn policy_strict_blocks_on_any_unsupported() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_err());
}

#[test]
fn policy_best_effort_blocks_on_unsupported() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_err());
}

#[test]
fn policy_strict_passes_when_all_satisfied() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn report_mismatches_suggests_alternatives() {
    let reg = CapabilityRegistry::with_defaults();
    let m = manifest_from(&[]);
    let r = negotiate_capabilities(&[Capability::Streaming], &m);
    let mismatches = report_mismatches(&r, &reg);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].capability, Capability::Streaming);
    // All dialects support streaming natively
    assert!(!mismatches[0].alternative_backends.is_empty());
}

#[test]
fn suggest_alternatives_via_registry() {
    let reg = CapabilityRegistry::with_defaults();
    let r = negotiate_capabilities(&[Capability::ExtendedThinking], &openai_gpt4o_manifest());
    let suggestions = reg.suggest_alternatives(&r);
    assert_eq!(suggestions.len(), 1);
    assert!(suggestions[0]
        .alternative_backends
        .contains(&"anthropic/claude-3.5-sonnet".to_string()));
}

// ===========================================================================
// 8. Capability manifest generation
// ===========================================================================

#[test]
fn generate_report_fully_compatible() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_with_emulated() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![],
    );
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.emulated_count, 1);
}

#[test]
fn generate_report_incompatible() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn generate_report_details_count_matches() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![Capability::Vision],
        vec![Capability::Audio],
    );
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 4);
}

#[test]
fn generate_report_display_matches_summary() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert_eq!(format!("{report}"), report.summary);
}

#[test]
fn negotiation_result_total_correct() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Vision],
    );
    assert_eq!(r.total(), 3);
}

#[test]
fn negotiation_result_emulated_caps_extraction() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![],
    );
    let caps = r.emulated_caps();
    assert_eq!(caps.len(), 2);
    assert!(caps.contains(&Capability::ToolRead));
    assert!(caps.contains(&Capability::ToolWrite));
}

#[test]
fn negotiation_result_unsupported_caps_extraction() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    let caps = r.unsupported_caps();
    assert_eq!(caps, vec![Capability::Audio]);
}

#[test]
fn negotiation_result_warnings_only_approximate() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let warnings = r.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_display_format() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Vision],
    );
    let s = format!("{r}");
    assert!(s.contains("1 native"));
    assert!(s.contains("1 emulated"));
    assert!(s.contains("1 unsupported"));
    assert!(s.contains("not viable"));
}

#[test]
fn negotiation_result_display_viable() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let s = format!("{r}");
    assert!(s.contains("viable"));
    assert!(!s.contains("not viable"));
}

// ===========================================================================
// 9. Capability checks in runtime context
// ===========================================================================

#[test]
fn runtime_check_strict_all_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = check_runtime_capabilities(&[Capability::Streaming], &m, NegotiationPolicy::Strict);
    assert!(r.can_proceed);
    assert_eq!(r.available, vec![Capability::Streaming]);
    assert!(r.blocking.is_empty());
}

#[test]
fn runtime_check_strict_unsupported_blocks() {
    let m = CapabilityManifest::new();
    let r = check_runtime_capabilities(&[Capability::Streaming], &m, NegotiationPolicy::Strict);
    assert!(!r.can_proceed);
    assert_eq!(r.blocking.len(), 1);
}

#[test]
fn runtime_check_best_effort_unsupported_blocks() {
    let m = CapabilityManifest::new();
    let r = check_runtime_capabilities(&[Capability::Streaming], &m, NegotiationPolicy::BestEffort);
    assert!(!r.can_proceed);
}

#[test]
fn runtime_check_permissive_never_blocks() {
    let m = CapabilityManifest::new();
    let r = check_runtime_capabilities(&[Capability::Streaming], &m, NegotiationPolicy::Permissive);
    assert!(r.can_proceed);
    assert!(r.blocking.is_empty());
}

#[test]
fn runtime_check_emulated_included() {
    let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = check_runtime_capabilities(&[Capability::ToolUse], &m, NegotiationPolicy::Strict);
    assert!(r.can_proceed);
    assert_eq!(r.emulated.len(), 1);
}

#[test]
fn runtime_check_display_ready() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = check_runtime_capabilities(&[Capability::Streaming], &m, NegotiationPolicy::Strict);
    let s = format!("{r}");
    assert!(s.contains("ready"));
}

#[test]
fn runtime_check_display_blocked() {
    let m = CapabilityManifest::new();
    let r = check_runtime_capabilities(&[Capability::Streaming], &m, NegotiationPolicy::Strict);
    let s = format!("{r}");
    assert!(s.contains("blocked"));
}

#[test]
fn runtime_check_empty_required_always_proceeds() {
    let m = CapabilityManifest::new();
    let r = check_runtime_capabilities(&[], &m, NegotiationPolicy::Strict);
    assert!(r.can_proceed);
}

// ===========================================================================
// 10. Transition matrix
// ===========================================================================

#[test]
fn classify_transition_native_to_native_unchanged() {
    assert_eq!(
        classify_transition(&CoreSupportLevel::Native, &CoreSupportLevel::Native),
        TransitionKind::Unchanged
    );
}

#[test]
fn classify_transition_emulated_to_native_upgrade() {
    assert_eq!(
        classify_transition(&CoreSupportLevel::Emulated, &CoreSupportLevel::Native),
        TransitionKind::Upgrade
    );
}

#[test]
fn classify_transition_native_to_emulated_downgrade() {
    assert_eq!(
        classify_transition(&CoreSupportLevel::Native, &CoreSupportLevel::Emulated),
        TransitionKind::Downgrade
    );
}

#[test]
fn classify_transition_native_to_unsupported_lost() {
    assert_eq!(
        classify_transition(&CoreSupportLevel::Native, &CoreSupportLevel::Unsupported),
        TransitionKind::Lost
    );
}

#[test]
fn classify_transition_emulated_to_unsupported_lost() {
    assert_eq!(
        classify_transition(&CoreSupportLevel::Emulated, &CoreSupportLevel::Unsupported),
        TransitionKind::Lost
    );
}

#[test]
fn classify_transition_unsupported_to_native_upgrade() {
    // Unsupported rank=0, Native rank=3; but unsupported→native is NOT Lost because
    // r_to > 0, and r_to > r_from → Upgrade
    assert_eq!(
        classify_transition(&CoreSupportLevel::Unsupported, &CoreSupportLevel::Native),
        TransitionKind::Upgrade
    );
}

#[test]
fn classify_transition_unsupported_to_unsupported_unchanged() {
    assert_eq!(
        classify_transition(
            &CoreSupportLevel::Unsupported,
            &CoreSupportLevel::Unsupported
        ),
        TransitionKind::Unchanged
    );
}

#[test]
fn classify_transition_restricted_to_native_upgrade() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert_eq!(
        classify_transition(&restricted, &CoreSupportLevel::Native),
        TransitionKind::Upgrade
    );
}

#[test]
fn classify_transition_native_to_restricted_downgrade() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert_eq!(
        classify_transition(&CoreSupportLevel::Native, &restricted),
        TransitionKind::Downgrade
    );
}

#[test]
fn classify_transition_restricted_to_unsupported_lost() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert_eq!(
        classify_transition(&restricted, &CoreSupportLevel::Unsupported),
        TransitionKind::Lost
    );
}

#[test]
fn transition_kind_display() {
    assert_eq!(TransitionKind::Unchanged.to_string(), "unchanged");
    assert_eq!(TransitionKind::Upgrade.to_string(), "upgrade");
    assert_eq!(TransitionKind::Downgrade.to_string(), "downgrade");
    assert_eq!(TransitionKind::Lost.to_string(), "lost");
}

// ===========================================================================
// Additional cross-cutting tests
// ===========================================================================

#[test]
fn negotiate_all_capabilities_against_openai() {
    let m = openai_gpt4o_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert!(r.total() == all_capabilities().len());
    assert!(r.native.len() + r.emulated.len() + r.unsupported.len() == r.total());
}

#[test]
fn negotiate_all_capabilities_against_claude() {
    let m = claude_35_sonnet_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), all_capabilities().len());
}

#[test]
fn negotiate_all_capabilities_against_gemini() {
    let m = gemini_15_pro_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), all_capabilities().len());
}

#[test]
fn negotiate_all_capabilities_against_kimi() {
    let m = kimi_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), all_capabilities().len());
}

#[test]
fn negotiate_all_capabilities_against_codex() {
    let m = codex_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), all_capabilities().len());
}

#[test]
fn negotiate_all_capabilities_against_copilot() {
    let m = copilot_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), all_capabilities().len());
}

#[test]
fn from_simple_uses_client_side_for_emulated() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::ToolRead], vec![]);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.emulated[0].1, EmulationStrategy::ClientSide);
}

#[test]
fn from_simple_uses_not_available_for_unsupported() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    assert_eq!(r.unsupported.len(), 1);
    assert_eq!(r.unsupported[0].1, "not available");
}

#[test]
fn negotiate_dialects_skips_unsupported_source_caps() {
    let src = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Audio, CoreSupportLevel::Unsupported),
    ]);
    let tgt = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let result = negotiate_dialects("src", &src, "tgt", &tgt);
    // Audio was Unsupported in source so should be skipped
    assert!(result
        .transitions
        .iter()
        .all(|t| t.capability != Capability::Audio));
}

#[test]
fn negotiate_dialects_upgrade_counted() {
    let src = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let tgt = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    let result = negotiate_dialects("src", &src, "tgt", &tgt);
    assert_eq!(result.upgrades, 1);
    assert_eq!(result.downgrades, 0);
    assert_eq!(result.losses, 0);
}

#[test]
fn negotiate_dialects_downgrade_counted() {
    let src = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    let tgt = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let result = negotiate_dialects("src", &src, "tgt", &tgt);
    assert_eq!(result.downgrades, 1);
    assert_eq!(result.upgrades, 0);
}

#[test]
fn negotiate_dialects_loss_counted() {
    let src = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    let tgt = CapabilityManifest::new();
    let result = negotiate_dialects("src", &src, "tgt", &tgt);
    assert_eq!(result.losses, 1);
    assert!(!result.is_viable());
}

#[test]
fn capability_mismatch_display() {
    let mm = CapabilityMismatch {
        capability: Capability::Vision,
        reason: "not available".into(),
        emulation: Some(EmulationStrategy::Approximate),
        alternative_backends: vec!["openai/gpt-4o".into()],
    };
    let s = format!("{mm}");
    assert!(s.contains("Vision"));
    assert!(s.contains("not available"));
    assert!(s.contains("approximate"));
    assert!(s.contains("openai/gpt-4o"));
}

#[test]
fn capability_mismatch_display_no_emulation_no_alternatives() {
    let mm = CapabilityMismatch {
        capability: Capability::Audio,
        reason: "missing".into(),
        emulation: None,
        alternative_backends: vec![],
    };
    let s = format!("{mm}");
    assert!(s.contains("Audio"));
    assert!(s.contains("missing"));
    assert!(!s.contains("emulate"));
    assert!(!s.contains("alternatives"));
}

#[test]
fn emulation_strategy_serde_roundtrip() {
    for strategy in [
        EmulationStrategy::ClientSide,
        EmulationStrategy::ServerFallback,
        EmulationStrategy::Approximate,
    ] {
        let json = serde_json::to_string(&strategy).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, strategy);
    }
}

#[test]
fn support_level_serde_roundtrip() {
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
fn negotiation_policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn negotiation_error_display_contains_policy_name() {
    use abp_capability::negotiate::NegotiationError;
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![(Capability::Audio, "missing".into())],
        warnings: vec![],
    };
    let s = format!("{err}");
    assert!(s.contains("best-effort"));
}

#[test]
fn pre_negotiate_restricted_classified_as_emulated() {
    let m = manifest_from(&[(
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
fn registry_negotiate_dialects_returns_none_for_unknown() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg
        .negotiate_dialects("openai/gpt-4o", "nonexistent")
        .is_none());
}

#[test]
fn all_pairwise_dialect_negotiations_have_transitions() {
    let reg = CapabilityRegistry::with_defaults();
    let names = reg.names();
    for src in &names {
        for tgt in &names {
            if src == tgt {
                continue;
            }
            let result = reg.negotiate_dialects(src, tgt).unwrap();
            // Transitions should be nonempty when manifests differ
            assert!(
                !result.transitions.is_empty(),
                "{src} → {tgt} should have transitions"
            );
        }
    }
}

#[test]
fn same_dialect_negotiation_is_always_viable() {
    let reg = CapabilityRegistry::with_defaults();
    for name in reg.names() {
        let result = reg.negotiate_dialects(name, name).unwrap();
        assert!(
            result.is_viable(),
            "{name} → {name} should always be viable"
        );
        assert_eq!(result.losses, 0);
    }
}
