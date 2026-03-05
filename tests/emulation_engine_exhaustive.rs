#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive tests for the emulation engine, capability negotiation,
//! and strategy subsystems across all capability variants and support levels.

use abp_capability::negotiate::{apply_policy, pre_negotiate, NegotiationError, NegotiationPolicy};
use abp_capability::{
    check_capability, default_emulation_strategy, generate_report, negotiate,
    negotiate_capabilities, CapabilityRegistry, CompatibilityReport, EmulationStrategy,
    NegotiationResult, SupportLevel,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::negotiate::{
    check_capabilities, dialect_manifest, CapabilityDiff, CapabilityNegotiator, CapabilityReport,
    CapabilityReportEntry, DialectSupportLevel, NegotiationRequest,
    NegotiationResult as CoreNegotiationResult,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel, WorkOrderBuilder,
};
use abp_emulation::strategies::{
    ParsedToolCall, StreamChunk, StreamingEmulation, ThinkingDetail, ThinkingEmulation,
    ToolUseEmulation, VisionEmulation,
};
use abp_emulation::{
    apply_emulation, can_emulate, compute_fidelity, default_strategy, emulate_code_execution,
    emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output, EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport,
    EmulationStrategy as EmuStrategy, FidelityLabel,
};
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

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

fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn user_only_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn build_wo(reqs: Vec<(Capability, MinSupport)>) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: reqs
                .into_iter()
                .map(|(c, m)| CapabilityRequirement {
                    capability: c,
                    min_support: m,
                })
                .collect(),
        })
        .build()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. EmulationStrategy variants (system_prompt_injection, post_processing, disabled)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_system_prompt_injection_variant() {
    let s = EmuStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_post_processing_variant() {
    let s = EmuStrategy::PostProcessing {
        detail: "truncate".into(),
    };
    assert!(matches!(s, EmuStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_disabled_variant() {
    let s = EmuStrategy::Disabled {
        reason: "unsafe".into(),
    };
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn strategy_equality() {
    let a = EmuStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmuStrategy::SystemPromptInjection { prompt: "x".into() };
    assert_eq!(a, b);
}

#[test]
fn strategy_inequality_different_variant() {
    let a = EmuStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmuStrategy::Disabled { reason: "x".into() };
    assert_ne!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Named strategy constructors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn emulate_structured_output_is_system_prompt() {
    let s = emulate_structured_output();
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn emulate_code_execution_is_system_prompt() {
    let s = emulate_code_execution();
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn emulate_extended_thinking_is_system_prompt() {
    let s = emulate_extended_thinking();
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn emulate_image_input_is_system_prompt() {
    let s = emulate_image_input();
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn emulate_stop_sequences_is_post_processing() {
    let s = emulate_stop_sequences();
    assert!(matches!(s, EmuStrategy::PostProcessing { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. default_strategy for all Capability variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_strategy_extended_thinking() {
    let s = default_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_structured_output() {
    let s = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s, EmuStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_code_execution_disabled() {
    let s = default_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_image_input() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_stop_sequences() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmuStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_streaming_disabled() {
    let s = default_strategy(&Capability::Streaming);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_use_disabled() {
    let s = default_strategy(&Capability::ToolUse);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_read_disabled() {
    let s = default_strategy(&Capability::ToolRead);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_vision_disabled() {
    let s = default_strategy(&Capability::Vision);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_audio_disabled() {
    let s = default_strategy(&Capability::Audio);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_every_capability_returns_something() {
    for cap in all_capabilities() {
        let _s = default_strategy(&cap);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. can_emulate for all Capability variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn can_emulate_extended_thinking_true() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn can_emulate_structured_output_true() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn can_emulate_image_input_true() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn can_emulate_stop_sequences_true() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn cannot_emulate_code_execution() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn cannot_emulate_streaming() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn cannot_emulate_tool_read() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn cannot_emulate_vision() {
    assert!(!can_emulate(&Capability::Vision));
}

#[test]
fn can_emulate_consistency_all_caps() {
    for cap in all_capabilities() {
        let strategy = default_strategy(&cap);
        let emulatable = can_emulate(&cap);
        assert_eq!(
            emulatable,
            !matches!(strategy, EmuStrategy::Disabled { .. }),
            "can_emulate mismatch for {cap:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. SupportLevel::satisfies() across all combinations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn satisfies_native_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_native_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn satisfies_emulated_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_emulated_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn satisfies_unsupported_native() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_unsupported_emulated() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn satisfies_restricted_native() {
    let r = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!r.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_restricted_emulated() {
    let r = CoreSupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(r.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. EmulationEngine — apply, check_missing, resolve_strategy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn engine_with_defaults_apply_extended_thinking() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Think step by step"));
}

#[test]
fn engine_apply_disabled_generates_warning() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn engine_apply_post_processing_no_mutation() {
    let original = simple_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(conv, original);
}

#[test]
fn engine_apply_multiple_strategies() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
            Capability::CodeExecution,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn engine_apply_empty_capabilities() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
}

#[test]
fn engine_check_missing_returns_same_classification() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn engine_resolve_strategy_uses_config_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmuStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn engine_resolve_strategy_falls_back_to_default() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::Streaming);
    assert!(matches!(s, EmuStrategy::Disabled { .. }));
}

#[test]
fn engine_config_override_enables_disabled_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmuStrategy::SystemPromptInjection {
            prompt: "Simulate execution".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn engine_apply_creates_system_message_if_absent() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Labeled emulation — never silent degradation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn emulation_always_labeled_in_report() {
    let engine = EmulationEngine::with_defaults();
    for cap in all_capabilities() {
        let mut conv = simple_conv();
        let report = engine.apply(&[cap.clone()], &mut conv);
        let total = report.applied.len() + report.warnings.len();
        assert!(
            total >= 1,
            "Capability {cap:?} produced no label (applied={}, warnings={})",
            report.applied.len(),
            report.warnings.len()
        );
    }
}

#[test]
fn disabled_capabilities_produce_warnings_not_silent() {
    let engine = EmulationEngine::with_defaults();
    let disabled_caps: Vec<Capability> = all_capabilities()
        .into_iter()
        .filter(|c| !can_emulate(c))
        .collect();

    for cap in disabled_caps {
        let report = engine.check_missing(&[cap.clone()]);
        assert!(
            !report.warnings.is_empty(),
            "Disabled capability {cap:?} did not generate a warning"
        );
    }
}

#[test]
fn emulatable_capabilities_always_appear_in_applied() {
    let engine = EmulationEngine::with_defaults();
    let emulatable: Vec<Capability> = all_capabilities()
        .into_iter()
        .filter(|c| can_emulate(c))
        .collect();

    for cap in emulatable {
        let report = engine.check_missing(&[cap.clone()]);
        assert!(
            !report.applied.is_empty(),
            "Emulatable capability {cap:?} not in applied"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. FidelityLabel / compute_fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_native_for_natively_supported() {
    let native = vec![Capability::Streaming];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(
        labels.get(&Capability::Streaming),
        Some(&FidelityLabel::Native)
    );
}

#[test]
fn fidelity_emulated_for_applied() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmuStrategy::SystemPromptInjection {
                prompt: "think".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn fidelity_omits_warned_capabilities() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["CodeExecution not emulated".into()],
    };
    let labels = compute_fidelity(&[], &report);
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn fidelity_mixed_native_and_emulated() {
    let native = vec![Capability::Streaming];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmuStrategy::SystemPromptInjection {
                prompt: "cot".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 2);
    assert!(matches!(
        labels.get(&Capability::Streaming),
        Some(FidelityLabel::Native)
    ));
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. EmulationConfig
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_new_is_empty() {
    let config = EmulationConfig::new();
    assert!(config.strategies.is_empty());
}

#[test]
fn config_set_and_get() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmuStrategy::Disabled {
            reason: "off".into(),
        },
    );
    assert!(config
        .strategies
        .contains_key(&Capability::ExtendedThinking));
}

#[test]
fn config_overwrite_replaces() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmuStrategy::SystemPromptInjection {
            prompt: "v1".into(),
        },
    );
    config.set(
        Capability::ExtendedThinking,
        EmuStrategy::SystemPromptInjection {
            prompt: "v2".into(),
        },
    );
    let s = config
        .strategies
        .get(&Capability::ExtendedThinking)
        .unwrap();
    assert!(matches!(s, EmuStrategy::SystemPromptInjection { prompt } if prompt == "v2"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. EmulationReport
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn report_empty_default() {
    let r = EmulationReport::default();
    assert!(r.is_empty());
    assert!(!r.has_unemulatable());
}

#[test]
fn report_not_empty_with_applied() {
    let r = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmuStrategy::SystemPromptInjection { prompt: "t".into() },
        }],
        warnings: vec![],
    };
    assert!(!r.is_empty());
    assert!(!r.has_unemulatable());
}

#[test]
fn report_has_unemulatable_with_warnings() {
    let r = EmulationReport {
        applied: vec![],
        warnings: vec!["something".into()],
    };
    assert!(!r.is_empty());
    assert!(r.has_unemulatable());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Emulation chain (multiple strategies composed)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chain_multiple_system_prompt_injections() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Think step by step"));
    assert!(sys_text.contains("Image inputs"));
}

#[test]
fn chain_system_prompt_and_post_processing() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    let has_spi = report
        .applied
        .iter()
        .any(|e| matches!(e.strategy, EmuStrategy::SystemPromptInjection { .. }));
    let has_pp = report
        .applied
        .iter()
        .any(|e| matches!(e.strategy, EmuStrategy::PostProcessing { .. }));
    assert!(has_spi);
    assert!(has_pp);
}

#[test]
fn chain_with_disabled_caps_mixed() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::StopSequences,
            Capability::Streaming,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn chain_all_emulatable_capabilities() {
    let engine = EmulationEngine::with_defaults();
    let emulatable: Vec<Capability> = all_capabilities()
        .into_iter()
        .filter(|c| can_emulate(c))
        .collect();
    let mut conv = simple_conv();
    let report = engine.apply(&emulatable, &mut conv);
    assert_eq!(report.applied.len(), emulatable.len());
    assert!(report.warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. free-function apply_emulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn free_fn_apply_emulation_default() {
    let config = EmulationConfig::new();
    let mut conv = simple_conv();
    let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn free_fn_apply_emulation_with_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmuStrategy::SystemPromptInjection {
            prompt: "sim".into(),
        },
    );
    let mut conv = simple_conv();
    let report = apply_emulation(&config, &[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Serde round-trips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_strategy_system_prompt() {
    let s = EmuStrategy::SystemPromptInjection {
        prompt: "hello".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmuStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_roundtrip_strategy_post_processing() {
    let s = EmuStrategy::PostProcessing {
        detail: "validate".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmuStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_roundtrip_strategy_disabled() {
    let s = EmuStrategy::Disabled {
        reason: "nope".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmuStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_roundtrip_config() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmuStrategy::SystemPromptInjection {
            prompt: "cot".into(),
        },
    );
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn serde_roundtrip_report() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmuStrategy::SystemPromptInjection {
                prompt: "think".into(),
            },
        }],
        warnings: vec!["warn".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Capability negotiation (abp-capability crate)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_caps_all_native() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(r.is_viable());
    assert_eq!(r.native.len(), 2);
}

#[test]
fn negotiate_caps_all_unsupported() {
    let m = make_manifest(&[]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert!(!r.is_viable());
    assert_eq!(r.unsupported.len(), 2);
}

#[test]
fn negotiate_caps_emulated() {
    let m = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = negotiate_capabilities(&[Capability::ToolUse], &m);
    assert!(r.is_viable());
    assert_eq!(r.emulated.len(), 1);
}

#[test]
fn negotiate_caps_restricted_classified_as_emulated() {
    let m = make_manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert_eq!(r.emulated.len(), 1);
    assert!(r.is_viable());
}

#[test]
fn negotiate_caps_explicit_unsupported() {
    let m = make_manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    let r = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(r.unsupported.len(), 1);
    assert!(!r.is_viable());
}

#[test]
fn negotiate_caps_mixed_all_levels() {
    let m = make_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Unsupported),
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
    assert_eq!(r.total(), 3);
}

#[test]
fn negotiate_empty_requirements() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. NegotiationResult helpers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn result_is_compatible_alias() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    assert!(r.is_compatible());
}

#[test]
fn result_emulated_caps() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![Capability::ToolUse, Capability::Vision],
        vec![],
    );
    let caps = r.emulated_caps();
    assert_eq!(caps.len(), 2);
}

#[test]
fn result_unsupported_caps() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![],
        vec![Capability::Audio, Capability::Logprobs],
    );
    let caps = r.unsupported_caps();
    assert_eq!(caps.len(), 2);
}

#[test]
fn result_warnings_for_approximate() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![],
    };
    assert_eq!(r.warnings().len(), 1);
}

#[test]
fn result_no_warnings_for_client_side() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    assert!(r.warnings().is_empty());
}

#[test]
fn result_display() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Audio],
    );
    let display = format!("{r}");
    assert!(display.contains("1 native"));
    assert!(display.contains("1 emulated"));
    assert!(display.contains("1 unsupported"));
    assert!(display.contains("not viable"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. check_capability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn check_cap_native() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_cap_emulated() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    assert!(matches!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_cap_restricted() {
    let m = make_manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    assert!(matches!(
        check_capability(&m, &Capability::ToolBash),
        SupportLevel::Restricted { .. }
    ));
}

#[test]
fn check_cap_explicit_unsupported() {
    let m = make_manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    assert!(matches!(
        check_capability(&m, &Capability::Vision),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn check_cap_missing_from_manifest() {
    let m = make_manifest(&[]);
    let level = check_capability(&m, &Capability::Audio);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. default_emulation_strategy (abp-capability)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_emu_strategy_client_side_group() {
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
            "{cap:?} should be ClientSide"
        );
    }
}

#[test]
fn default_emu_strategy_server_fallback_group() {
    let server = [
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
    for cap in &server {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ServerFallback,
            "{cap:?} should be ServerFallback"
        );
    }
}

#[test]
fn default_emu_strategy_approximate_group() {
    let approx = [
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
    for cap in &approx {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::Approximate,
            "{cap:?} should be Approximate"
        );
    }
}

#[test]
fn default_emu_strategy_covers_all_capabilities() {
    for cap in all_capabilities() {
        let _ = default_emulation_strategy(&cap);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. EmulationStrategy (abp-capability) Display + has_fidelity_loss
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn emu_strategy_display_client_side() {
    assert_eq!(
        EmulationStrategy::ClientSide.to_string(),
        "client-side emulation"
    );
}

#[test]
fn emu_strategy_display_server_fallback() {
    assert_eq!(
        EmulationStrategy::ServerFallback.to_string(),
        "server fallback"
    );
}

#[test]
fn emu_strategy_display_approximate() {
    assert_eq!(EmulationStrategy::Approximate.to_string(), "approximate");
}

#[test]
fn emu_strategy_fidelity_loss_approximate() {
    assert!(EmulationStrategy::Approximate.has_fidelity_loss());
}

#[test]
fn emu_strategy_no_fidelity_loss_client_side() {
    assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
}

#[test]
fn emu_strategy_no_fidelity_loss_server_fallback() {
    assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. CapabilityRegistry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
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

#[test]
fn registry_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test", m);
    assert!(reg.contains("test"));
    assert!(reg
        .get("test")
        .unwrap()
        .contains_key(&Capability::Streaming));
}

#[test]
fn registry_unregister() {
    let mut reg = CapabilityRegistry::new();
    reg.register("test", BTreeMap::new());
    assert!(reg.unregister("test"));
    assert!(!reg.contains("test"));
    assert!(!reg.unregister("test"));
}

#[test]
fn registry_names() {
    let mut reg = CapabilityRegistry::new();
    reg.register("b", BTreeMap::new());
    reg.register("a", BTreeMap::new());
    let names = reg.names();
    assert_eq!(names, vec!["a", "b"]); // BTreeMap = sorted
}

#[test]
fn registry_query_capability() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert!(!results.is_empty());
    for (name, level) in &results {
        assert!(
            matches!(
                level,
                SupportLevel::Native
                    | SupportLevel::Emulated { .. }
                    | SupportLevel::Unsupported { .. }
            ),
            "unexpected level for {name}: {level:?}"
        );
    }
}

#[test]
fn registry_negotiate_by_name() {
    let reg = CapabilityRegistry::with_defaults();
    let r = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
        .unwrap();
    assert!(r.is_viable());
    assert_eq!(r.native, vec![Capability::Streaming]);
}

#[test]
fn registry_negotiate_by_name_missing() {
    let reg = CapabilityRegistry::new();
    assert!(reg
        .negotiate_by_name("nonexistent", &[Capability::Streaming])
        .is_none());
}

#[test]
fn registry_compare_backends() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet");
    assert!(result.is_some());
    let r = result.unwrap();
    assert!(r.total() > 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. generate_report
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn generate_report_compatible() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert!(report.summary.contains("fully compatible"));
    assert_eq!(report.native_count, 1);
}

#[test]
fn generate_report_incompatible() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn generate_report_mixed() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Audio],
    );
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert_eq!(report.details.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. NegotiationPolicy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_strict_fails_unsupported() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
}

#[test]
fn policy_strict_passes_native() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn policy_strict_passes_emulated() {
    let m = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn policy_best_effort_fails_unsupported() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(&[Capability::Vision], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_err());
}

#[test]
fn policy_permissive_always_ok() {
    let m = make_manifest(&[]);
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. NegotiationError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiation_error_display_format() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Vision, "n/a".into())],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
    assert!(msg.contains("1 unsupported"));
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

// ═══════════════════════════════════════════════════════════════════════════
// 23. SupportLevel (abp-capability) Display
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn support_level_display_native() {
    assert_eq!(SupportLevel::Native.to_string(), "native");
}

#[test]
fn support_level_display_emulated() {
    let level = SupportLevel::Emulated {
        method: "polyfill".into(),
    };
    assert!(level.to_string().contains("polyfill"));
}

#[test]
fn support_level_display_restricted() {
    let level = SupportLevel::Restricted {
        reason: "sandboxed".into(),
    };
    assert!(level.to_string().contains("sandboxed"));
}

#[test]
fn support_level_display_unsupported() {
    let level = SupportLevel::Unsupported {
        reason: "missing".into(),
    };
    assert!(level.to_string().contains("missing"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. Core CapabilityNegotiator (abp-core::negotiate)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn core_negotiator_all_required_satisfied() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    m.insert(Capability::ToolUse, CoreSupportLevel::Native);

    let req = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolUse],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 2);
}

#[test]
fn core_negotiator_unsatisfied() {
    let m = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied.len(), 1);
}

#[test]
fn core_negotiator_preferred_bonus() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    m.insert(Capability::ToolUse, CoreSupportLevel::Native);

    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolUse],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
    assert_eq!(result.bonus.len(), 1);
}

#[test]
fn core_negotiator_best_match() {
    let m1 = BTreeMap::from([(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolUse],
        minimum_support: CoreSupportLevel::Native,
    };
    let best = CapabilityNegotiator::best_match(&req, &[("a", m1), ("b", m2)]);
    assert!(best.is_some());
    let (name, _) = best.unwrap();
    assert_eq!(name, "b");
}

#[test]
fn core_negotiator_no_match() {
    let m = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let best = CapabilityNegotiator::best_match(&req, &[("a", m)]);
    assert!(best.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. CapabilityDiff
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diff_added_capability() {
    let old = CapabilityManifest::new();
    let mut new = CapabilityManifest::new();
    new.insert(Capability::Streaming, CoreSupportLevel::Native);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.added.len(), 1);
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_removed_capability() {
    let mut old = CapabilityManifest::new();
    old.insert(Capability::Streaming, CoreSupportLevel::Native);
    let new = CapabilityManifest::new();
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed.len(), 1);
}

#[test]
fn diff_upgraded() {
    let mut old = CapabilityManifest::new();
    old.insert(Capability::Streaming, CoreSupportLevel::Emulated);
    let mut new = CapabilityManifest::new();
    new.insert(Capability::Streaming, CoreSupportLevel::Native);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
}

#[test]
fn diff_downgraded() {
    let mut old = CapabilityManifest::new();
    old.insert(Capability::Streaming, CoreSupportLevel::Native);
    let mut new = CapabilityManifest::new();
    new.insert(Capability::Streaming, CoreSupportLevel::Emulated);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
}

#[test]
fn diff_no_changes() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, CoreSupportLevel::Native);
    let diff = CapabilityDiff::diff(&m, &m);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.upgraded.is_empty());
    assert!(diff.downgraded.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. Dialect manifests and CapabilityReport (abp-core::negotiate)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_manifest_claude_exists() {
    let m = dialect_manifest("claude");
    assert!(!m.is_empty());
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_openai_exists() {
    let m = dialect_manifest("openai");
    assert!(!m.is_empty());
}

#[test]
fn dialect_manifest_gemini_exists() {
    let m = dialect_manifest("gemini");
    assert!(!m.is_empty());
}

#[test]
fn dialect_manifest_unknown_is_empty() {
    let m = dialect_manifest("unknown_vendor");
    assert!(m.is_empty());
}

#[test]
fn capability_report_all_satisfiable() {
    let wo = build_wo(vec![(Capability::Streaming, MinSupport::Emulated)]);
    let report = check_capabilities(&wo, "claude", "claude");
    assert!(report.all_satisfiable());
}

#[test]
fn capability_report_native_capabilities() {
    let wo = build_wo(vec![(Capability::Streaming, MinSupport::Emulated)]);
    let report = check_capabilities(&wo, "claude", "claude");
    let native = report.native_capabilities();
    assert!(!native.is_empty());
}

#[test]
fn capability_report_to_receipt_metadata() {
    let wo = build_wo(vec![(Capability::Streaming, MinSupport::Emulated)]);
    let report = check_capabilities(&wo, "claude", "openai");
    let meta = report.to_receipt_metadata();
    assert!(meta.is_object());
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. Concrete strategies: ThinkingEmulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_brief_prompt() {
    let t = ThinkingEmulation::brief();
    assert!(t.prompt_text().contains("step by step"));
}

#[test]
fn thinking_standard_prompt_has_tags() {
    let t = ThinkingEmulation::standard();
    assert!(t.prompt_text().contains("<thinking>"));
}

#[test]
fn thinking_detailed_prompt_has_verification() {
    let t = ThinkingEmulation::detailed();
    assert!(t.prompt_text().contains("Verify"));
}

#[test]
fn thinking_inject_adds_to_system() {
    let t = ThinkingEmulation::standard();
    let mut conv = simple_conv();
    t.inject(&mut conv);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("<thinking>"));
}

#[test]
fn thinking_inject_creates_system_message() {
    let t = ThinkingEmulation::brief();
    let mut conv = user_only_conv();
    t.inject(&mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn thinking_extract_with_tags() {
    let text = "Before <thinking>my reasoning</thinking> After";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "my reasoning");
    assert_eq!(answer, "Before After");
}

#[test]
fn thinking_extract_without_tags() {
    let text = "plain answer";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert_eq!(answer, "plain answer");
}

#[test]
fn thinking_to_block_some() {
    let block = ThinkingEmulation::to_thinking_block("<thinking>reason</thinking> answer");
    assert!(block.is_some());
    assert!(matches!(block.unwrap(), IrContentBlock::Thinking { .. }));
}

#[test]
fn thinking_to_block_none() {
    let block = ThinkingEmulation::to_thinking_block("no tags here");
    assert!(block.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. Concrete strategies: ToolUseEmulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_empty_tools_prompt() {
    let prompt = ToolUseEmulation::tools_to_prompt(&[]);
    assert!(prompt.is_empty());
}

#[test]
fn tool_use_tools_prompt_contains_name() {
    let tools = vec![IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("read_file"));
    assert!(prompt.contains("Read a file"));
}

#[test]
fn tool_use_inject_tools() {
    let mut conv = simple_conv();
    let tools = vec![IrToolDefinition {
        name: "search".into(),
        description: "Search".into(),
        parameters: serde_json::Value::Null,
    }];
    ToolUseEmulation::inject_tools(&mut conv, &tools);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("search"));
}

#[test]
fn tool_use_parse_valid_call() {
    let text = r#"Here is my response <tool_call>
{"name": "read_file", "arguments": {"path": "test.rs"}}
</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    let call = results[0].as_ref().unwrap();
    assert_eq!(call.name, "read_file");
}

#[test]
fn tool_use_parse_multiple_calls() {
    let text = r#"<tool_call>{"name": "a", "arguments": {}}</tool_call> text <tool_call>{"name": "b", "arguments": {}}</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
}

#[test]
fn tool_use_parse_invalid_json() {
    let text = "<tool_call>not json</tool_call>";
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn tool_use_parse_missing_name() {
    let text = r#"<tool_call>{"arguments": {}}</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn tool_use_parse_unclosed_tag() {
    let text = "<tool_call>content without closing";
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn tool_use_to_block() {
    let call = ParsedToolCall {
        name: "test".into(),
        arguments: serde_json::json!({"key": "val"}),
    };
    let block = ToolUseEmulation::to_tool_use_block(&call, "id-1");
    assert!(
        matches!(block, IrContentBlock::ToolUse { id, name, .. } if id == "id-1" && name == "test")
    );
}

#[test]
fn tool_use_format_result_success() {
    let result = ToolUseEmulation::format_tool_result("grep", "found 5 matches", false);
    assert!(result.contains("grep"));
    assert!(result.contains("found 5 matches"));
    assert!(!result.contains("error"));
}

#[test]
fn tool_use_format_result_error() {
    let result = ToolUseEmulation::format_tool_result("grep", "permission denied", true);
    assert!(result.contains("error"));
}

#[test]
fn tool_use_extract_text_outside() {
    let text = "before <tool_call>{\"name\":\"a\",\"arguments\":{}}</tool_call> after";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("before"));
    assert!(outside.contains("after"));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn tool_use_extract_text_no_calls() {
    let text = "just plain text";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert_eq!(outside, "just plain text");
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. Concrete strategies: VisionEmulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn vision_has_images_false() {
    let conv = simple_conv();
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_has_images_true() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    );
    let conv = IrConversation::new().push(msg);
    assert!(VisionEmulation::has_images(&conv));
}

#[test]
fn vision_replace_images() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "data".into(),
        }],
    );
    let mut conv = IrConversation::new().push(msg);
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 1);
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_apply_full() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Look at this".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data".into(),
            },
        ],
    );
    let mut conv = IrConversation::new().push(msg);
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
    assert!(conv.system_message().is_some());
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("does not support vision"));
}

#[test]
fn vision_apply_no_images_no_system() {
    let mut conv = user_only_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 0);
    assert!(conv.system_message().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 30. Concrete strategies: StreamingEmulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_chunk_size_minimum() {
    let s = StreamingEmulation::new(0);
    assert_eq!(s.chunk_size(), 1);
}

#[test]
fn streaming_default_chunk_size() {
    let s = StreamingEmulation::default_chunk_size();
    assert_eq!(s.chunk_size(), 20);
}

#[test]
fn streaming_split_empty() {
    let s = StreamingEmulation::new(10);
    let chunks = s.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert!(chunks[0].content.is_empty());
}

#[test]
fn streaming_split_short() {
    let s = StreamingEmulation::new(100);
    let chunks = s.split_into_chunks("hello");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert_eq!(chunks[0].content, "hello");
}

#[test]
fn streaming_split_exact_boundary() {
    let s = StreamingEmulation::new(5);
    let chunks = s.split_into_chunks("abcde");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_split_multi_chunk() {
    let s = StreamingEmulation::new(5);
    let chunks = s.split_into_chunks("hello world");
    assert!(chunks.len() >= 2);
    assert!(chunks.last().unwrap().is_final);
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.index, i);
    }
}

#[test]
fn streaming_split_fixed() {
    let s = StreamingEmulation::new(3);
    let chunks = s.split_fixed("abcdef");
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].content, "abc");
    assert_eq!(chunks[1].content, "def");
    assert!(chunks[1].is_final);
}

#[test]
fn streaming_reassemble() {
    let s = StreamingEmulation::new(5);
    let text = "hello world test";
    let chunks = s.split_into_chunks(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_reassemble_fixed() {
    let s = StreamingEmulation::new(3);
    let text = "abcdefghi";
    let chunks = s.split_fixed(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_chunk_serde_roundtrip() {
    let chunk = StreamChunk {
        content: "hello".into(),
        index: 0,
        is_final: true,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let decoded: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, decoded);
}

// ═══════════════════════════════════════════════════════════════════════════
// 31. negotiate() with MinSupport requirements
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_with_requirements_native_only() {
    let m = make_manifest(&[
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
                min_support: MinSupport::Native,
            },
        ],
    };
    let r = negotiate(&m, &reqs);
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn negotiate_with_requirements_emulated_ok() {
    let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let r = negotiate(&m, &reqs);
    assert!(r.is_viable());
    assert_eq!(r.emulated.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 32. Pre-populated manifest coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_manifest_has_streaming() {
    let m = abp_capability::openai_gpt4o_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn claude_manifest_has_extended_thinking() {
    let m = abp_capability::claude_35_sonnet_manifest();
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn gemini_manifest_has_code_execution() {
    let m = abp_capability::gemini_15_pro_manifest();
    assert!(matches!(
        m.get(&Capability::CodeExecution),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn kimi_manifest_has_vision() {
    let m = abp_capability::kimi_manifest();
    assert!(matches!(
        m.get(&Capability::Vision),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn codex_manifest_has_tool_read() {
    let m = abp_capability::codex_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(CoreSupportLevel::Native)
    ));
}

#[test]
fn copilot_manifest_has_tool_web_search() {
    let m = abp_capability::copilot_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWebSearch),
        Some(CoreSupportLevel::Native)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 33. Cross-dialect capability reports
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_claude_to_openai_report() {
    let wo = build_wo(vec![
        (Capability::Streaming, MinSupport::Emulated),
        (Capability::ExtendedThinking, MinSupport::Emulated),
    ]);
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(!report.all_satisfiable());
    let unsup = report.unsupported_capabilities();
    assert!(!unsup.is_empty());
}

#[test]
fn cross_dialect_openai_to_gemini_report() {
    let wo = build_wo(vec![
        (Capability::Streaming, MinSupport::Emulated),
        (Capability::ToolUse, MinSupport::Emulated),
    ]);
    let report = check_capabilities(&wo, "openai", "gemini");
    assert!(report.all_satisfiable());
}

#[test]
fn cross_dialect_to_unknown_all_unsupported() {
    let wo = build_wo(vec![(Capability::Streaming, MinSupport::Emulated)]);
    let report = check_capabilities(&wo, "claude", "unknown_backend");
    assert!(!report.all_satisfiable());
}
