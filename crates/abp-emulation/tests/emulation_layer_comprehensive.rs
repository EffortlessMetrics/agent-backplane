#![allow(clippy::all)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for the ABP emulation layer.
//!
//! Covers: emulation detection/labeling, native vs emulated vs unsupported
//! classification, emulation strategies, explicit labeling (no silent degradation),
//! emulation reporting in receipts, edge cases, error cases, and serde round-trips.

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_emulation::strategies::*;
use abp_emulation::*;

// ── Helpers ────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn bare_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn multi_turn() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "System"))
        .push(IrMessage::text(IrRole::User, "Q1"))
        .push(IrMessage::text(IrRole::Assistant, "A1"))
        .push(IrMessage::text(IrRole::User, "Q2"))
}

fn image_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    ))
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "get_weather".into(),
            description: "Get current weather for a location".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"location": {"type": "string"}}}),
        },
        IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        },
    ]
}

/// All capabilities that have a non-disabled default strategy.
fn emulatable_capabilities() -> Vec<Capability> {
    vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ]
}

/// All capabilities that are disabled by default.
fn disabled_capabilities() -> Vec<Capability> {
    vec![
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
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
        Capability::McpClient,
        Capability::McpServer,
        Capability::PdfInput,
        Capability::Logprobs,
        Capability::SeedDeterminism,
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

// ═══════════════════════════════════════════════════════════════════════
// §1  Emulation detection — default_strategy & can_emulate
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn default_strategy_extended_thinking_is_system_prompt_injection() {
    let s = default_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_structured_output_is_post_processing() {
    let s = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_code_execution_is_disabled() {
    let s = default_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_image_input_is_system_prompt_injection() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_stop_sequences_is_post_processing() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_streaming_is_disabled() {
    let s = default_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_use_is_disabled() {
    let s = default_strategy(&Capability::ToolUse);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn can_emulate_returns_true_for_all_emulatable() {
    for cap in emulatable_capabilities() {
        assert!(can_emulate(&cap), "expected can_emulate=true for {cap:?}");
    }
}

#[test]
fn can_emulate_returns_false_for_all_disabled() {
    for cap in disabled_capabilities() {
        assert!(!can_emulate(&cap), "expected can_emulate=false for {cap:?}");
    }
}

#[test]
fn default_strategy_disabled_reason_contains_capability_name() {
    let s = default_strategy(&Capability::Streaming);
    if let EmulationStrategy::Disabled { reason } = s {
        assert!(reason.contains("Streaming"));
    } else {
        panic!("expected Disabled");
    }
}

#[test]
fn default_strategy_code_execution_reason_mentions_sandbox() {
    let s = default_strategy(&Capability::CodeExecution);
    if let EmulationStrategy::Disabled { reason } = s {
        assert!(reason.contains("sandbox") || reason.contains("safely"));
    } else {
        panic!("expected Disabled");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  Native vs emulated vs unsupported classification (FidelityLabel)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_native_for_natively_supported() {
    let native = vec![Capability::Streaming, Capability::ToolUse];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);

    assert_eq!(
        labels.get(&Capability::Streaming).unwrap(),
        &FidelityLabel::Native
    );
    assert_eq!(
        labels.get(&Capability::ToolUse).unwrap(),
        &FidelityLabel::Native
    );
}

#[test]
fn fidelity_emulated_for_applied_emulation() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "Think.".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    let label = labels.get(&Capability::ExtendedThinking).unwrap();
    assert!(matches!(label, FidelityLabel::Emulated { .. }));
}

#[test]
fn fidelity_omits_unemulatable_capabilities() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["Capability CodeExecution not emulated: unsafe".into()],
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
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "Think.".into(),
            },
        }],
        warnings: vec!["CodeExecution disabled".into()],
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
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn fidelity_emulated_overrides_native_for_same_capability() {
    // If a capability appears in both native and emulated, emulated wins (applied after)
    let native = vec![Capability::ExtendedThinking];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::PostProcessing {
                detail: "custom".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn fidelity_empty_inputs_gives_empty_labels() {
    let labels = compute_fidelity(&[], &EmulationReport::default());
    assert!(labels.is_empty());
}

#[test]
fn fidelity_all_native() {
    let native = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::CodeExecution,
    ];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 3);
    for (_, label) in &labels {
        assert_eq!(label, &FidelityLabel::Native);
    }
}

#[test]
fn fidelity_all_emulated() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: emulate_extended_thinking(),
            },
            EmulationEntry {
                capability: Capability::StructuredOutputJsonSchema,
                strategy: emulate_structured_output(),
            },
        ],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 2);
    for (_, label) in &labels {
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Emulation strategies for different capabilities
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn named_strategy_structured_output() {
    let s = emulate_structured_output();
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("JSON"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn named_strategy_code_execution() {
    let s = emulate_code_execution();
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("execute code") || prompt.contains("reason through"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn named_strategy_extended_thinking() {
    let s = emulate_extended_thinking();
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("step by step"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn named_strategy_image_input() {
    let s = emulate_image_input();
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("Image") || prompt.contains("image"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn named_strategy_stop_sequences() {
    let s = emulate_stop_sequences();
    if let EmulationStrategy::PostProcessing { detail } = &s {
        assert!(detail.contains("stop sequence"));
    } else {
        panic!("expected PostProcessing");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Explicit labeling — no silent degradation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn emulation_always_records_in_report() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(!report.is_empty(), "emulation must be recorded");
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
}

#[test]
fn disabled_capability_generates_warning_not_silent() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.has_unemulatable());
    assert!(!report.warnings.is_empty());
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn multiple_emulations_each_labeled() {
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
    let caps: Vec<_> = report.applied.iter().map(|e| &e.capability).collect();
    assert!(caps.contains(&&Capability::ExtendedThinking));
    assert!(caps.contains(&&Capability::StructuredOutputJsonSchema));
}

#[test]
fn disabled_and_emulated_both_recorded() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::CodeExecution],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn report_entry_strategy_matches_resolved() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let expected = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert_eq!(report.applied[0].strategy, expected);
}

// ═══════════════════════════════════════════════════════════════════════
// §5  Emulation reporting in receipts / reports
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn report_is_empty_when_nothing_requested() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn report_has_unemulatable_only_for_disabled() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(!report.has_unemulatable());
}

#[test]
fn report_has_unemulatable_true_for_disabled() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.has_unemulatable());
}

#[test]
fn report_serde_roundtrip_empty() {
    let report = EmulationReport::default();
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn report_serde_roundtrip_with_entries() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think.".into(),
                },
            },
            EmulationEntry {
                capability: Capability::StopSequences,
                strategy: EmulationStrategy::PostProcessing {
                    detail: "truncate".into(),
                },
            },
        ],
        warnings: vec!["CodeExecution disabled".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn fidelity_label_serde_roundtrip_native() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn fidelity_label_serde_roundtrip_emulated() {
    let label = FidelityLabel::Emulated {
        strategy: emulate_extended_thinking(),
    };
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn emulation_config_serde_roundtrip() {
    let mut config = EmulationConfig::new();
    config.set(Capability::ExtendedThinking, emulate_extended_thinking());
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn strategy_serde_roundtrip_all_variants() {
    let variants = vec![
        EmulationStrategy::SystemPromptInjection {
            prompt: "test prompt".into(),
        },
        EmulationStrategy::PostProcessing {
            detail: "validate".into(),
        },
        EmulationStrategy::Disabled {
            reason: "not available".into(),
        },
    ];
    for s in &variants {
        let json = serde_json::to_string(s).unwrap();
        let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*s, decoded);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6  Edge cases: all emulated, none emulated, mixed
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_capabilities_emulated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = emulatable_capabilities();
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len(), caps.len());
    assert!(report.warnings.is_empty());
}

#[test]
fn no_capabilities_emulated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
}

#[test]
fn all_disabled_capabilities_produce_warnings() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = disabled_capabilities();
    let report = engine.apply(&caps, &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), caps.len());
}

#[test]
fn mixed_emulatable_and_disabled() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::StructuredOutputJsonSchema,
            Capability::Streaming,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn duplicate_capabilities_each_recorded() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
}

#[test]
fn empty_conversation_with_emulation() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    // Should have created a system message
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn multi_turn_conversation_preserves_messages() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = multi_turn();
    let original_len = conv.messages.len();
    let _ = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages.len(), original_len);
}

#[test]
fn system_prompt_injection_appends_to_existing_system_message() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let _ = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("You are helpful."));
    assert!(text.contains("Think step by step"));
}

#[test]
fn system_prompt_injection_creates_new_system_message_if_missing() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = bare_conv();
    let _ = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(
        conv.messages[0]
            .text_content()
            .contains("Think step by step")
    );
}

#[test]
fn post_processing_does_not_mutate_conversation() {
    let original = simple_conv();
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn disabled_strategy_does_not_mutate_conversation() {
    let original = bare_conv();
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv, original);
}

// ═══════════════════════════════════════════════════════════════════════
// §7  Error cases: cannot emulate
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cannot_emulate_code_execution_by_default() {
    assert!(!can_emulate(&Capability::CodeExecution));
    let engine = EmulationEngine::with_defaults();
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
    assert!(report.has_unemulatable());
}

#[test]
fn cannot_emulate_streaming_by_default() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn cannot_emulate_tool_read_by_default() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn cannot_emulate_tool_write_by_default() {
    assert!(!can_emulate(&Capability::ToolWrite));
}

#[test]
fn cannot_emulate_session_resume_by_default() {
    assert!(!can_emulate(&Capability::SessionResume));
}

#[test]
fn cannot_emulate_checkpointing_by_default() {
    assert!(!can_emulate(&Capability::Checkpointing));
}

#[test]
fn cannot_emulate_mcp_client_by_default() {
    assert!(!can_emulate(&Capability::McpClient));
}

#[test]
fn cannot_emulate_embeddings_by_default() {
    assert!(!can_emulate(&Capability::Embeddings));
}

#[test]
fn cannot_emulate_audio_by_default() {
    assert!(!can_emulate(&Capability::Audio));
}

#[test]
fn cannot_emulate_image_generation_by_default() {
    assert!(!can_emulate(&Capability::ImageGeneration));
}

#[test]
fn warning_message_format_contains_capability_and_reason() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.warnings[0].contains("CodeExecution"));
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn all_disabled_capabilities_produce_individual_warnings() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = vec![
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
    ];
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.warnings.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// §8  EmulationConfig
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_new_is_empty() {
    let config = EmulationConfig::new();
    assert!(config.strategies.is_empty());
}

#[test]
fn config_default_is_empty() {
    let config = EmulationConfig::default();
    assert!(config.strategies.is_empty());
}

#[test]
fn config_set_adds_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    assert_eq!(config.strategies.len(), 1);
}

#[test]
fn config_set_overwrites_existing() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "v1".into(),
        },
    );
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "v2".into(),
        },
    );
    assert_eq!(config.strategies.len(), 1);
    if let EmulationStrategy::SystemPromptInjection { prompt } = config
        .strategies
        .get(&Capability::ExtendedThinking)
        .unwrap()
    {
        assert_eq!(prompt, "v2");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn config_override_replaces_default_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn config_override_enables_disabled_capability() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code execution.".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn config_override_disables_emulatable_capability() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user choice".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// §9  EmulationEngine
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn engine_with_defaults_resolves_default_strategies() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn engine_resolve_falls_back_to_default() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn engine_check_missing_reports_emulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn engine_check_missing_reports_disabled() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn engine_check_missing_mixed() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::StructuredOutputJsonSchema,
    ]);
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn engine_check_missing_empty_capabilities() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
}

#[test]
fn engine_apply_consistent_with_check_missing() {
    let engine = EmulationEngine::with_defaults();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::StructuredOutputJsonSchema,
    ];
    let check = engine.check_missing(&caps);
    let mut conv = simple_conv();
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

// ═══════════════════════════════════════════════════════════════════════
// §10  Free-function apply_emulation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn free_function_apply_emulation_works() {
    let config = EmulationConfig::new();
    let mut conv = simple_conv();
    let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn free_function_respects_config_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "custom".into(),
        },
    );
    let mut conv = bare_conv();
    let report = apply_emulation(&config, &[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn free_function_empty_everything() {
    let config = EmulationConfig::new();
    let mut conv = IrConversation::new();
    let report = apply_emulation(&config, &[], &mut conv);
    assert!(report.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §11  ThinkingEmulation strategies
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn thinking_brief_prompt() {
    let emu = ThinkingEmulation::brief();
    assert!(emu.prompt_text().contains("step by step"));
}

#[test]
fn thinking_standard_prompt_has_tags() {
    let emu = ThinkingEmulation::standard();
    let text = emu.prompt_text();
    assert!(text.contains("<thinking>"));
    assert!(text.contains("</thinking>"));
}

#[test]
fn thinking_detailed_prompt_has_verification() {
    let emu = ThinkingEmulation::detailed();
    let text = emu.prompt_text();
    assert!(text.contains("Verify") || text.contains("verify"));
}

#[test]
fn thinking_inject_adds_to_existing_system() {
    let emu = ThinkingEmulation::standard();
    let mut conv = simple_conv();
    emu.inject(&mut conv);
    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("<thinking>"));
    assert!(sys.text_content().contains("You are helpful."));
}

#[test]
fn thinking_inject_creates_system_if_missing() {
    let emu = ThinkingEmulation::brief();
    let mut conv = bare_conv();
    emu.inject(&mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("step by step"));
}

#[test]
fn thinking_extract_with_tags() {
    let text = "Some preamble <thinking>I need to think</thinking> The answer is 42.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "I need to think");
    assert!(answer.contains("42"));
}

#[test]
fn thinking_extract_no_tags() {
    let text = "Just a plain answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert_eq!(answer, text);
}

#[test]
fn thinking_extract_empty_tags() {
    let text = "<thinking></thinking>The answer";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert!(answer.contains("The answer"));
}

#[test]
fn thinking_extract_tags_only() {
    let text = "<thinking>reasoning</thinking>";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "reasoning");
    assert!(answer.is_empty());
}

#[test]
fn thinking_to_block_returns_some_with_tags() {
    let text = "<thinking>I think therefore I am</thinking> Answer.";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_some());
    if let Some(IrContentBlock::Thinking { text }) = block {
        assert_eq!(text, "I think therefore I am");
    }
}

#[test]
fn thinking_to_block_returns_none_without_tags() {
    let text = "No thinking tags here.";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_none());
}

#[test]
fn thinking_detail_brief_standard_detailed_differ() {
    let b = ThinkingEmulation::brief().prompt_text();
    let s = ThinkingEmulation::standard().prompt_text();
    let d = ThinkingEmulation::detailed().prompt_text();
    assert_ne!(b, s);
    assert_ne!(s, d);
    assert_ne!(b, d);
}

// ═══════════════════════════════════════════════════════════════════════
// §12  ToolUseEmulation strategies
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tools_to_prompt_empty_returns_empty() {
    let prompt = ToolUseEmulation::tools_to_prompt(&[]);
    assert!(prompt.is_empty());
}

#[test]
fn tools_to_prompt_includes_tool_names() {
    let tools = sample_tools();
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("get_weather"));
    assert!(prompt.contains("search"));
}

#[test]
fn tools_to_prompt_includes_descriptions() {
    let tools = sample_tools();
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("Get current weather"));
    assert!(prompt.contains("Search the web"));
}

#[test]
fn tools_to_prompt_includes_tool_call_instructions() {
    let tools = sample_tools();
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("<tool_call>"));
}

#[test]
fn inject_tools_adds_to_existing_system() {
    let tools = sample_tools();
    let mut conv = simple_conv();
    ToolUseEmulation::inject_tools(&mut conv, &tools);
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("get_weather"));
    assert!(text.contains("You are helpful."));
}

#[test]
fn inject_tools_creates_system_if_missing() {
    let tools = sample_tools();
    let mut conv = bare_conv();
    ToolUseEmulation::inject_tools(&mut conv, &tools);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("get_weather"));
}

#[test]
fn inject_tools_empty_tools_no_change() {
    let original = bare_conv();
    let mut conv = bare_conv();
    ToolUseEmulation::inject_tools(&mut conv, &[]);
    assert_eq!(conv, original);
}

#[test]
fn parse_tool_calls_single_valid() {
    let text = r#"Some text <tool_call>
{"name": "get_weather", "arguments": {"location": "NYC"}}
</tool_call> more text"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    let call = calls[0].as_ref().unwrap();
    assert_eq!(call.name, "get_weather");
    assert_eq!(call.arguments["location"], "NYC");
}

#[test]
fn parse_tool_calls_multiple() {
    let text = r#"<tool_call>
{"name": "get_weather", "arguments": {"location": "NYC"}}
</tool_call>
<tool_call>
{"name": "search", "arguments": {"query": "rust"}}
</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].as_ref().unwrap().name, "get_weather");
    assert_eq!(calls[1].as_ref().unwrap().name, "search");
}

#[test]
fn parse_tool_calls_no_tags() {
    let text = "Just normal text without any tool calls.";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert!(calls.is_empty());
}

#[test]
fn parse_tool_calls_invalid_json() {
    let text = "<tool_call>not valid json</tool_call>";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("invalid JSON"));
}

#[test]
fn parse_tool_calls_missing_name() {
    let text = r#"<tool_call>{"arguments": {"x": 1}}</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("missing 'name'"));
}

#[test]
fn parse_tool_calls_unclosed_tag() {
    let text = "<tool_call>{\"name\": \"test\", \"arguments\": {}}";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("unclosed"));
}

#[test]
fn parse_tool_calls_missing_arguments_uses_null() {
    let text = r#"<tool_call>{"name": "test"}</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    let call = calls[0].as_ref().unwrap();
    assert_eq!(call.name, "test");
    assert!(call.arguments.is_null());
}

#[test]
fn to_tool_use_block_creates_correct_block() {
    let call = ParsedToolCall {
        name: "get_weather".into(),
        arguments: serde_json::json!({"location": "NYC"}),
    };
    let block = ToolUseEmulation::to_tool_use_block(&call, "call-1");
    if let IrContentBlock::ToolUse { id, name, input } = block {
        assert_eq!(id, "call-1");
        assert_eq!(name, "get_weather");
        assert_eq!(input["location"], "NYC");
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn format_tool_result_success() {
    let result = ToolUseEmulation::format_tool_result("test", "ok", false);
    assert!(result.contains("test"));
    assert!(result.contains("ok"));
    assert!(!result.contains("error"));
}

#[test]
fn format_tool_result_error() {
    let result = ToolUseEmulation::format_tool_result("test", "fail", true);
    assert!(result.contains("error"));
    assert!(result.contains("fail"));
}

#[test]
fn extract_text_outside_tool_calls_no_calls() {
    let text = "Just plain text.";
    assert_eq!(
        ToolUseEmulation::extract_text_outside_tool_calls(text),
        text
    );
}

#[test]
fn extract_text_outside_tool_calls_removes_calls() {
    let text = "Before <tool_call>{\"name\":\"x\",\"arguments\":{}}</tool_call> After";
    let result = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(result.contains("Before"));
    assert!(result.contains("After"));
    assert!(!result.contains("tool_call"));
}

#[test]
fn extract_text_outside_only_tool_call() {
    let text = r#"<tool_call>{"name":"x","arguments":{}}</tool_call>"#;
    let result = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(result.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §13  VisionEmulation strategies
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vision_has_images_true() {
    let conv = image_conv();
    assert!(VisionEmulation::has_images(&conv));
}

#[test]
fn vision_has_images_false() {
    let conv = simple_conv();
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_replace_images_returns_count() {
    let mut conv = image_conv();
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 1);
}

#[test]
fn vision_replace_images_removes_image_blocks() {
    let mut conv = image_conv();
    VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_replace_images_adds_placeholder_text() {
    let mut conv = image_conv();
    VisionEmulation::replace_images_with_placeholders(&mut conv);
    let text = conv.messages[0].text_content();
    assert!(text.contains("[Image"));
    assert!(text.contains("image/png"));
}

#[test]
fn vision_replace_preserves_text_blocks() {
    let mut conv = image_conv();
    VisionEmulation::replace_images_with_placeholders(&mut conv);
    let text = conv.messages[0].text_content();
    assert!(text.contains("What is this?"));
}

#[test]
fn vision_inject_fallback_prompt_with_images() {
    let mut conv = image_conv();
    VisionEmulation::inject_vision_fallback_prompt(&mut conv, 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("1 image(s)"));
}

#[test]
fn vision_inject_fallback_prompt_zero_images_noop() {
    let original = bare_conv();
    let mut conv = bare_conv();
    VisionEmulation::inject_vision_fallback_prompt(&mut conv, 0);
    assert_eq!(conv, original);
}

#[test]
fn vision_apply_full_pipeline() {
    let mut conv = image_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
    assert!(!VisionEmulation::has_images(&conv));
    // Check system prompt was injected
    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("image"));
}

#[test]
fn vision_apply_no_images_noop() {
    let original = simple_conv();
    let mut conv = simple_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 0);
    assert_eq!(conv, original);
}

#[test]
fn vision_multiple_images_in_single_message() {
    let mut conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data1".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "data2".into(),
            },
        ],
    ));
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 2);
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_images_across_multiple_messages() {
    let mut conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "d1".into(),
            }],
        ))
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "d2".into(),
            }],
        ));
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 2);
}

// ═══════════════════════════════════════════════════════════════════════
// §14  StreamingEmulation strategies
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn streaming_chunk_size_minimum_is_one() {
    let emu = StreamingEmulation::new(0);
    assert_eq!(emu.chunk_size(), 1);
}

#[test]
fn streaming_default_chunk_size_is_20() {
    let emu = StreamingEmulation::default_chunk_size();
    assert_eq!(emu.chunk_size(), 20);
}

#[test]
fn streaming_split_empty_text() {
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.is_empty());
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_split_short_text() {
    let emu = StreamingEmulation::new(100);
    let chunks = emu.split_into_chunks("Hello");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "Hello");
    assert!(chunks[0].is_final);
    assert_eq!(chunks[0].index, 0);
}

#[test]
fn streaming_split_prefers_word_boundaries() {
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks("Hello World Test");
    // "Hello " is 6 chars, fits in 10, then "World " and "Test"
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, "Hello World Test");
}

#[test]
fn streaming_last_chunk_is_final() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello World");
    let last = chunks.last().unwrap();
    assert!(last.is_final);
    for chunk in &chunks[..chunks.len() - 1] {
        assert!(!chunk.is_final);
    }
}

#[test]
fn streaming_indices_are_sequential() {
    let emu = StreamingEmulation::new(3);
    let chunks = emu.split_into_chunks("Hello World");
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.index, i);
    }
}

#[test]
fn streaming_reassemble_round_trip() {
    let emu = StreamingEmulation::new(7);
    let text = "The quick brown fox jumps over the lazy dog";
    let chunks = emu.split_into_chunks(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_split_fixed_empty() {
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_fixed("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.is_empty());
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_split_fixed_exact_size() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed("Hello");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "Hello");
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_split_fixed_multiple_chunks() {
    let emu = StreamingEmulation::new(3);
    let chunks = emu.split_fixed("abcdef");
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].content, "abc");
    assert_eq!(chunks[1].content, "def");
    assert!(chunks[1].is_final);
}

#[test]
fn streaming_split_fixed_reassemble() {
    let emu = StreamingEmulation::new(4);
    let text = "Hello World!";
    let chunks = emu.split_fixed(text);
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

// ═══════════════════════════════════════════════════════════════════════
// §15  Integration: engine + strategies + fidelity pipeline
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn full_pipeline_native_and_emulated_fidelity() {
    let native = vec![Capability::Streaming, Capability::ToolUse];
    let engine = EmulationEngine::with_defaults();
    let missing = vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
    ];
    let mut conv = simple_conv();
    let report = engine.apply(&missing, &mut conv);
    let labels = compute_fidelity(&native, &report);

    assert_eq!(labels.len(), 4);
    assert!(matches!(
        labels.get(&Capability::Streaming),
        Some(FidelityLabel::Native)
    ));
    assert!(matches!(
        labels.get(&Capability::ToolUse),
        Some(FidelityLabel::Native)
    ));
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
    assert!(matches!(
        labels.get(&Capability::StructuredOutputJsonSchema),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn full_pipeline_with_config_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate execution carefully.".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    let native = vec![Capability::Streaming];
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 2);
    assert!(matches!(
        labels.get(&Capability::CodeExecution),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn full_pipeline_disabled_not_in_fidelity() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::CodeExecution],
        &mut conv,
    );
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 1);
    assert!(labels.contains_key(&Capability::ExtendedThinking));
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn multiple_system_prompt_injections_accumulate() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let _ = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("Think step by step"));
    assert!(text.contains("Image") || text.contains("image"));
}

#[test]
fn parsed_tool_call_serde_roundtrip() {
    let call = ParsedToolCall {
        name: "search".into(),
        arguments: serde_json::json!({"q": "test"}),
    };
    let json = serde_json::to_string(&call).unwrap();
    let decoded: ParsedToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(call, decoded);
}

#[test]
fn thinking_detail_serde_roundtrip() {
    for detail in &[
        ThinkingDetail::Brief,
        ThinkingDetail::Standard,
        ThinkingDetail::Detailed,
    ] {
        let json = serde_json::to_string(detail).unwrap();
        let decoded: ThinkingDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(*detail, decoded);
    }
}

#[test]
fn emulation_report_default_is_empty() {
    let r = EmulationReport::default();
    assert!(r.is_empty());
    assert!(!r.has_unemulatable());
}

#[test]
fn fidelity_deterministic_ordering() {
    let native = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::CodeExecution,
    ];
    let report = EmulationReport::default();
    let labels1 = compute_fidelity(&native, &report);
    let labels2 = compute_fidelity(&native, &report);
    let keys1: Vec<_> = labels1.keys().collect();
    let keys2: Vec<_> = labels2.keys().collect();
    assert_eq!(keys1, keys2);
}

#[test]
fn engine_clone_produces_identical_behavior() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "custom".into(),
        },
    );
    let engine1 = EmulationEngine::new(config);
    let engine2 = engine1.clone();
    let s1 = engine1.resolve_strategy(&Capability::ExtendedThinking);
    let s2 = engine2.resolve_strategy(&Capability::ExtendedThinking);
    assert_eq!(s1, s2);
}
