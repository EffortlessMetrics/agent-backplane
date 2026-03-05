#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive capability matrix tests covering all 41 Capability variants
//! across all 6 dialects (OpenAI, Claude, Gemini, Kimi, Codex, Copilot).

use abp_capability::negotiate::{apply_policy, pre_negotiate, NegotiationError, NegotiationPolicy};
use abp_capability::{
    check_capability, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    default_emulation_strategy, gemini_15_pro_manifest, generate_report, kimi_manifest,
    negotiate_capabilities, openai_gpt4o_manifest, CapabilityRegistry, CompatibilityReport,
    EmulationStrategy, NegotiationResult, SupportLevel,
};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport, CapabilityReportEntry,
    DialectSupportLevel, NegotiationRequest, NegotiationResult as CoreNegotiationResult,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// All 41 Capability variants in declaration order.
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

/// Snake_case name for a capability (matching serde rename).
fn capability_snake_name(cap: &Capability) -> String {
    serde_json::to_value(cap)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

/// Named manifest constructors for all 6 dialects.
fn dialect_manifests() -> Vec<(&'static str, CapabilityManifest)> {
    vec![
        ("openai/gpt-4o", openai_gpt4o_manifest()),
        ("anthropic/claude-3.5-sonnet", claude_35_sonnet_manifest()),
        ("google/gemini-1.5-pro", gemini_15_pro_manifest()),
        ("moonshot/kimi", kimi_manifest()),
        ("openai/codex", codex_manifest()),
        ("github/copilot", copilot_manifest()),
    ]
}

fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

/// Helper to check if a manifest entry matches the expected support level variant.
fn is_native(m: &CapabilityManifest, cap: &Capability) -> bool {
    matches!(m.get(cap), Some(CoreSupportLevel::Native))
}

fn is_emulated(m: &CapabilityManifest, cap: &Capability) -> bool {
    matches!(m.get(cap), Some(CoreSupportLevel::Emulated))
}

fn is_unsupported(m: &CapabilityManifest, cap: &Capability) -> bool {
    matches!(m.get(cap), Some(CoreSupportLevel::Unsupported))
}

fn is_absent(m: &CapabilityManifest, cap: &Capability) -> bool {
    m.get(cap).is_none()
}

// ===========================================================================
// 1. Serde round-trip for all 41 Capability variants
// ===========================================================================

#[test]
fn serde_roundtrip_all_41_capabilities() {
    let caps = all_capabilities();
    assert_eq!(caps.len(), 41, "Expected exactly 41 Capability variants");
    for cap in &caps {
        let json = serde_json::to_string(cap).unwrap();
        let parsed: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, cap, "Round-trip failed for {cap:?}");
    }
}

#[test]
fn serde_roundtrip_streaming() {
    let cap = Capability::Streaming;
    let json = serde_json::to_string(&cap).unwrap();
    assert_eq!(json, "\"streaming\"");
    let parsed: Capability = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cap);
}

#[test]
fn serde_roundtrip_tool_read() {
    let json = serde_json::to_string(&Capability::ToolRead).unwrap();
    assert_eq!(json, "\"tool_read\"");
}

#[test]
fn serde_roundtrip_tool_write() {
    let json = serde_json::to_string(&Capability::ToolWrite).unwrap();
    assert_eq!(json, "\"tool_write\"");
}

#[test]
fn serde_roundtrip_tool_edit() {
    let json = serde_json::to_string(&Capability::ToolEdit).unwrap();
    assert_eq!(json, "\"tool_edit\"");
}

#[test]
fn serde_roundtrip_tool_bash() {
    let json = serde_json::to_string(&Capability::ToolBash).unwrap();
    assert_eq!(json, "\"tool_bash\"");
}

#[test]
fn serde_roundtrip_tool_glob() {
    let json = serde_json::to_string(&Capability::ToolGlob).unwrap();
    assert_eq!(json, "\"tool_glob\"");
}

#[test]
fn serde_roundtrip_tool_grep() {
    let json = serde_json::to_string(&Capability::ToolGrep).unwrap();
    assert_eq!(json, "\"tool_grep\"");
}

#[test]
fn serde_roundtrip_tool_web_search() {
    let json = serde_json::to_string(&Capability::ToolWebSearch).unwrap();
    assert_eq!(json, "\"tool_web_search\"");
}

#[test]
fn serde_roundtrip_tool_web_fetch() {
    let json = serde_json::to_string(&Capability::ToolWebFetch).unwrap();
    assert_eq!(json, "\"tool_web_fetch\"");
}

#[test]
fn serde_roundtrip_tool_ask_user() {
    let json = serde_json::to_string(&Capability::ToolAskUser).unwrap();
    assert_eq!(json, "\"tool_ask_user\"");
}

#[test]
fn serde_roundtrip_hooks_pre_tool_use() {
    let json = serde_json::to_string(&Capability::HooksPreToolUse).unwrap();
    assert_eq!(json, "\"hooks_pre_tool_use\"");
}

#[test]
fn serde_roundtrip_hooks_post_tool_use() {
    let json = serde_json::to_string(&Capability::HooksPostToolUse).unwrap();
    assert_eq!(json, "\"hooks_post_tool_use\"");
}

#[test]
fn serde_roundtrip_session_resume() {
    let json = serde_json::to_string(&Capability::SessionResume).unwrap();
    assert_eq!(json, "\"session_resume\"");
}

#[test]
fn serde_roundtrip_session_fork() {
    let json = serde_json::to_string(&Capability::SessionFork).unwrap();
    assert_eq!(json, "\"session_fork\"");
}

#[test]
fn serde_roundtrip_checkpointing() {
    let json = serde_json::to_string(&Capability::Checkpointing).unwrap();
    assert_eq!(json, "\"checkpointing\"");
}

#[test]
fn serde_roundtrip_structured_output_json_schema() {
    let json = serde_json::to_string(&Capability::StructuredOutputJsonSchema).unwrap();
    assert_eq!(json, "\"structured_output_json_schema\"");
}

#[test]
fn serde_roundtrip_mcp_client() {
    let json = serde_json::to_string(&Capability::McpClient).unwrap();
    assert_eq!(json, "\"mcp_client\"");
}

#[test]
fn serde_roundtrip_mcp_server() {
    let json = serde_json::to_string(&Capability::McpServer).unwrap();
    assert_eq!(json, "\"mcp_server\"");
}

#[test]
fn serde_roundtrip_tool_use() {
    let json = serde_json::to_string(&Capability::ToolUse).unwrap();
    assert_eq!(json, "\"tool_use\"");
}

#[test]
fn serde_roundtrip_extended_thinking() {
    let json = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
    assert_eq!(json, "\"extended_thinking\"");
}

#[test]
fn serde_roundtrip_image_input() {
    let json = serde_json::to_string(&Capability::ImageInput).unwrap();
    assert_eq!(json, "\"image_input\"");
}

#[test]
fn serde_roundtrip_pdf_input() {
    let json = serde_json::to_string(&Capability::PdfInput).unwrap();
    assert_eq!(json, "\"pdf_input\"");
}

#[test]
fn serde_roundtrip_code_execution() {
    let json = serde_json::to_string(&Capability::CodeExecution).unwrap();
    assert_eq!(json, "\"code_execution\"");
}

#[test]
fn serde_roundtrip_logprobs() {
    let json = serde_json::to_string(&Capability::Logprobs).unwrap();
    assert_eq!(json, "\"logprobs\"");
}

#[test]
fn serde_roundtrip_seed_determinism() {
    let json = serde_json::to_string(&Capability::SeedDeterminism).unwrap();
    assert_eq!(json, "\"seed_determinism\"");
}

#[test]
fn serde_roundtrip_stop_sequences() {
    let json = serde_json::to_string(&Capability::StopSequences).unwrap();
    assert_eq!(json, "\"stop_sequences\"");
}

#[test]
fn serde_roundtrip_function_calling() {
    let json = serde_json::to_string(&Capability::FunctionCalling).unwrap();
    assert_eq!(json, "\"function_calling\"");
}

#[test]
fn serde_roundtrip_vision() {
    let json = serde_json::to_string(&Capability::Vision).unwrap();
    assert_eq!(json, "\"vision\"");
}

#[test]
fn serde_roundtrip_audio() {
    let json = serde_json::to_string(&Capability::Audio).unwrap();
    assert_eq!(json, "\"audio\"");
}

#[test]
fn serde_roundtrip_json_mode() {
    let json = serde_json::to_string(&Capability::JsonMode).unwrap();
    assert_eq!(json, "\"json_mode\"");
}

#[test]
fn serde_roundtrip_system_message() {
    let json = serde_json::to_string(&Capability::SystemMessage).unwrap();
    assert_eq!(json, "\"system_message\"");
}

#[test]
fn serde_roundtrip_temperature() {
    let json = serde_json::to_string(&Capability::Temperature).unwrap();
    assert_eq!(json, "\"temperature\"");
}

#[test]
fn serde_roundtrip_top_p() {
    let json = serde_json::to_string(&Capability::TopP).unwrap();
    assert_eq!(json, "\"top_p\"");
}

#[test]
fn serde_roundtrip_top_k() {
    let json = serde_json::to_string(&Capability::TopK).unwrap();
    assert_eq!(json, "\"top_k\"");
}

#[test]
fn serde_roundtrip_max_tokens() {
    let json = serde_json::to_string(&Capability::MaxTokens).unwrap();
    assert_eq!(json, "\"max_tokens\"");
}

#[test]
fn serde_roundtrip_frequency_penalty() {
    let json = serde_json::to_string(&Capability::FrequencyPenalty).unwrap();
    assert_eq!(json, "\"frequency_penalty\"");
}

#[test]
fn serde_roundtrip_presence_penalty() {
    let json = serde_json::to_string(&Capability::PresencePenalty).unwrap();
    assert_eq!(json, "\"presence_penalty\"");
}

#[test]
fn serde_roundtrip_cache_control() {
    let json = serde_json::to_string(&Capability::CacheControl).unwrap();
    assert_eq!(json, "\"cache_control\"");
}

#[test]
fn serde_roundtrip_batch_mode() {
    let json = serde_json::to_string(&Capability::BatchMode).unwrap();
    assert_eq!(json, "\"batch_mode\"");
}

#[test]
fn serde_roundtrip_embeddings() {
    let json = serde_json::to_string(&Capability::Embeddings).unwrap();
    assert_eq!(json, "\"embeddings\"");
}

#[test]
fn serde_roundtrip_image_generation() {
    let json = serde_json::to_string(&Capability::ImageGeneration).unwrap();
    assert_eq!(json, "\"image_generation\"");
}

// ===========================================================================
// 2. Capability::all() equivalent — verify exactly 41 unique variants
// ===========================================================================

#[test]
fn all_capabilities_count_is_41() {
    let caps = all_capabilities();
    assert_eq!(caps.len(), 41);
}

#[test]
fn all_capabilities_are_unique() {
    let caps = all_capabilities();
    let mut set = std::collections::BTreeSet::new();
    for cap in &caps {
        assert!(set.insert(cap.clone()), "Duplicate capability: {cap:?}");
    }
    assert_eq!(set.len(), 41);
}

#[test]
fn all_capabilities_names_are_snake_case() {
    for cap in &all_capabilities() {
        let name = capability_snake_name(cap);
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
            "Capability name is not snake_case: {name}"
        );
        assert!(!name.starts_with('_'), "Name starts with _: {name}");
        assert!(!name.ends_with('_'), "Name ends with _: {name}");
        assert!(
            !name.contains("__"),
            "Name contains double underscore: {name}"
        );
    }
}

#[test]
fn all_capability_names_are_distinct() {
    let names: Vec<String> = all_capabilities()
        .iter()
        .map(capability_snake_name)
        .collect();
    let mut seen = std::collections::HashSet::new();
    for name in &names {
        assert!(seen.insert(name.clone()), "Duplicate name: {name}");
    }
}

// ===========================================================================
// 3. SupportLevel::satisfies() exhaustive truth table
// ===========================================================================

#[test]
fn satisfies_native_requires_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_emulated_fails_native_requirement() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_restricted_fails_native_requirement() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_unsupported_fails_native_requirement() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_native_meets_emulated_requirement() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn satisfies_emulated_meets_emulated_requirement() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn satisfies_restricted_meets_emulated_requirement() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn satisfies_unsupported_fails_emulated_requirement() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// 4. Default support levels per dialect×capability
// ===========================================================================

#[test]
fn openai_manifest_has_expected_native_caps() {
    let m = openai_gpt4o_manifest();
    assert!(is_native(&m, &Capability::Streaming));
    assert!(is_native(&m, &Capability::ToolUse));
    assert!(is_native(&m, &Capability::FunctionCalling));
    assert!(is_native(&m, &Capability::Vision));
    assert!(is_native(&m, &Capability::SystemMessage));
    assert!(is_native(&m, &Capability::Temperature));
    assert!(is_native(&m, &Capability::BatchMode));
}

#[test]
fn openai_manifest_unsupported_caps() {
    let m = openai_gpt4o_manifest();
    assert!(is_unsupported(&m, &Capability::TopK));
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
    assert!(is_unsupported(&m, &Capability::CacheControl));
}

#[test]
fn claude_manifest_has_expected_native_caps() {
    let m = claude_35_sonnet_manifest();
    assert!(is_native(&m, &Capability::Streaming));
    assert!(is_native(&m, &Capability::ToolUse));
    assert!(is_native(&m, &Capability::PdfInput));
    assert!(is_native(&m, &Capability::ExtendedThinking));
    assert!(is_native(&m, &Capability::CacheControl));
    assert!(is_native(&m, &Capability::TopK));
}

#[test]
fn claude_manifest_emulated_caps() {
    let m = claude_35_sonnet_manifest();
    assert!(is_emulated(&m, &Capability::FunctionCalling));
    assert!(is_emulated(&m, &Capability::StructuredOutputJsonSchema));
    assert!(is_emulated(&m, &Capability::JsonMode));
}

#[test]
fn claude_manifest_unsupported_caps() {
    let m = claude_35_sonnet_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
    assert!(is_unsupported(&m, &Capability::Logprobs));
    assert!(is_unsupported(&m, &Capability::SeedDeterminism));
}

#[test]
fn gemini_manifest_has_expected_native_caps() {
    let m = gemini_15_pro_manifest();
    assert!(is_native(&m, &Capability::Streaming));
    assert!(is_native(&m, &Capability::ToolUse));
    assert!(is_native(&m, &Capability::PdfInput));
    assert!(is_native(&m, &Capability::CodeExecution));
    assert!(is_native(&m, &Capability::CacheControl));
}

#[test]
fn gemini_manifest_unsupported_caps() {
    let m = gemini_15_pro_manifest();
    assert!(is_unsupported(&m, &Capability::BatchMode));
    assert!(is_unsupported(&m, &Capability::Logprobs));
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
}

#[test]
fn kimi_manifest_has_expected_native_caps() {
    let m = kimi_manifest();
    assert!(is_native(&m, &Capability::Streaming));
    assert!(is_native(&m, &Capability::ToolUse));
    assert!(is_native(&m, &Capability::ImageInput));
    assert!(is_native(&m, &Capability::FrequencyPenalty));
}

#[test]
fn kimi_manifest_unsupported_caps() {
    let m = kimi_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
    assert!(is_unsupported(&m, &Capability::CacheControl));
}

#[test]
fn codex_manifest_has_tool_caps_native() {
    let m = codex_manifest();
    assert!(is_native(&m, &Capability::ToolRead));
    assert!(is_native(&m, &Capability::ToolWrite));
    assert!(is_native(&m, &Capability::ToolEdit));
    assert!(is_native(&m, &Capability::ToolBash));
    assert!(is_native(&m, &Capability::ToolGlob));
    assert!(is_native(&m, &Capability::ToolGrep));
}

#[test]
fn codex_manifest_unsupported_caps() {
    let m = codex_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
    assert!(is_unsupported(&m, &Capability::TopK));
}

#[test]
fn copilot_manifest_has_tool_caps_native() {
    let m = copilot_manifest();
    assert!(is_native(&m, &Capability::ToolRead));
    assert!(is_native(&m, &Capability::ToolWrite));
    assert!(is_native(&m, &Capability::ToolEdit));
    assert!(is_native(&m, &Capability::ToolBash));
    assert!(is_native(&m, &Capability::ToolWebSearch));
    assert!(is_native(&m, &Capability::ToolWebFetch));
    assert!(is_native(&m, &Capability::ToolAskUser));
}

#[test]
fn copilot_manifest_unsupported_caps() {
    let m = copilot_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
    assert!(is_unsupported(&m, &Capability::Logprobs));
    assert!(is_unsupported(&m, &Capability::BatchMode));
}

// ===========================================================================
// 5. Every capability checked against every dialect manifest
// ===========================================================================

#[test]
fn check_capability_all_41_against_openai() {
    let m = openai_gpt4o_manifest();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        // Just verify it returns a valid SupportLevel without panic
        let _ = format!("{level}");
    }
}

#[test]
fn check_capability_all_41_against_claude() {
    let m = claude_35_sonnet_manifest();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        let _ = format!("{level}");
    }
}

#[test]
fn check_capability_all_41_against_gemini() {
    let m = gemini_15_pro_manifest();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        let _ = format!("{level}");
    }
}

#[test]
fn check_capability_all_41_against_kimi() {
    let m = kimi_manifest();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        let _ = format!("{level}");
    }
}

#[test]
fn check_capability_all_41_against_codex() {
    let m = codex_manifest();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        let _ = format!("{level}");
    }
}

#[test]
fn check_capability_all_41_against_copilot() {
    let m = copilot_manifest();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        let _ = format!("{level}");
    }
}

#[test]
fn check_capability_absent_cap_is_unsupported() {
    let m = CapabilityManifest::new();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        assert!(
            matches!(level, SupportLevel::Unsupported { .. }),
            "Expected Unsupported for {cap:?} in empty manifest, got {level}"
        );
    }
}

// ===========================================================================
// 6. Emulation strategy coverage for all 41 capabilities
// ===========================================================================

#[test]
fn emulation_strategy_covers_all_41() {
    for cap in &all_capabilities() {
        let strategy = default_emulation_strategy(cap);
        match strategy {
            EmulationStrategy::ClientSide
            | EmulationStrategy::ServerFallback
            | EmulationStrategy::Approximate => {}
        }
    }
}

#[test]
fn emulation_strategy_client_side_caps() {
    let client_side = [
        Capability::StructuredOutputJsonSchema,
        Capability::JsonMode,
        Capability::PdfInput,
        Capability::CodeExecution,
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
        Capability::Checkpointing,
    ];
    for cap in &client_side {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ClientSide,
            "Expected ClientSide for {cap:?}"
        );
    }
}

#[test]
fn emulation_strategy_server_fallback_caps() {
    let server_fallback = [
        Capability::FunctionCalling,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::BatchMode,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::McpClient,
        Capability::McpServer,
        Capability::SystemMessage,
    ];
    for cap in &server_fallback {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ServerFallback,
            "Expected ServerFallback for {cap:?}"
        );
    }
}

#[test]
fn emulation_strategy_approximate_caps() {
    let approximate = [
        Capability::Vision,
        Capability::ImageInput,
        Capability::Audio,
        Capability::ImageGeneration,
        Capability::Embeddings,
        Capability::CacheControl,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::Streaming,
        Capability::StopSequences,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
    ];
    for cap in &approximate {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::Approximate,
            "Expected Approximate for {cap:?}"
        );
    }
}

#[test]
fn emulation_fidelity_loss_only_on_approximate() {
    for cap in &all_capabilities() {
        let strategy = default_emulation_strategy(cap);
        if matches!(strategy, EmulationStrategy::Approximate) {
            assert!(strategy.has_fidelity_loss());
        } else {
            assert!(!strategy.has_fidelity_loss());
        }
    }
}

// ===========================================================================
// 7. CapabilityRegistry operations
// ===========================================================================

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_with_defaults_has_6_dialects() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
    assert!(!reg.is_empty());
}

#[test]
fn registry_with_defaults_contains_all_dialect_names() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    assert!(reg.contains("google/gemini-1.5-pro"));
    assert!(reg.contains("moonshot/kimi"));
    assert!(reg.contains("openai/codex"));
    assert!(reg.contains("github/copilot"));
}

#[test]
fn registry_names_returns_all_six() {
    let reg = CapabilityRegistry::with_defaults();
    let names = reg.names();
    assert_eq!(names.len(), 6);
}

#[test]
fn registry_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test-backend", m.clone());
    assert!(reg.contains("test-backend"));
    let got = reg.get("test-backend").unwrap();
    assert!(is_native(got, &Capability::Streaming));
}

#[test]
fn registry_unregister() {
    let mut reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.unregister("openai/gpt-4o"));
    assert!(!reg.contains("openai/gpt-4o"));
    assert_eq!(reg.len(), 5);
}

#[test]
fn registry_unregister_nonexistent_returns_false() {
    let mut reg = CapabilityRegistry::new();
    assert!(!reg.unregister("nonexistent"));
}

#[test]
fn registry_get_nonexistent_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_query_capability_streaming() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert_eq!(results.len(), 6);
    for (name, level) in &results {
        assert!(
            matches!(level, SupportLevel::Native),
            "Expected Native streaming for {name}, got {level}"
        );
    }
}

#[test]
fn registry_query_capability_audio() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Audio);
    let native_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .count();
    // Only openai and gemini have native audio
    assert_eq!(native_count, 2);
}

#[test]
fn registry_negotiate_by_name_known() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name(
            "openai/gpt-4o",
            &[Capability::Streaming, Capability::ToolUse],
        )
        .unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native.len(), 2);
}

#[test]
fn registry_negotiate_by_name_unknown() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg
        .negotiate_by_name("unknown/backend", &[Capability::Streaming])
        .is_none());
}

#[test]
fn registry_compare_openai_to_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
        .unwrap();
    // Audio is native in openai, unsupported in claude → some unsupported
    assert!(!result.unsupported.is_empty());
}

#[test]
fn registry_compare_unknown_source() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("unknown", "openai/gpt-4o").is_none());
}

#[test]
fn registry_compare_unknown_target() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("openai/gpt-4o", "unknown").is_none());
}

// ===========================================================================
// 8. CapabilityReport generation
// ===========================================================================

#[test]
fn capability_report_all_native() {
    let report = CapabilityReport {
        source_dialect: "openai".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        }],
    };
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
    assert!(report.emulated_capabilities().is_empty());
    assert!(report.unsupported_capabilities().is_empty());
}

#[test]
fn capability_report_mixed() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::PdfInput,
                support: DialectSupportLevel::Emulated {
                    detail: "converted".into(),
                },
            },
            CapabilityReportEntry {
                capability: Capability::ExtendedThinking,
                support: DialectSupportLevel::Unsupported {
                    reason: "not available".into(),
                },
            },
        ],
    };
    assert!(!report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
    assert_eq!(report.emulated_capabilities().len(), 1);
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

#[test]
fn capability_report_to_receipt_metadata() {
    let report = CapabilityReport {
        source_dialect: "openai".into(),
        target_dialect: "gemini".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        }],
    };
    let meta = report.to_receipt_metadata();
    assert!(meta.is_object());
    assert_eq!(meta["source_dialect"], "openai");
    assert_eq!(meta["target_dialect"], "gemini");
}

#[test]
fn capability_report_empty_entries_is_satisfiable() {
    let report = CapabilityReport {
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        entries: vec![],
    };
    assert!(report.all_satisfiable());
}

// ===========================================================================
// 9. CapabilityDiff computation
// ===========================================================================

#[test]
fn diff_identical_manifests_is_empty() {
    let m = openai_gpt4o_manifest();
    let diff = CapabilityDiff::diff(&m, &m);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.upgraded.is_empty());
    assert!(diff.downgraded.is_empty());
}

#[test]
fn diff_empty_to_full_shows_all_added() {
    let old = CapabilityManifest::new();
    let new = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.added.len(), 2);
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_full_to_empty_shows_all_removed() {
    let old = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let new = CapabilityManifest::new();
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed.len(), 2);
}

#[test]
fn diff_upgrade_emulated_to_native() {
    let old = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let new = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
    assert!(diff.downgraded.is_empty());
    let (cap, old_lvl, new_lvl) = &diff.upgraded[0];
    assert_eq!(cap, &Capability::Streaming);
    assert!(matches!(old_lvl, CoreSupportLevel::Emulated));
    assert!(matches!(new_lvl, CoreSupportLevel::Native));
}

#[test]
fn diff_downgrade_native_to_unsupported() {
    let old = make_manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
    let new = make_manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.upgraded.is_empty());
    assert_eq!(diff.downgraded.len(), 1);
}

#[test]
fn diff_openai_to_claude() {
    let diff = CapabilityDiff::diff(&openai_gpt4o_manifest(), &claude_35_sonnet_manifest());
    // Some caps added (e.g. TopK native in Claude, absent in openai is native too actually... let's just check non-empty)
    let total_changes =
        diff.added.len() + diff.removed.len() + diff.upgraded.len() + diff.downgraded.len();
    assert!(total_changes > 0, "Expected some differences");
}

#[test]
fn diff_upgrade_restricted_to_native() {
    let old = make_manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let new = make_manifest(&[(Capability::ToolBash, CoreSupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
}

#[test]
fn diff_same_level_no_change() {
    let old = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let new = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.upgraded.is_empty());
    assert!(diff.downgraded.is_empty());
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
}

// ===========================================================================
// 10. NegotiationPolicy outcomes
// ===========================================================================

#[test]
fn policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn policy_strict_all_native_passes() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn policy_strict_unsupported_fails() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
    assert_eq!(err.unsupported.len(), 1);
}

#[test]
fn policy_strict_emulated_passes() {
    let m = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn policy_best_effort_all_native_passes() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
}

#[test]
fn policy_best_effort_unsupported_fails() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[Capability::Vision], &m);
    let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::BestEffort);
}

#[test]
fn policy_permissive_always_ok_with_unsupported() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert!(!r.is_viable());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn policy_permissive_always_ok_with_native() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn policy_permissive_empty_requirements_ok() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn policy_strict_restricted_passes() {
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
fn policy_serde_roundtrip_strict() {
    let p = NegotiationPolicy::Strict;
    let json = serde_json::to_string(&p).unwrap();
    let parsed: NegotiationPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, p);
}

#[test]
fn policy_serde_roundtrip_best_effort() {
    let p = NegotiationPolicy::BestEffort;
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, "\"best_effort\"");
    let parsed: NegotiationPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, p);
}

#[test]
fn policy_serde_roundtrip_permissive() {
    let p = NegotiationPolicy::Permissive;
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, "\"permissive\"");
    let parsed: NegotiationPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, p);
}

#[test]
fn policy_display_strict() {
    assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
}

#[test]
fn policy_display_best_effort() {
    assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
}

#[test]
fn policy_display_permissive() {
    assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
}

// ===========================================================================
// 11. Cross-dialect capability comparison
// ===========================================================================

#[test]
fn cross_dialect_openai_vs_claude_streaming_native_both() {
    let o = openai_gpt4o_manifest();
    let c = claude_35_sonnet_manifest();
    assert!(is_native(&o, &Capability::Streaming));
    assert!(is_native(&c, &Capability::Streaming));
}

#[test]
fn cross_dialect_audio_openai_native_claude_unsupported() {
    let o = openai_gpt4o_manifest();
    let c = claude_35_sonnet_manifest();
    assert!(is_native(&o, &Capability::Audio));
    assert!(is_unsupported(&c, &Capability::Audio));
}

#[test]
fn cross_dialect_extended_thinking_claude_native_openai_unsupported() {
    let o = openai_gpt4o_manifest();
    let c = claude_35_sonnet_manifest();
    assert!(is_native(&c, &Capability::ExtendedThinking));
    assert!(is_unsupported(&o, &Capability::ExtendedThinking));
}

#[test]
fn cross_dialect_codex_vs_copilot_tool_coverage() {
    let codex = codex_manifest();
    let copilot = copilot_manifest();
    for cap in &[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ] {
        assert!(is_native(&codex, cap));
        assert!(is_native(&copilot, cap));
    }
}

#[test]
fn cross_dialect_copilot_has_web_tools_codex_does_not() {
    let codex = codex_manifest();
    let copilot = copilot_manifest();
    assert!(is_native(&copilot, &Capability::ToolWebSearch));
    assert!(is_native(&copilot, &Capability::ToolWebFetch));
    assert!(is_native(&copilot, &Capability::ToolAskUser));
    assert!(is_absent(&codex, &Capability::ToolWebSearch));
    assert!(is_absent(&codex, &Capability::ToolWebFetch));
    assert!(is_absent(&codex, &Capability::ToolAskUser));
}

#[test]
fn cross_dialect_gemini_pdf_native_openai_unsupported() {
    let g = gemini_15_pro_manifest();
    let o = openai_gpt4o_manifest();
    assert!(is_native(&g, &Capability::PdfInput));
    assert!(is_unsupported(&o, &Capability::PdfInput));
}

#[test]
fn cross_dialect_all_six_have_streaming() {
    for (name, m) in &dialect_manifests() {
        assert!(
            is_native(m, &Capability::Streaming),
            "{name} should have native streaming"
        );
    }
}

#[test]
fn cross_dialect_all_six_have_tool_use() {
    for (name, m) in &dialect_manifests() {
        assert!(
            is_native(m, &Capability::ToolUse),
            "{name} should have native tool_use"
        );
    }
}

// ===========================================================================
// 12. NegotiationResult operations
// ===========================================================================

#[test]
fn negotiation_result_is_viable_when_no_unsupported() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    assert!(r.is_viable());
    assert!(r.is_compatible());
}

#[test]
fn negotiation_result_not_viable_with_unsupported() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::Audio],
    );
    assert!(!r.is_viable());
}

#[test]
fn negotiation_result_total() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Audio],
    );
    assert_eq!(r.total(), 3);
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
fn negotiation_result_emulated_caps_extraction() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![],
    );
    let emulated = r.emulated_caps();
    assert_eq!(emulated.len(), 2);
}

#[test]
fn negotiation_result_unsupported_caps_extraction() {
    let r =
        NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio, Capability::Vision]);
    let unsupported = r.unsupported_caps();
    assert_eq!(unsupported.len(), 2);
}

#[test]
fn negotiation_result_display_viable() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let display = format!("{r}");
    assert!(display.contains("viable"));
    assert!(display.contains("1 native"));
}

#[test]
fn negotiation_result_display_not_viable() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    let display = format!("{r}");
    assert!(display.contains("not viable"));
}

// ===========================================================================
// 13. CompatibilityReport (generate_report)
// ===========================================================================

#[test]
fn generate_report_all_native_compatible() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_with_unsupported_incompatible() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 1);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn generate_report_details_count_matches() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Audio],
    );
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn generate_report_display_equals_summary() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert_eq!(format!("{report}"), report.summary);
}

// ===========================================================================
// 14. negotiate_capabilities for full dialect manifests
// ===========================================================================

#[test]
fn negotiate_all_41_against_openai() {
    let m = openai_gpt4o_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), 41);
}

#[test]
fn negotiate_all_41_against_claude() {
    let m = claude_35_sonnet_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), 41);
}

#[test]
fn negotiate_all_41_against_gemini() {
    let m = gemini_15_pro_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), 41);
}

#[test]
fn negotiate_all_41_against_kimi() {
    let m = kimi_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), 41);
}

#[test]
fn negotiate_all_41_against_codex() {
    let m = codex_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), 41);
}

#[test]
fn negotiate_all_41_against_copilot() {
    let m = copilot_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert_eq!(r.total(), 41);
}

// ===========================================================================
// 15. SupportLevel serde
// ===========================================================================

#[test]
fn core_support_level_serde_native() {
    let lvl = CoreSupportLevel::Native;
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: CoreSupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, CoreSupportLevel::Native));
}

#[test]
fn core_support_level_serde_emulated() {
    let lvl = CoreSupportLevel::Emulated;
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: CoreSupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, CoreSupportLevel::Emulated));
}

#[test]
fn core_support_level_serde_unsupported() {
    let lvl = CoreSupportLevel::Unsupported;
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: CoreSupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, CoreSupportLevel::Unsupported));
}

#[test]
fn core_support_level_serde_restricted() {
    let lvl = CoreSupportLevel::Restricted {
        reason: "policy".into(),
    };
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: CoreSupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, CoreSupportLevel::Restricted { .. }));
}

// ===========================================================================
// 16. Core negotiator (CapabilityNegotiator from abp_core::negotiate)
// ===========================================================================

#[test]
fn core_negotiator_all_satisfied() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolUse],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Emulated,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 2);
    assert!(result.unsatisfied.is_empty());
}

#[test]
fn core_negotiator_preferred_bonus() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Vision, Capability::Audio],
        minimum_support: CoreSupportLevel::Emulated,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
    assert_eq!(result.bonus.len(), 1);
    assert_eq!(result.bonus[0], Capability::Vision);
}

#[test]
fn core_negotiator_unsatisfied() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Audio],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied.len(), 1);
}

#[test]
fn core_negotiator_best_match_selects_highest_score() {
    let m1 = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Vision],
        minimum_support: CoreSupportLevel::Emulated,
    };
    let manifests = vec![("backend-a", m1), ("backend-b", m2)];
    let (name, result) = CapabilityNegotiator::best_match(&req, &manifests).unwrap();
    assert_eq!(name, "backend-b");
    assert_eq!(result.bonus.len(), 1);
}

#[test]
fn core_negotiator_best_match_none_compatible() {
    let m = make_manifest(&[]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let manifests = vec![("empty", m)];
    assert!(CapabilityNegotiator::best_match(&req, &manifests).is_none());
}

// ===========================================================================
// 17. DialectSupportLevel serde
// ===========================================================================

#[test]
fn dialect_support_level_serde_native() {
    let lvl = DialectSupportLevel::Native;
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, DialectSupportLevel::Native);
}

#[test]
fn dialect_support_level_serde_emulated() {
    let lvl = DialectSupportLevel::Emulated {
        detail: "polyfill".into(),
    };
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, DialectSupportLevel::Emulated { .. }));
}

#[test]
fn dialect_support_level_serde_unsupported() {
    let lvl = DialectSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&lvl).unwrap();
    let parsed: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, DialectSupportLevel::Unsupported { .. }));
}

// ===========================================================================
// 18. Manifest size sanity for all 6 dialects
// ===========================================================================

#[test]
fn manifest_sizes_are_reasonable() {
    let manifests = dialect_manifests();
    for (name, m) in &manifests {
        assert!(
            m.len() >= 20,
            "{name} manifest unexpectedly small: {} entries",
            m.len()
        );
        assert!(
            m.len() <= 41,
            "{name} manifest has more entries than Capability variants: {}",
            m.len()
        );
    }
}

// ===========================================================================
// 19. NegotiationError Display
// ===========================================================================

#[test]
fn negotiation_error_display_includes_policy() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Audio, "no audio".into())],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
}

#[test]
fn negotiation_error_display_includes_count() {
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![
            (Capability::Audio, "no audio".into()),
            (Capability::Vision, "no vision".into()),
        ],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("2 unsupported"));
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
// 20. EmulationStrategy Display and serde
// ===========================================================================

#[test]
fn emulation_strategy_display() {
    assert_eq!(
        EmulationStrategy::ClientSide.to_string(),
        "client-side emulation"
    );
    assert_eq!(
        EmulationStrategy::ServerFallback.to_string(),
        "server fallback"
    );
    assert_eq!(EmulationStrategy::Approximate.to_string(), "approximate");
}

#[test]
fn emulation_strategy_serde_roundtrip() {
    for strategy in &[
        EmulationStrategy::ClientSide,
        EmulationStrategy::ServerFallback,
        EmulationStrategy::Approximate,
    ] {
        let json = serde_json::to_string(strategy).unwrap();
        let parsed: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, strategy);
    }
}

// ===========================================================================
// 21. Edge cases
// ===========================================================================

#[test]
fn negotiate_empty_requirements_against_full_manifest() {
    let m = openai_gpt4o_manifest();
    let r = negotiate_capabilities(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiate_empty_manifest_all_41_unsupported() {
    let m = CapabilityManifest::new();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert!(!r.is_viable());
    assert_eq!(r.unsupported.len(), 41);
    assert_eq!(r.native.len(), 0);
    assert_eq!(r.emulated.len(), 0);
}

#[test]
fn negotiate_duplicate_capabilities() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Streaming], &m);
    assert_eq!(r.native.len(), 2);
    assert_eq!(r.total(), 2);
}

#[test]
fn capability_ord_is_deterministic() {
    let mut caps = all_capabilities();
    let caps2 = all_capabilities();
    caps.sort();
    let mut caps3 = caps2.clone();
    caps3.sort();
    assert_eq!(caps, caps3);
}

#[test]
fn capability_manifest_is_btreemap_sorted() {
    let m = openai_gpt4o_manifest();
    let keys: Vec<_> = m.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn support_level_display() {
    let native = SupportLevel::Native;
    assert_eq!(format!("{native}"), "native");

    let emulated = SupportLevel::Emulated {
        method: "adapter".into(),
    };
    assert!(format!("{emulated}").contains("emulated"));

    let restricted = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(format!("{restricted}").contains("restricted"));

    let unsupported = SupportLevel::Unsupported {
        reason: "missing".into(),
    };
    assert!(format!("{unsupported}").contains("unsupported"));
}
