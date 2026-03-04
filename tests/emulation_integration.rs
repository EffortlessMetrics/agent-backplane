// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for the emulation framework — testing emulated
//! capability behavior across the `abp-emulation` and `abp-capability` crates.

use abp_capability::{
    CapabilityRegistry, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    gemini_15_pro_manifest, generate_report, kimi_manifest, negotiate_capabilities,
    openai_gpt4o_manifest,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
use abp_emulation::strategies::{
    StreamingEmulation, ThinkingEmulation, ToolUseEmulation, VisionEmulation,
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, emulate_code_execution,
    emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
};

// =========================================================================
// Helpers
// =========================================================================

fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn user_only_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

// =========================================================================
// Module: emulation_registration
// =========================================================================
mod emulation_registration {
    use super::*;

    #[test]
    fn config_registers_and_retrieves_strategy() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::SystemPromptInjection {
                prompt: "custom thinking".into(),
            },
        );
        let engine = EmulationEngine::new(config);
        let s = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn config_handles_duplicate_registration_last_wins() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::Disabled {
                reason: "first".into(),
            },
        );
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::SystemPromptInjection {
                prompt: "second".into(),
            },
        );
        let engine = EmulationEngine::new(config);
        let s = engine.resolve_strategy(&Capability::ExtendedThinking);
        match s {
            EmulationStrategy::SystemPromptInjection { prompt } => {
                assert_eq!(prompt, "second");
            }
            other => panic!("expected SystemPromptInjection, got {other:?}"),
        }
    }

    #[test]
    fn config_returns_default_for_unregistered() {
        let engine = EmulationEngine::with_defaults();
        let s = engine.resolve_strategy(&Capability::ExtendedThinking);
        // Default for ExtendedThinking is SystemPromptInjection
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn config_iteration_over_all_strategies() {
        let mut config = EmulationConfig::new();
        config.set(Capability::ExtendedThinking, emulate_extended_thinking());
        config.set(
            Capability::StructuredOutputJsonSchema,
            emulate_structured_output(),
        );
        config.set(Capability::CodeExecution, emulate_code_execution());
        assert_eq!(config.strategies.len(), 3);
        assert!(
            config
                .strategies
                .contains_key(&Capability::ExtendedThinking)
        );
        assert!(
            config
                .strategies
                .contains_key(&Capability::StructuredOutputJsonSchema)
        );
        assert!(config.strategies.contains_key(&Capability::CodeExecution));
    }

    #[test]
    fn config_clear_resets_to_empty() {
        let mut config = EmulationConfig::new();
        config.set(Capability::ExtendedThinking, emulate_extended_thinking());
        config.set(Capability::ImageInput, emulate_image_input());
        config.strategies.clear();
        assert!(config.strategies.is_empty());
        // Engine with cleared config falls back to defaults
        let engine = EmulationEngine::new(config);
        let s = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn capability_registry_registers_and_retrieves() {
        let mut reg = CapabilityRegistry::new();
        let manifest = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("test-backend", manifest);
        assert!(reg.get("test-backend").is_some());
        assert!(reg.contains("test-backend"));
    }

    #[test]
    fn capability_registry_returns_none_for_unregistered() {
        let reg = CapabilityRegistry::new();
        assert!(reg.get("nonexistent").is_none());
        assert!(!reg.contains("nonexistent"));
    }

    #[test]
    fn capability_registry_duplicate_overwrites() {
        let mut reg = CapabilityRegistry::new();
        let m1 = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let m2 = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
        reg.register("backend", m1);
        reg.register("backend", m2);
        let m = reg.get("backend").unwrap();
        assert!(!m.contains_key(&Capability::Streaming));
        assert!(m.contains_key(&Capability::ToolUse));
    }

    #[test]
    fn capability_registry_iteration_names() {
        let mut reg = CapabilityRegistry::new();
        reg.register("a", make_manifest(&[]));
        reg.register("b", make_manifest(&[]));
        reg.register("c", make_manifest(&[]));
        let names = reg.names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn capability_registry_unregister_and_clear() {
        let mut reg = CapabilityRegistry::new();
        reg.register("x", make_manifest(&[]));
        reg.register("y", make_manifest(&[]));
        assert_eq!(reg.len(), 2);
        assert!(reg.unregister("x"));
        assert_eq!(reg.len(), 1);
        assert!(!reg.contains("x"));
        assert!(!reg.unregister("x")); // already removed
    }
}

// =========================================================================
// Module: emulation_execution
// =========================================================================
mod emulation_execution {
    use super::*;

    #[test]
    fn system_prompt_injection_produces_labeled_output() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
        let sys = conv.system_message().unwrap();
        assert!(sys.text_content().contains("Think step by step"));
    }

    #[test]
    fn emulation_output_includes_emulated_marker_in_report() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        // The report entry records the exact strategy used
        assert!(matches!(
            &report.applied[0].strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn emulator_error_propagation_disabled_warns() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("not emulated"));
    }

    #[test]
    fn emulator_with_custom_config_parameters() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ToolUse,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Use XML tags to call tools.".into(),
            },
        );
        let mut conv = user_only_conv();
        let engine = EmulationEngine::new(config);
        let report = engine.apply(&[Capability::ToolUse], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert!(conv.system_message().is_some());
        assert!(
            conv.system_message()
                .unwrap()
                .text_content()
                .contains("XML tags")
        );
    }

    #[test]
    fn streaming_emulation_produces_valid_chunk_sequence() {
        let emu = StreamingEmulation::new(10);
        let chunks = emu.split_into_chunks("Hello world, this is a test message.");

        // All chunks form a valid sequence
        assert!(!chunks.is_empty());
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
        // Only the last chunk is final
        assert!(chunks.last().unwrap().is_final);
        for chunk in &chunks[..chunks.len() - 1] {
            assert!(!chunk.is_final);
        }
        // Reassemble should recover original text
        let reassembled = StreamingEmulation::reassemble(&chunks);
        assert_eq!(reassembled, "Hello world, this is a test message.");
    }

    #[test]
    fn multiple_emulations_applied_in_order() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(
            &[
                Capability::ExtendedThinking,
                Capability::ImageInput,
                Capability::StopSequences,
            ],
            &mut conv,
        );
        // ExtendedThinking and ImageInput are SystemPromptInjection, StopSequences is PostProcessing
        assert_eq!(report.applied.len(), 3);
        assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
        assert_eq!(report.applied[1].capability, Capability::ImageInput);
        assert_eq!(report.applied[2].capability, Capability::StopSequences);
    }

    #[test]
    fn post_processing_strategy_leaves_conv_untouched() {
        let original = simple_conv();
        let mut conv = original.clone();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
        assert_eq!(conv, original);
        assert_eq!(report.applied.len(), 1);
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::PostProcessing { .. }
        ));
    }

    #[test]
    fn check_missing_dry_run_does_not_mutate() {
        let original = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report =
            engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);
        // check_missing is a dry run — should classify without applying
        assert_eq!(report.applied.len(), 1);
        assert_eq!(report.warnings.len(), 1);
        // original conversation not touched (it wasn't passed)
        assert_eq!(original.messages.len(), 2);
    }

    #[test]
    fn free_function_apply_emulation_equivalent_to_engine() {
        let config = EmulationConfig::new();
        let mut conv1 = simple_conv();
        let mut conv2 = simple_conv();

        let report1 = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv1);
        let engine = EmulationEngine::new(config.clone());
        let report2 = engine.apply(&[Capability::ExtendedThinking], &mut conv2);

        assert_eq!(report1, report2);
        assert_eq!(conv1, conv2);
    }

    #[test]
    fn empty_capabilities_produces_no_emulation() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[], &mut conv);
        assert!(report.is_empty());
        assert!(!report.has_unemulatable());
    }
}

// =========================================================================
// Module: emulation_labeling
// =========================================================================
mod emulation_labeling {
    use super::*;

    #[test]
    fn fidelity_labels_native_capabilities() {
        let native = vec![Capability::Streaming, Capability::ToolUse];
        let report = EmulationReport::default();
        let labels = compute_fidelity(&native, &report);

        assert_eq!(labels.len(), 2);
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
    }

    #[test]
    fn fidelity_labels_emulated_capabilities() {
        let native = vec![];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think step by step.".into(),
                },
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);

        assert_eq!(labels.len(), 1);
        assert!(matches!(
            labels[&Capability::ExtendedThinking],
            FidelityLabel::Emulated { .. }
        ));
    }

    #[test]
    fn fidelity_distinguishes_native_from_emulated() {
        let native = vec![Capability::Streaming];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: emulate_extended_thinking(),
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);

        assert_eq!(labels.len(), 2);
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert!(matches!(
            labels[&Capability::ExtendedThinking],
            FidelityLabel::Emulated { .. }
        ));
    }

    #[test]
    fn mixed_native_and_emulated_report_correctly() {
        let native = vec![Capability::Streaming, Capability::ToolUse];
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
            warnings: vec!["Capability CodeExecution not emulated: disabled".into()],
        };

        let labels = compute_fidelity(&native, &report);
        assert_eq!(labels.len(), 4);

        // Native caps
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
        // Emulated caps
        assert!(matches!(
            labels[&Capability::ExtendedThinking],
            FidelityLabel::Emulated { .. }
        ));
        assert!(matches!(
            labels[&Capability::StructuredOutputJsonSchema],
            FidelityLabel::Emulated { .. }
        ));
        // CodeExecution warning is NOT in the labels (it was not applied)
        assert!(!labels.contains_key(&Capability::CodeExecution));
    }

    #[test]
    fn fidelity_label_serde_roundtrip_native() {
        let label = FidelityLabel::Native;
        let json = serde_json::to_string(&label).unwrap();
        let back: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(label, back);
    }

    #[test]
    fn fidelity_label_serde_roundtrip_emulated() {
        let label = FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
        };
        let json = serde_json::to_string(&label).unwrap();
        let back: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(label, back);
    }

    #[test]
    fn label_format_consistent_across_strategies() {
        let strategies = vec![
            EmulationStrategy::SystemPromptInjection {
                prompt: "foo".into(),
            },
            EmulationStrategy::PostProcessing {
                detail: "bar".into(),
            },
        ];
        for strategy in strategies {
            let label = FidelityLabel::Emulated {
                strategy: strategy.clone(),
            };
            let json = serde_json::to_string(&label).unwrap();
            // All emulated labels use the "emulated" fidelity tag
            assert!(json.contains("\"fidelity\":\"emulated\""));
            // And contain a "strategy" key with a "type" discriminator
            assert!(json.contains("\"strategy\""));
            assert!(json.contains("\"type\""));
        }
    }

    #[test]
    fn native_label_format_consistent() {
        let label = FidelityLabel::Native;
        let json = serde_json::to_string(&label).unwrap();
        assert!(json.contains("\"fidelity\":\"native\""));
    }

    #[test]
    fn warnings_excluded_from_fidelity_labels() {
        let native = vec![];
        let report = EmulationReport {
            applied: vec![],
            warnings: vec![
                "Capability CodeExecution not emulated: unsafe".into(),
                "Capability Streaming not emulated: not available".into(),
            ],
        };
        let labels = compute_fidelity(&native, &report);
        assert!(labels.is_empty());
    }

    #[test]
    fn fidelity_labels_btreemap_is_deterministically_ordered() {
        let native = vec![Capability::Streaming, Capability::ToolUse];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: emulate_extended_thinking(),
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);
        let keys: Vec<_> = labels.keys().collect();
        // BTreeMap sorts by key — verify sorted order is stable
        for w in keys.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }
}

// =========================================================================
// Module: emulation_capability_matrix
// =========================================================================
mod emulation_capability_matrix {
    use super::*;

    // Helper: check whether a capability is native, emulated, or unsupported
    // in a given manifest.
    fn classify(manifest: &CapabilityManifest, cap: &Capability) -> &'static str {
        match manifest.get(cap) {
            Some(CoreSupportLevel::Native) => "native",
            Some(CoreSupportLevel::Emulated) => "emulated",
            Some(CoreSupportLevel::Restricted { .. }) => "restricted",
            Some(CoreSupportLevel::Unsupported) => "unsupported",
            None => "absent",
        }
    }

    #[test]
    fn tool_use_across_dialects() {
        assert_eq!(
            classify(&openai_gpt4o_manifest(), &Capability::ToolUse),
            "native"
        );
        assert_eq!(
            classify(&claude_35_sonnet_manifest(), &Capability::ToolUse),
            "native"
        );
        assert_eq!(
            classify(&gemini_15_pro_manifest(), &Capability::ToolUse),
            "native"
        );
        assert_eq!(classify(&kimi_manifest(), &Capability::ToolUse), "native");
        assert_eq!(classify(&codex_manifest(), &Capability::ToolUse), "native");
        assert_eq!(
            classify(&copilot_manifest(), &Capability::ToolUse),
            "native"
        );
    }

    #[test]
    fn streaming_across_dialects() {
        assert_eq!(
            classify(&openai_gpt4o_manifest(), &Capability::Streaming),
            "native"
        );
        assert_eq!(
            classify(&claude_35_sonnet_manifest(), &Capability::Streaming),
            "native"
        );
        assert_eq!(
            classify(&gemini_15_pro_manifest(), &Capability::Streaming),
            "native"
        );
        assert_eq!(classify(&kimi_manifest(), &Capability::Streaming), "native");
        assert_eq!(
            classify(&codex_manifest(), &Capability::Streaming),
            "native"
        );
        assert_eq!(
            classify(&copilot_manifest(), &Capability::Streaming),
            "native"
        );
    }

    #[test]
    fn extended_thinking_across_dialects() {
        // Only Claude supports extended thinking natively
        assert_eq!(
            classify(&claude_35_sonnet_manifest(), &Capability::ExtendedThinking),
            "native"
        );
        assert_eq!(
            classify(&openai_gpt4o_manifest(), &Capability::ExtendedThinking),
            "unsupported"
        );
        assert_eq!(
            classify(&gemini_15_pro_manifest(), &Capability::ExtendedThinking),
            "unsupported"
        );
        assert_eq!(
            classify(&kimi_manifest(), &Capability::ExtendedThinking),
            "unsupported"
        );
        assert_eq!(
            classify(&codex_manifest(), &Capability::ExtendedThinking),
            "unsupported"
        );
        assert_eq!(
            classify(&copilot_manifest(), &Capability::ExtendedThinking),
            "unsupported"
        );
    }

    #[test]
    fn vision_across_dialects() {
        assert_eq!(
            classify(&openai_gpt4o_manifest(), &Capability::Vision),
            "native"
        );
        assert_eq!(
            classify(&claude_35_sonnet_manifest(), &Capability::Vision),
            "native"
        );
        assert_eq!(
            classify(&gemini_15_pro_manifest(), &Capability::Vision),
            "native"
        );
        assert_eq!(classify(&kimi_manifest(), &Capability::Vision), "native");
        assert_eq!(classify(&codex_manifest(), &Capability::Vision), "emulated");
        assert_eq!(
            classify(&copilot_manifest(), &Capability::Vision),
            "emulated"
        );
    }

    #[test]
    fn tool_use_emulation_injects_prompt() {
        let tools = vec![IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk.".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }];
        let mut conv = user_only_conv();
        ToolUseEmulation::inject_tools(&mut conv, &tools);
        let sys = conv.system_message().unwrap();
        let text = sys.text_content();
        assert!(text.contains("read_file"));
        assert!(text.contains("tool_call"));
    }

    #[test]
    fn tool_use_emulation_parses_tool_calls() {
        let text = r#"Some text <tool_call>
{"name": "read_file", "arguments": {"path": "/tmp/foo.txt"}}
</tool_call> and more text"#;
        let calls = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        let call = calls[0].as_ref().unwrap();
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments["path"], "/tmp/foo.txt");
    }

    #[test]
    fn tool_use_emulation_extracts_text_outside_calls() {
        let text = "Hello <tool_call>\n{\"name\":\"x\",\"arguments\":{}}\n</tool_call> world";
        let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
        assert_eq!(outside, "Hello  world");
    }

    #[test]
    fn streaming_emulation_split_and_reassemble() {
        let emu = StreamingEmulation::default_chunk_size();
        let text = "The quick brown fox jumps over the lazy dog.";
        let chunks = emu.split_into_chunks(text);
        assert!(chunks.len() > 1);
        let reassembled = StreamingEmulation::reassemble(&chunks);
        assert_eq!(reassembled, text);
    }

    #[test]
    fn streaming_emulation_empty_input() {
        let emu = StreamingEmulation::new(10);
        let chunks = emu.split_into_chunks("");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_final);
        assert!(chunks[0].content.is_empty());
    }

    #[test]
    fn streaming_emulation_fixed_split() {
        let emu = StreamingEmulation::new(5);
        let chunks = emu.split_fixed("abcdefghij");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].content, "abcde");
        assert_eq!(chunks[1].content, "fghij");
        assert!(!chunks[0].is_final);
        assert!(chunks[1].is_final);
    }

    #[test]
    fn thinking_emulation_inject_brief() {
        let mut conv = user_only_conv();
        let emu = ThinkingEmulation::brief();
        emu.inject(&mut conv);
        let sys = conv.system_message().unwrap();
        assert!(sys.text_content().contains("Think step by step"));
    }

    #[test]
    fn thinking_emulation_inject_standard_with_tags() {
        let mut conv = user_only_conv();
        let emu = ThinkingEmulation::standard();
        emu.inject(&mut conv);
        let sys = conv.system_message().unwrap();
        assert!(sys.text_content().contains("<thinking>"));
    }

    #[test]
    fn thinking_emulation_inject_detailed_with_steps() {
        let mut conv = user_only_conv();
        let emu = ThinkingEmulation::detailed();
        emu.inject(&mut conv);
        let sys = conv.system_message().unwrap();
        let text = sys.text_content();
        assert!(text.contains("sub-problems"));
        assert!(text.contains("Verify"));
    }

    #[test]
    fn thinking_emulation_extract_thinking_from_response() {
        let resp = "<thinking>I need to reason carefully.</thinking>The answer is 42.";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(resp);
        assert_eq!(thinking, "I need to reason carefully.");
        assert_eq!(answer, "The answer is 42.");
    }

    #[test]
    fn thinking_emulation_no_tags_returns_empty_thinking() {
        let resp = "Just a plain answer.";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(resp);
        assert!(thinking.is_empty());
        assert_eq!(answer, "Just a plain answer.");
    }

    #[test]
    fn thinking_emulation_to_thinking_block() {
        let resp = "<thinking>Step 1: analyze</thinking>Final answer.";
        let block = ThinkingEmulation::to_thinking_block(resp);
        assert!(block.is_some());
        if let Some(IrContentBlock::Thinking { text }) = block {
            assert_eq!(text, "Step 1: analyze");
        }
    }

    #[test]
    fn thinking_emulation_no_block_when_no_tags() {
        let block = ThinkingEmulation::to_thinking_block("No thinking here.");
        assert!(block.is_none());
    }

    #[test]
    fn vision_emulation_replaces_images_with_placeholders() {
        let mut conv = IrConversation::new().push(IrMessage::new(
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
        ));
        let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
        assert_eq!(count, 1);
        let user_text = conv.messages[0].text_content();
        assert!(user_text.contains("[Image"));
        assert!(user_text.contains("does not support vision"));
    }

    #[test]
    fn vision_emulation_full_apply_injects_fallback_prompt() {
        let mut conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "abc123".into(),
            }],
        ));
        let count = VisionEmulation::apply(&mut conv);
        assert_eq!(count, 1);
        let sys = conv.system_message().unwrap();
        assert!(sys.text_content().contains("does not support vision"));
    }

    #[test]
    fn vision_emulation_has_images_detection() {
        let with_image = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            }],
        ));
        let without_image = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        assert!(VisionEmulation::has_images(&with_image));
        assert!(!VisionEmulation::has_images(&without_image));
    }

    #[test]
    fn vision_emulation_no_images_is_noop() {
        let mut conv = user_only_conv();
        let count = VisionEmulation::apply(&mut conv);
        assert_eq!(count, 0);
        // No system message injected when no images replaced
        assert!(conv.system_message().is_none());
    }

    #[test]
    fn default_strategies_cover_all_emulatable_caps() {
        // Verify every pre-configured named strategy returns a non-disabled variant
        let structured = emulate_structured_output();
        assert!(matches!(
            structured,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
        let code = emulate_code_execution();
        assert!(matches!(
            code,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
        let thinking = emulate_extended_thinking();
        assert!(matches!(
            thinking,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
        let image = emulate_image_input();
        assert!(matches!(
            image,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
        let stop = emulate_stop_sequences();
        assert!(matches!(stop, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn can_emulate_matrix_for_key_capabilities() {
        // Emulatable
        assert!(can_emulate(&Capability::ExtendedThinking));
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
        assert!(can_emulate(&Capability::ImageInput));
        assert!(can_emulate(&Capability::StopSequences));
        // Not emulatable by default
        assert!(!can_emulate(&Capability::CodeExecution));
        assert!(!can_emulate(&Capability::Streaming));
        assert!(!can_emulate(&Capability::ToolUse));
        assert!(!can_emulate(&Capability::Vision));
    }

    #[test]
    fn negotiation_classifies_emulated_as_emulated() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert_eq!(result.native, vec![Capability::Streaming]);
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.emulated[0].0, Capability::ToolUse);
    }

    #[test]
    fn registry_with_defaults_has_all_six_dialects() {
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
    fn registry_negotiate_by_name_works() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg
            .negotiate_by_name(
                "anthropic/claude-3.5-sonnet",
                &[Capability::ExtendedThinking, Capability::Vision],
            )
            .unwrap();
        // Claude supports both natively
        assert_eq!(result.native.len(), 2);
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn registry_compare_detects_gaps() {
        let reg = CapabilityRegistry::with_defaults();
        // Claude → OpenAI: ExtendedThinking is native in Claude, unsupported in OpenAI
        let result = reg
            .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
            .unwrap();
        let unsup_caps: Vec<_> = result.unsupported.iter().map(|(c, _)| c.clone()).collect();
        assert!(unsup_caps.contains(&Capability::ExtendedThinking));
    }

    #[test]
    fn compatibility_report_for_full_native() {
        let manifest = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 2);
        assert_eq!(report.emulated_count, 0);
        assert_eq!(report.unsupported_count, 0);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn tool_use_emulation_multiple_tool_calls() {
        let text = r#"Let me call two tools.
<tool_call>
{"name": "read_file", "arguments": {"path": "a.txt"}}
</tool_call>
<tool_call>
{"name": "write_file", "arguments": {"path": "b.txt", "content": "hello"}}
</tool_call>"#;
        let calls = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].as_ref().unwrap().name, "read_file");
        assert_eq!(calls[1].as_ref().unwrap().name, "write_file");
    }

    #[test]
    fn tool_use_emulation_invalid_json_returns_error() {
        let text = "<tool_call>not valid json</tool_call>";
        let calls = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert!(calls[0].is_err());
    }

    #[test]
    fn tool_use_emulation_to_tool_use_block() {
        let parsed = abp_emulation::strategies::ParsedToolCall {
            name: "bash".into(),
            arguments: serde_json::json!({"cmd": "ls"}),
        };
        let block = ToolUseEmulation::to_tool_use_block(&parsed, "call-001");
        match block {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call-001");
                assert_eq!(name, "bash");
                assert_eq!(input["cmd"], "ls");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_emulation_format_tool_result() {
        let ok = ToolUseEmulation::format_tool_result("bash", "success", false);
        assert!(ok.contains("returned:"));
        assert!(!ok.contains("error"));

        let err = ToolUseEmulation::format_tool_result("bash", "failed", true);
        assert!(err.contains("error"));
    }
}
