// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for SDK model catalogs, model name resolution,
//! and capability detection per model.

use abp_capability::{
    CapabilityRegistry, NegotiationResult, SupportLevel, check_capability,
    claude_35_sonnet_manifest, codex_manifest, copilot_manifest, gemini_15_pro_manifest,
    generate_report, kimi_manifest, negotiate_capabilities, openai_gpt4o_manifest,
};
use abp_core::{Capability, SupportLevel as CoreSupportLevel};
use abp_dialect::Dialect;
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use abp_projection::selection::{ModelCandidate, ModelSelector, SelectionStrategy};
use std::collections::BTreeMap;

// ── Helpers ─────────────────────────────────────────────────────────────

fn has_native(manifest: &BTreeMap<Capability, CoreSupportLevel>, cap: &Capability) -> bool {
    matches!(manifest.get(cap), Some(CoreSupportLevel::Native))
}

fn has_emulated(manifest: &BTreeMap<Capability, CoreSupportLevel>, cap: &Capability) -> bool {
    matches!(manifest.get(cap), Some(CoreSupportLevel::Emulated))
}

fn is_unsupported(manifest: &BTreeMap<Capability, CoreSupportLevel>, cap: &Capability) -> bool {
    matches!(
        manifest.get(cap),
        Some(CoreSupportLevel::Unsupported) | None
    )
}

/// Resolve a model name to its dialect.
fn resolve_dialect(model: &str) -> Option<Dialect> {
    let lower = model.to_lowercase();
    if lower.starts_with("gpt-") || lower.starts_with("chatgpt-") || lower.starts_with("o1") {
        return Some(Dialect::OpenAi);
    }
    if lower.starts_with("claude-") {
        return Some(Dialect::Claude);
    }
    if lower.starts_with("gemini-") || lower.starts_with("models/gemini-") {
        return Some(Dialect::Gemini);
    }
    if lower.starts_with("codex") || lower.starts_with("o3") || lower.starts_with("o4") {
        return Some(Dialect::Codex);
    }
    if lower.starts_with("moonshot-") || lower.starts_with("kimi") {
        return Some(Dialect::Kimi);
    }
    if lower.starts_with("copilot-") {
        return Some(Dialect::Copilot);
    }
    None
}

/// Resolve a model alias to its canonical name.
fn resolve_alias(alias: &str) -> &str {
    match alias.to_lowercase().as_str() {
        "gpt4" | "gpt-4-latest" => "gpt-4",
        "gpt4o" | "gpt-4o-latest" => "gpt-4o",
        "gpt35" | "gpt-3.5" => "gpt-3.5-turbo",
        "claude-sonnet" | "sonnet" => "claude-3.5-sonnet",
        "claude-haiku" | "haiku" => "claude-3-haiku",
        "claude-opus" | "opus" => "claude-3-opus",
        "gemini-pro" => "gemini-1.5-pro",
        "gemini-flash" => "gemini-1.5-flash",
        _ => alias,
    }
}

/// Resolve a date-versioned model to its base model.
fn resolve_versioned(model: &str) -> &str {
    // Strip known date suffixes like -0613, -1106, -20240620, etc.
    let parts: Vec<&str> = model.rsplitn(2, '-').collect();
    if parts.len() == 2 {
        let suffix = parts[0];
        // Date suffixes: 4 digits (MMDD) or 8 digits (YYYYMMDD)
        if (suffix.len() == 4 || suffix.len() == 8) && suffix.chars().all(|c| c.is_ascii_digit()) {
            return parts[1];
        }
    }
    model
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI models — capabilities per model
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_gpt4o_supports_streaming() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::Streaming));
}

#[test]
fn openai_gpt4o_supports_tool_use() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::ToolUse));
}

#[test]
fn openai_gpt4o_supports_function_calling() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::FunctionCalling));
}

#[test]
fn openai_gpt4o_supports_vision() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::Vision));
}

#[test]
fn openai_gpt4o_supports_audio() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::Audio));
}

#[test]
fn openai_gpt4o_supports_json_mode() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::JsonMode));
}

#[test]
fn openai_gpt4o_supports_structured_output() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::StructuredOutputJsonSchema));
}

#[test]
fn openai_gpt4o_supports_logprobs() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::Logprobs));
}

#[test]
fn openai_gpt4o_supports_seed_determinism() {
    let m = openai_gpt4o_manifest();
    assert!(has_native(&m, &Capability::SeedDeterminism));
}

#[test]
fn openai_gpt4o_does_not_support_extended_thinking() {
    let m = openai_gpt4o_manifest();
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
}

#[test]
fn openai_gpt4o_does_not_support_cache_control() {
    let m = openai_gpt4o_manifest();
    assert!(is_unsupported(&m, &Capability::CacheControl));
}

#[test]
fn openai_gpt4_resolves_to_openai_dialect() {
    assert_eq!(resolve_dialect("gpt-4"), Some(Dialect::OpenAi));
}

#[test]
fn openai_gpt4o_resolves_to_openai_dialect() {
    assert_eq!(resolve_dialect("gpt-4o"), Some(Dialect::OpenAi));
}

#[test]
fn openai_gpt35_turbo_resolves_to_openai_dialect() {
    assert_eq!(resolve_dialect("gpt-3.5-turbo"), Some(Dialect::OpenAi));
}

#[test]
fn openai_o1_preview_resolves_to_openai_dialect() {
    assert_eq!(resolve_dialect("o1-preview"), Some(Dialect::OpenAi));
}

#[test]
fn openai_o1_mini_resolves_to_openai_dialect() {
    assert_eq!(resolve_dialect("o1-mini"), Some(Dialect::OpenAi));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude models — capabilities per model
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_35_sonnet_supports_streaming() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::Streaming));
}

#[test]
fn claude_35_sonnet_supports_tool_use() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::ToolUse));
}

#[test]
fn claude_35_sonnet_supports_vision() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::Vision));
}

#[test]
fn claude_35_sonnet_supports_pdf_input() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::PdfInput));
}

#[test]
fn claude_35_sonnet_supports_extended_thinking() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::ExtendedThinking));
}

#[test]
fn claude_35_sonnet_supports_cache_control() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::CacheControl));
}

#[test]
fn claude_35_sonnet_supports_top_k() {
    let m = claude_35_sonnet_manifest();
    assert!(has_native(&m, &Capability::TopK));
}

#[test]
fn claude_35_sonnet_emulates_function_calling() {
    let m = claude_35_sonnet_manifest();
    assert!(has_emulated(&m, &Capability::FunctionCalling));
}

#[test]
fn claude_35_sonnet_does_not_support_audio() {
    let m = claude_35_sonnet_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
}

#[test]
fn claude_35_sonnet_does_not_support_logprobs() {
    let m = claude_35_sonnet_manifest();
    assert!(is_unsupported(&m, &Capability::Logprobs));
}

#[test]
fn claude_model_resolves_to_claude_dialect() {
    assert_eq!(resolve_dialect("claude-3.5-sonnet"), Some(Dialect::Claude));
}

#[test]
fn claude_haiku_resolves_to_claude_dialect() {
    assert_eq!(resolve_dialect("claude-3-haiku"), Some(Dialect::Claude));
}

#[test]
fn claude_opus_resolves_to_claude_dialect() {
    assert_eq!(resolve_dialect("claude-3-opus"), Some(Dialect::Claude));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Gemini models — capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_15_pro_supports_streaming() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::Streaming));
}

#[test]
fn gemini_15_pro_supports_tool_use() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::ToolUse));
}

#[test]
fn gemini_15_pro_supports_vision() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::Vision));
}

#[test]
fn gemini_15_pro_supports_audio() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::Audio));
}

#[test]
fn gemini_15_pro_supports_pdf_input() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::PdfInput));
}

#[test]
fn gemini_15_pro_supports_code_execution() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::CodeExecution));
}

#[test]
fn gemini_15_pro_supports_top_k() {
    let m = gemini_15_pro_manifest();
    assert!(has_native(&m, &Capability::TopK));
}

#[test]
fn gemini_15_pro_does_not_support_batch_mode() {
    let m = gemini_15_pro_manifest();
    assert!(is_unsupported(&m, &Capability::BatchMode));
}

#[test]
fn gemini_15_pro_does_not_support_extended_thinking() {
    let m = gemini_15_pro_manifest();
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
}

#[test]
fn gemini_model_resolves_to_gemini_dialect() {
    assert_eq!(resolve_dialect("gemini-1.5-pro"), Some(Dialect::Gemini));
}

#[test]
fn gemini_flash_resolves_to_gemini_dialect() {
    assert_eq!(resolve_dialect("gemini-1.5-flash"), Some(Dialect::Gemini));
}

#[test]
fn gemini_20_flash_resolves_to_gemini_dialect() {
    assert_eq!(resolve_dialect("gemini-2.0-flash"), Some(Dialect::Gemini));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Codex models — capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_supports_streaming() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::Streaming));
}

#[test]
fn codex_supports_tool_use() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::ToolUse));
}

#[test]
fn codex_supports_tool_read() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::ToolRead));
}

#[test]
fn codex_supports_tool_write() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::ToolWrite));
}

#[test]
fn codex_supports_tool_edit() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::ToolEdit));
}

#[test]
fn codex_supports_tool_bash() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::ToolBash));
}

#[test]
fn codex_supports_code_execution() {
    let m = codex_manifest();
    assert!(has_native(&m, &Capability::CodeExecution));
}

#[test]
fn codex_emulates_vision() {
    let m = codex_manifest();
    assert!(has_emulated(&m, &Capability::Vision));
}

#[test]
fn codex_does_not_support_audio() {
    let m = codex_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
}

#[test]
fn codex_does_not_support_extended_thinking() {
    let m = codex_manifest();
    assert!(is_unsupported(&m, &Capability::ExtendedThinking));
}

#[test]
fn codex_model_resolves_to_codex_dialect() {
    assert_eq!(resolve_dialect("codex"), Some(Dialect::Codex));
}

#[test]
fn o3_mini_resolves_to_codex_dialect() {
    assert_eq!(resolve_dialect("o3-mini"), Some(Dialect::Codex));
}

#[test]
fn o4_mini_resolves_to_codex_dialect() {
    assert_eq!(resolve_dialect("o4-mini"), Some(Dialect::Codex));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Kimi models — capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_supports_streaming() {
    let m = kimi_manifest();
    assert!(has_native(&m, &Capability::Streaming));
}

#[test]
fn kimi_supports_tool_use() {
    let m = kimi_manifest();
    assert!(has_native(&m, &Capability::ToolUse));
}

#[test]
fn kimi_supports_function_calling() {
    let m = kimi_manifest();
    assert!(has_native(&m, &Capability::FunctionCalling));
}

#[test]
fn kimi_supports_vision() {
    let m = kimi_manifest();
    assert!(has_native(&m, &Capability::Vision));
}

#[test]
fn kimi_supports_image_input() {
    let m = kimi_manifest();
    assert!(has_native(&m, &Capability::ImageInput));
}

#[test]
fn kimi_supports_frequency_penalty() {
    let m = kimi_manifest();
    assert!(has_native(&m, &Capability::FrequencyPenalty));
}

#[test]
fn kimi_does_not_support_audio() {
    let m = kimi_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
}

#[test]
fn kimi_does_not_support_code_execution() {
    let m = kimi_manifest();
    assert!(is_unsupported(&m, &Capability::CodeExecution));
}

#[test]
fn moonshot_v1_8k_resolves_to_kimi_dialect() {
    assert_eq!(resolve_dialect("moonshot-v1-8k"), Some(Dialect::Kimi));
}

#[test]
fn moonshot_v1_32k_resolves_to_kimi_dialect() {
    assert_eq!(resolve_dialect("moonshot-v1-32k"), Some(Dialect::Kimi));
}

#[test]
fn moonshot_v1_128k_resolves_to_kimi_dialect() {
    assert_eq!(resolve_dialect("moonshot-v1-128k"), Some(Dialect::Kimi));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Copilot models — capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_supports_streaming() {
    let m = copilot_manifest();
    assert!(has_native(&m, &Capability::Streaming));
}

#[test]
fn copilot_supports_tool_use() {
    let m = copilot_manifest();
    assert!(has_native(&m, &Capability::ToolUse));
}

#[test]
fn copilot_supports_tool_read() {
    let m = copilot_manifest();
    assert!(has_native(&m, &Capability::ToolRead));
}

#[test]
fn copilot_supports_tool_web_search() {
    let m = copilot_manifest();
    assert!(has_native(&m, &Capability::ToolWebSearch));
}

#[test]
fn copilot_supports_tool_ask_user() {
    let m = copilot_manifest();
    assert!(has_native(&m, &Capability::ToolAskUser));
}

#[test]
fn copilot_emulates_vision() {
    let m = copilot_manifest();
    assert!(has_emulated(&m, &Capability::Vision));
}

#[test]
fn copilot_does_not_support_audio() {
    let m = copilot_manifest();
    assert!(is_unsupported(&m, &Capability::Audio));
}

#[test]
fn copilot_does_not_support_logprobs() {
    let m = copilot_manifest();
    assert!(is_unsupported(&m, &Capability::Logprobs));
}

#[test]
fn copilot_gpt4_resolves_to_copilot_dialect() {
    assert_eq!(resolve_dialect("copilot-gpt-4"), Some(Dialect::Copilot));
}

#[test]
fn copilot_gpt35_resolves_to_copilot_dialect() {
    assert_eq!(
        resolve_dialect("copilot-gpt-3.5-turbo"),
        Some(Dialect::Copilot)
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Model resolution — resolve model name to backend model
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_contains_default_openai_entry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/gpt-4o"));
}

#[test]
fn registry_contains_default_claude_entry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
}

#[test]
fn registry_contains_default_gemini_entry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("google/gemini-1.5-pro"));
}

#[test]
fn registry_contains_default_kimi_entry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("moonshot/kimi"));
}

#[test]
fn registry_contains_default_codex_entry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/codex"));
}

#[test]
fn registry_contains_default_copilot_entry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("github/copilot"));
}

#[test]
fn registry_default_has_six_entries() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
}

#[test]
fn registry_lookup_nonexistent_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.get("nonexistent/model").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Cross-dialect model mapping — mapper availability
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_mapper_exists() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
}

#[test]
fn claude_to_openai_mapper_exists() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
}

#[test]
fn openai_to_gemini_mapper_exists() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
}

#[test]
fn openai_to_codex_mapper_exists() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Codex).is_some());
}

#[test]
fn openai_to_kimi_mapper_exists() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).is_some());
}

#[test]
fn openai_to_copilot_mapper_exists() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
}

#[test]
fn identity_mapper_for_same_dialect() {
    for &d in Dialect::all() {
        assert!(
            default_ir_mapper(d, d).is_some(),
            "identity mapper missing for {d}"
        );
    }
}

#[test]
fn supported_pairs_includes_all_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(
            pairs.contains(&(d, d)),
            "supported pairs missing identity for {d}"
        );
    }
}

#[test]
fn supported_pairs_includes_openai_claude_bidirectional() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Model capability matrix — which models support which capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_models_support_streaming() {
    let manifests = [
        openai_gpt4o_manifest(),
        claude_35_sonnet_manifest(),
        gemini_15_pro_manifest(),
        kimi_manifest(),
        codex_manifest(),
        copilot_manifest(),
    ];
    for m in &manifests {
        assert!(has_native(m, &Capability::Streaming));
    }
}

#[test]
fn all_models_support_tool_use() {
    let manifests = [
        openai_gpt4o_manifest(),
        claude_35_sonnet_manifest(),
        gemini_15_pro_manifest(),
        kimi_manifest(),
        codex_manifest(),
        copilot_manifest(),
    ];
    for m in &manifests {
        assert!(has_native(m, &Capability::ToolUse));
    }
}

#[test]
fn all_models_support_system_message() {
    let manifests = [
        openai_gpt4o_manifest(),
        claude_35_sonnet_manifest(),
        gemini_15_pro_manifest(),
        kimi_manifest(),
        codex_manifest(),
        copilot_manifest(),
    ];
    for m in &manifests {
        assert!(has_native(m, &Capability::SystemMessage));
    }
}

#[test]
fn all_models_support_temperature() {
    let manifests = [
        openai_gpt4o_manifest(),
        claude_35_sonnet_manifest(),
        gemini_15_pro_manifest(),
        kimi_manifest(),
        codex_manifest(),
        copilot_manifest(),
    ];
    for m in &manifests {
        assert!(has_native(m, &Capability::Temperature));
    }
}

#[test]
fn only_claude_supports_extended_thinking_natively() {
    assert!(!has_native(
        &openai_gpt4o_manifest(),
        &Capability::ExtendedThinking
    ));
    assert!(has_native(
        &claude_35_sonnet_manifest(),
        &Capability::ExtendedThinking
    ));
    assert!(!has_native(
        &gemini_15_pro_manifest(),
        &Capability::ExtendedThinking
    ));
    assert!(!has_native(
        &codex_manifest(),
        &Capability::ExtendedThinking
    ));
    assert!(!has_native(
        &copilot_manifest(),
        &Capability::ExtendedThinking
    ));
    assert!(!has_native(&kimi_manifest(), &Capability::ExtendedThinking));
}

#[test]
fn negotiate_streaming_against_all_manifests() {
    let manifests = [
        ("openai/gpt-4o", openai_gpt4o_manifest()),
        ("anthropic/claude-3.5-sonnet", claude_35_sonnet_manifest()),
        ("google/gemini-1.5-pro", gemini_15_pro_manifest()),
        ("moonshot/kimi", kimi_manifest()),
        ("openai/codex", codex_manifest()),
        ("github/copilot", copilot_manifest()),
    ];
    for (name, m) in &manifests {
        let result = negotiate_capabilities(&[Capability::Streaming], m);
        assert!(
            result.is_compatible(),
            "{name} should support streaming natively"
        );
    }
}

#[test]
fn registry_query_streaming_across_all() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    for (name, level) in &results {
        assert!(
            matches!(level, SupportLevel::Native),
            "{name} should have native streaming"
        );
    }
}

#[test]
fn capability_report_openai_gpt4o_is_viable() {
    let m = openai_gpt4o_manifest();
    let result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &m,
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 3);
}

#[test]
fn compare_openai_vs_claude_in_registry() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
        .unwrap();
    // Claude can cover most OpenAI capabilities (some via emulation)
    assert!(!result.native.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Unknown models — graceful handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_model_returns_none_dialect() {
    assert!(resolve_dialect("totally-unknown-model").is_none());
}

#[test]
fn unknown_model_not_in_registry() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.get("unknown/model").is_none());
}

#[test]
fn negotiate_by_name_unknown_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(
        reg.negotiate_by_name("unknown/model", &[Capability::Streaming])
            .is_none()
    );
}

#[test]
fn compare_unknown_source_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("unknown/x", "openai/gpt-4o").is_none());
}

#[test]
fn compare_unknown_target_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("openai/gpt-4o", "unknown/x").is_none());
}

#[test]
fn empty_registry_contains_nothing() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.get("openai/gpt-4o").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Model aliases — common aliases resolve to canonical names
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn alias_gpt4_resolves_to_gpt_4() {
    assert_eq!(resolve_alias("gpt4"), "gpt-4");
}

#[test]
fn alias_gpt4o_resolves_to_gpt_4o() {
    assert_eq!(resolve_alias("gpt4o"), "gpt-4o");
}

#[test]
fn alias_gpt35_resolves_to_gpt_35_turbo() {
    assert_eq!(resolve_alias("gpt35"), "gpt-3.5-turbo");
}

#[test]
fn alias_sonnet_resolves_to_claude_35_sonnet() {
    assert_eq!(resolve_alias("sonnet"), "claude-3.5-sonnet");
}

#[test]
fn alias_haiku_resolves_to_claude_3_haiku() {
    assert_eq!(resolve_alias("haiku"), "claude-3-haiku");
}

#[test]
fn alias_opus_resolves_to_claude_3_opus() {
    assert_eq!(resolve_alias("opus"), "claude-3-opus");
}

#[test]
fn alias_gemini_pro_resolves_to_gemini_15_pro() {
    assert_eq!(resolve_alias("gemini-pro"), "gemini-1.5-pro");
}

#[test]
fn alias_gemini_flash_resolves_to_gemini_15_flash() {
    assert_eq!(resolve_alias("gemini-flash"), "gemini-1.5-flash");
}

#[test]
fn unknown_alias_returns_as_is() {
    assert_eq!(resolve_alias("some-custom-model"), "some-custom-model");
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Model versioning — date-versioned models resolve correctly
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn versioned_gpt4_0613_resolves_to_gpt4() {
    assert_eq!(resolve_versioned("gpt-4-0613"), "gpt-4");
}

#[test]
fn versioned_gpt4_1106_resolves_to_gpt4() {
    assert_eq!(resolve_versioned("gpt-4-1106"), "gpt-4");
}

#[test]
fn versioned_gpt4_20240620_resolves_to_base() {
    assert_eq!(resolve_versioned("gpt-4-20240620"), "gpt-4");
}

#[test]
fn unversioned_model_returns_as_is() {
    assert_eq!(resolve_versioned("gpt-4o"), "gpt-4o");
}

#[test]
fn versioned_model_non_date_suffix_unchanged() {
    assert_eq!(resolve_versioned("gpt-4-turbo"), "gpt-4-turbo");
}

#[test]
fn versioned_model_with_5_digit_suffix_unchanged() {
    assert_eq!(resolve_versioned("gpt-4-12345"), "gpt-4-12345");
}

// ═══════════════════════════════════════════════════════════════════════
// Additional cross-cutting tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn model_selector_lowest_latency() {
    let candidates = vec![
        ModelCandidate {
            backend_name: "fast".into(),
            model_id: "gpt-4o".into(),
            estimated_latency_ms: Some(50),
            estimated_cost_per_1k_tokens: None,
            fidelity_score: None,
            weight: 1.0,
        },
        ModelCandidate {
            backend_name: "slow".into(),
            model_id: "claude-3.5-sonnet".into(),
            estimated_latency_ms: Some(200),
            estimated_cost_per_1k_tokens: None,
            fidelity_score: None,
            weight: 1.0,
        },
    ];
    let selector = ModelSelector::new(SelectionStrategy::LowestLatency, candidates);
    let selected = selector.select().unwrap();
    assert_eq!(selected.backend_name, "fast");
}

#[test]
fn model_selector_highest_fidelity() {
    let candidates = vec![
        ModelCandidate {
            backend_name: "low-fi".into(),
            model_id: "gpt-3.5-turbo".into(),
            estimated_latency_ms: None,
            estimated_cost_per_1k_tokens: None,
            fidelity_score: Some(0.7),
            weight: 1.0,
        },
        ModelCandidate {
            backend_name: "high-fi".into(),
            model_id: "gpt-4o".into(),
            estimated_latency_ms: None,
            estimated_cost_per_1k_tokens: None,
            fidelity_score: Some(0.95),
            weight: 1.0,
        },
    ];
    let selector = ModelSelector::new(SelectionStrategy::HighestFidelity, candidates);
    let selected = selector.select().unwrap();
    assert_eq!(selected.backend_name, "high-fi");
}

#[test]
fn model_selector_empty_candidates_returns_none() {
    let selector = ModelSelector::new(SelectionStrategy::LowestLatency, vec![]);
    assert!(selector.select().is_none());
}

#[test]
fn model_selector_fallback_chain_returns_first() {
    let candidates = vec![
        ModelCandidate {
            backend_name: "primary".into(),
            model_id: "gpt-4o".into(),
            estimated_latency_ms: None,
            estimated_cost_per_1k_tokens: None,
            fidelity_score: None,
            weight: 1.0,
        },
        ModelCandidate {
            backend_name: "fallback".into(),
            model_id: "gpt-3.5-turbo".into(),
            estimated_latency_ms: None,
            estimated_cost_per_1k_tokens: None,
            fidelity_score: None,
            weight: 1.0,
        },
    ];
    let selector = ModelSelector::new(SelectionStrategy::FallbackChain, candidates);
    let selected = selector.select().unwrap();
    assert_eq!(selected.backend_name, "primary");
}

#[test]
fn negotiate_unsupported_capability_is_not_viable() {
    let m = openai_gpt4o_manifest();
    let result = negotiate_capabilities(&[Capability::ExtendedThinking], &m);
    assert!(!result.is_viable());
    assert!(!result.unsupported.is_empty());
}

#[test]
fn negotiate_empty_requirements_is_viable() {
    let m = openai_gpt4o_manifest();
    let result = negotiate_capabilities(&[], &m);
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

#[test]
fn check_capability_absent_from_manifest_is_unsupported() {
    let m = BTreeMap::new();
    let level = check_capability(&m, &Capability::Streaming);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
}

#[test]
fn codex_tool_capabilities_are_native() {
    let m = codex_manifest();
    let tool_caps = [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ];
    for cap in &tool_caps {
        assert!(has_native(&m, cap), "codex should natively support {cap:?}");
    }
}

#[test]
fn copilot_tool_capabilities_are_native() {
    let m = copilot_manifest();
    let tool_caps = [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
    ];
    for cap in &tool_caps {
        assert!(
            has_native(&m, cap),
            "copilot should natively support {cap:?}"
        );
    }
}

#[test]
fn custom_model_registration_and_retrieval() {
    let mut reg = CapabilityRegistry::new();
    let mut manifest = BTreeMap::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    reg.register("custom/my-model", manifest);
    assert!(reg.contains("custom/my-model"));
    assert_eq!(reg.len(), 1);
}

#[test]
fn unregister_model_from_registry() {
    let mut reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.unregister("openai/gpt-4o"));
    assert!(!reg.contains("openai/gpt-4o"));
}

#[test]
fn registry_names_returns_all() {
    let reg = CapabilityRegistry::with_defaults();
    let names = reg.names();
    assert_eq!(names.len(), 6);
    assert!(names.contains(&"openai/gpt-4o"));
    assert!(names.contains(&"anthropic/claude-3.5-sonnet"));
}

#[test]
fn negotiate_by_name_openai_streaming() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
        .unwrap();
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 1);
}

#[test]
fn negotiation_result_display() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::Vision],
        vec![Capability::Audio],
    );
    let display = format!("{result}");
    assert!(display.contains("1 native"));
    assert!(display.contains("1 emulated"));
    assert!(display.contains("1 unsupported"));
}
