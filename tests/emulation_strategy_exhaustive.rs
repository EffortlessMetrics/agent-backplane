#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive emulation strategy tests for Agent Backplane.
//!
//! Covers: thinking-as-text, system-as-user, images-as-placeholder,
//! strip-thinking, tool emulation, streaming emulation, JSON mode,
//! graceful degradation, emulation labeling, and metadata tracking.

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_emulation::strategies::{
    ParsedToolCall, StreamChunk, StreamingEmulation, ThinkingDetail, ThinkingEmulation,
    ToolUseEmulation, VisionEmulation,
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, default_strategy,
    emulate_code_execution, emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
};
use serde_json::json;
use std::collections::BTreeMap;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn user_only_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn conv_with_images() -> IrConversation {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Describe this image:".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    );
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a vision model."))
        .push(msg)
}

fn conv_with_multiple_images() -> IrConversation {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data1".into(),
            },
            IrContentBlock::Text {
                text: "Compare these:".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "data2".into(),
            },
        ],
    );
    IrConversation::new().push(msg)
}

fn conv_with_thinking_response() -> String {
    "<thinking>Let me analyze this step by step. First I need to consider the options.</thinking>The answer is 42.".into()
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        },
        IrToolDefinition {
            name: "calculate".into(),
            description: "Perform math calculations".into(),
            parameters: json!({"type": "object", "properties": {"expression": {"type": "string"}}}),
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Thinking as Text
// ═══════════════════════════════════════════════════════════════════════

mod thinking_as_text {
    use super::*;

    #[test]
    fn extract_thinking_basic() {
        let text = "<thinking>step 1, step 2</thinking>Final answer.";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert_eq!(thinking, "step 1, step 2");
        assert_eq!(answer, "Final answer.");
    }

    #[test]
    fn extract_thinking_no_tags() {
        let text = "Just a plain response.";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert!(thinking.is_empty());
        assert_eq!(answer, "Just a plain response.");
    }

    #[test]
    fn extract_thinking_empty_tags() {
        let text = "<thinking></thinking>Answer here.";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert!(thinking.is_empty());
        assert_eq!(answer, "Answer here.");
    }

    #[test]
    fn extract_thinking_with_leading_text() {
        let text = "Preamble <thinking>inner reasoning</thinking> conclusion";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert_eq!(thinking, "inner reasoning");
        assert!(answer.contains("Preamble"));
        assert!(answer.contains("conclusion"));
    }

    #[test]
    fn extract_thinking_multiline() {
        let text = "<thinking>\nLine 1\nLine 2\nLine 3\n</thinking>\nThe multiline answer is here.";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert!(thinking.contains("Line 1"));
        assert!(thinking.contains("Line 3"));
        assert!(answer.contains("multiline answer"));
    }

    #[test]
    fn to_thinking_block_produces_block() {
        let text = "<thinking>deep analysis</thinking>result";
        let block = ThinkingEmulation::to_thinking_block(text);
        assert!(block.is_some());
        match block.unwrap() {
            IrContentBlock::Thinking { text } => assert_eq!(text, "deep analysis"),
            _ => panic!("expected Thinking block"),
        }
    }

    #[test]
    fn to_thinking_block_returns_none_for_no_thinking() {
        let text = "no thinking tags here";
        let block = ThinkingEmulation::to_thinking_block(text);
        assert!(block.is_none());
    }

    #[test]
    fn thinking_brief_prompt_text() {
        let emu = ThinkingEmulation::brief();
        let prompt = emu.prompt_text();
        assert!(prompt.contains("Think step by step"));
        assert!(!prompt.contains("<thinking>"));
    }

    #[test]
    fn thinking_standard_prompt_text_contains_xml_markers() {
        let emu = ThinkingEmulation::standard();
        let prompt = emu.prompt_text();
        assert!(prompt.contains("<thinking>"));
        assert!(prompt.contains("</thinking>"));
    }

    #[test]
    fn thinking_detailed_prompt_includes_verification() {
        let emu = ThinkingEmulation::detailed();
        let prompt = emu.prompt_text();
        assert!(prompt.contains("Verify"));
        assert!(prompt.contains("sub-problem"));
    }

    #[test]
    fn thinking_inject_appends_to_existing_system() {
        let mut conv = simple_conv();
        let emu = ThinkingEmulation::standard();
        emu.inject(&mut conv);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("You are helpful."));
        assert!(sys_text.contains("<thinking>"));
    }

    #[test]
    fn thinking_inject_creates_system_when_missing() {
        let mut conv = user_only_conv();
        let emu = ThinkingEmulation::brief();
        emu.inject(&mut conv);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert!(
            conv.messages[0]
                .text_content()
                .contains("Think step by step")
        );
    }

    #[test]
    fn thinking_detail_brief_is_shortest() {
        let brief = ThinkingEmulation::brief().prompt_text().len();
        let standard = ThinkingEmulation::standard().prompt_text().len();
        let detailed = ThinkingEmulation::detailed().prompt_text().len();
        assert!(brief < standard);
        assert!(standard < detailed);
    }

    #[test]
    fn thinking_extract_preserves_whitespace_in_answer() {
        let text = "<thinking>reason</thinking>  spaced answer  ";
        let (_, answer) = ThinkingEmulation::extract_thinking(text);
        assert_eq!(answer, "spaced answer");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  System as User (system prompt injection emulation)
// ═══════════════════════════════════════════════════════════════════════

mod system_as_user {
    use super::*;

    #[test]
    fn engine_injects_system_prompt_for_extended_thinking() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Think step by step"));
        assert_eq!(report.applied.len(), 1);
    }

    #[test]
    fn engine_creates_system_msg_when_absent() {
        let mut conv = user_only_conv();
        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::ExtendedThinking], &mut conv);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn system_injection_preserves_original_system_content() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::ExtendedThinking], &mut conv);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("You are helpful."));
    }

    #[test]
    fn multiple_system_injections_accumulate() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        engine.apply(
            &[Capability::ExtendedThinking, Capability::ImageInput],
            &mut conv,
        );
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Think step by step"));
        assert!(sys_text.contains("Image inputs"));
    }

    #[test]
    fn system_injection_does_not_add_extra_messages() {
        let mut conv = simple_conv();
        let original_len = conv.len();
        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::ExtendedThinking], &mut conv);
        assert_eq!(conv.len(), original_len);
    }

    #[test]
    fn system_injection_with_custom_prompt() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Custom thinking prompt here.".into(),
            },
        );
        let mut conv = simple_conv();
        let engine = EmulationEngine::new(config);
        engine.apply(&[Capability::ExtendedThinking], &mut conv);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Custom thinking prompt here."));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Images as Placeholder
// ═══════════════════════════════════════════════════════════════════════

mod images_as_placeholder {
    use super::*;

    #[test]
    fn replace_single_image() {
        let mut conv = conv_with_images();
        let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
        assert_eq!(count, 1);
    }

    #[test]
    fn replace_multiple_images() {
        let mut conv = conv_with_multiple_images();
        let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
        assert_eq!(count, 2);
    }

    #[test]
    fn replaced_image_contains_media_type() {
        let mut conv = conv_with_images();
        VisionEmulation::replace_images_with_placeholders(&mut conv);
        let user_text = conv.messages.last().unwrap().text_content();
        assert!(user_text.contains("image/png"));
    }

    #[test]
    fn replaced_image_has_placeholder_marker() {
        let mut conv = conv_with_images();
        VisionEmulation::replace_images_with_placeholders(&mut conv);
        let user_text = conv.messages.last().unwrap().text_content();
        assert!(user_text.contains("[Image"));
        assert!(user_text.contains("does not support vision"));
    }

    #[test]
    fn text_content_preserved_after_image_replacement() {
        let mut conv = conv_with_images();
        VisionEmulation::replace_images_with_placeholders(&mut conv);
        let user_text = conv.messages.last().unwrap().text_content();
        assert!(user_text.contains("Describe this image:"));
    }

    #[test]
    fn no_images_returns_zero() {
        let mut conv = simple_conv();
        let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
        assert_eq!(count, 0);
    }

    #[test]
    fn has_images_true_when_images_present() {
        let conv = conv_with_images();
        assert!(VisionEmulation::has_images(&conv));
    }

    #[test]
    fn has_images_false_when_no_images() {
        let conv = simple_conv();
        assert!(!VisionEmulation::has_images(&conv));
    }

    #[test]
    fn apply_replaces_and_injects_prompt() {
        let mut conv = conv_with_images();
        let count = VisionEmulation::apply(&mut conv);
        assert_eq!(count, 1);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("image(s) were provided"));
    }

    #[test]
    fn apply_on_no_images_does_nothing() {
        let original = simple_conv();
        let mut conv = original.clone();
        VisionEmulation::apply(&mut conv);
        assert_eq!(conv, original);
    }

    #[test]
    fn vision_fallback_prompt_skipped_when_zero() {
        let mut conv = simple_conv();
        VisionEmulation::inject_vision_fallback_prompt(&mut conv, 0);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(!sys_text.contains("image"));
    }

    #[test]
    fn vision_fallback_prompt_includes_count() {
        let mut conv = simple_conv();
        VisionEmulation::inject_vision_fallback_prompt(&mut conv, 3);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("3 image(s)"));
    }

    #[test]
    fn multiple_image_types_in_placeholders() {
        let mut conv = conv_with_multiple_images();
        VisionEmulation::replace_images_with_placeholders(&mut conv);
        let user_text = conv.messages.last().unwrap().text_content();
        assert!(user_text.contains("image/png"));
        assert!(user_text.contains("image/jpeg"));
    }

    #[test]
    fn image_replacement_preserves_message_count() {
        let mut conv = conv_with_images();
        let original_len = conv.len();
        VisionEmulation::replace_images_with_placeholders(&mut conv);
        assert_eq!(conv.len(), original_len);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Strip Thinking
// ═══════════════════════════════════════════════════════════════════════

mod strip_thinking {
    use super::*;

    #[test]
    fn extract_removes_thinking_tags() {
        let text = conv_with_thinking_response();
        let (_, answer) = ThinkingEmulation::extract_thinking(&text);
        assert!(!answer.contains("<thinking>"));
        assert!(!answer.contains("</thinking>"));
    }

    #[test]
    fn extract_returns_only_answer() {
        let text = conv_with_thinking_response();
        let (_, answer) = ThinkingEmulation::extract_thinking(&text);
        assert_eq!(answer, "The answer is 42.");
    }

    #[test]
    fn strip_thinking_from_nested_content() {
        let text = "Before<thinking>hidden reasoning</thinking>After";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert_eq!(thinking, "hidden reasoning");
        assert!(answer.contains("Before"));
        assert!(answer.contains("After"));
    }

    #[test]
    fn strip_thinking_only_tags_present() {
        let text = "<thinking>all reasoning</thinking>";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert_eq!(thinking, "all reasoning");
        assert!(answer.is_empty());
    }

    #[test]
    fn strip_thinking_preserves_non_thinking_text() {
        let text = "Important context\n<thinking>hidden</thinking>\nMore text";
        let (_, answer) = ThinkingEmulation::extract_thinking(text);
        assert!(answer.contains("Important context"));
        assert!(answer.contains("More text"));
    }

    #[test]
    fn strip_thinking_unclosed_tag_returns_original() {
        let text = "<thinking>unclosed reasoning without end tag";
        let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
        assert!(thinking.is_empty());
        assert_eq!(answer, text);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5  Tool Emulation
// ═══════════════════════════════════════════════════════════════════════

mod tool_emulation {
    use super::*;

    #[test]
    fn tools_to_prompt_empty_list() {
        let prompt = ToolUseEmulation::tools_to_prompt(&[]);
        assert!(prompt.is_empty());
    }

    #[test]
    fn tools_to_prompt_contains_tool_names() {
        let tools = sample_tools();
        let prompt = ToolUseEmulation::tools_to_prompt(&tools);
        assert!(prompt.contains("search"));
        assert!(prompt.contains("calculate"));
    }

    #[test]
    fn tools_to_prompt_contains_descriptions() {
        let tools = sample_tools();
        let prompt = ToolUseEmulation::tools_to_prompt(&tools);
        assert!(prompt.contains("Search the web"));
        assert!(prompt.contains("Perform math calculations"));
    }

    #[test]
    fn tools_to_prompt_contains_xml_call_format() {
        let tools = sample_tools();
        let prompt = ToolUseEmulation::tools_to_prompt(&tools);
        assert!(prompt.contains("<tool_call>"));
        assert!(prompt.contains("</tool_call>"));
    }

    #[test]
    fn parse_single_tool_call() {
        let text = r#"<tool_call>{"name": "search", "arguments": {"query": "rust"}}</tool_call>"#;
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(results.len(), 1);
        let call = results[0].as_ref().unwrap();
        assert_eq!(call.name, "search");
        assert_eq!(call.arguments["query"], "rust");
    }

    #[test]
    fn parse_multiple_tool_calls() {
        let text = r#"
<tool_call>{"name": "search", "arguments": {"query": "rust"}}</tool_call>
<tool_call>{"name": "calculate", "arguments": {"expression": "2+2"}}</tool_call>
"#;
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn parse_tool_call_invalid_json() {
        let text = "<tool_call>not json</tool_call>";
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
        assert!(results[0].as_ref().unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn parse_tool_call_missing_name() {
        let text = r#"<tool_call>{"arguments": {"a": 1}}</tool_call>"#;
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
        assert!(results[0].as_ref().unwrap_err().contains("missing 'name'"));
    }

    #[test]
    fn parse_tool_call_unclosed_tag() {
        let text = r#"<tool_call>{"name": "test", "arguments": {}}"#;
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
        assert!(results[0].as_ref().unwrap_err().contains("unclosed"));
    }

    #[test]
    fn parse_no_tool_calls() {
        let text = "Just regular text with no tool calls.";
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert!(results.is_empty());
    }

    #[test]
    fn to_tool_use_block_structure() {
        let call = ParsedToolCall {
            name: "search".into(),
            arguments: json!({"query": "test"}),
        };
        let block = ToolUseEmulation::to_tool_use_block(&call, "tool-123");
        match block {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tool-123");
                assert_eq!(name, "search");
                assert_eq!(input["query"], "test");
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn inject_tools_into_system_message() {
        let mut conv = simple_conv();
        let tools = sample_tools();
        ToolUseEmulation::inject_tools(&mut conv, &tools);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("search"));
        assert!(sys_text.contains("<tool_call>"));
    }

    #[test]
    fn inject_tools_creates_system_if_missing() {
        let mut conv = user_only_conv();
        let tools = sample_tools();
        ToolUseEmulation::inject_tools(&mut conv, &tools);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn inject_empty_tools_is_noop() {
        let original = simple_conv();
        let mut conv = original.clone();
        ToolUseEmulation::inject_tools(&mut conv, &[]);
        assert_eq!(conv, original);
    }

    #[test]
    fn format_tool_result_success() {
        let result = ToolUseEmulation::format_tool_result("search", "found 5 results", false);
        assert!(result.contains("search"));
        assert!(result.contains("returned:"));
        assert!(!result.contains("error"));
    }

    #[test]
    fn format_tool_result_error() {
        let result = ToolUseEmulation::format_tool_result("search", "timeout", true);
        assert!(result.contains("error"));
        assert!(result.contains("timeout"));
    }

    #[test]
    fn extract_text_outside_tool_calls() {
        let text = "Hello <tool_call>{\"name\":\"x\",\"arguments\":{}}</tool_call> World";
        let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
        assert!(outside.contains("Hello"));
        assert!(outside.contains("World"));
        assert!(!outside.contains("tool_call"));
    }

    #[test]
    fn extract_text_no_tool_calls_returns_original() {
        let text = "Just plain text.";
        let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
        assert_eq!(outside, "Just plain text.");
    }

    #[test]
    fn tool_call_with_null_arguments() {
        let text = r#"<tool_call>{"name": "noop"}</tool_call>"#;
        let results = ToolUseEmulation::parse_tool_calls(text);
        assert_eq!(results.len(), 1);
        let call = results[0].as_ref().unwrap();
        assert_eq!(call.name, "noop");
        assert!(call.arguments.is_null());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6  Streaming Emulation
// ═══════════════════════════════════════════════════════════════════════

mod streaming_emulation {
    use super::*;

    #[test]
    fn split_empty_text() {
        let emu = StreamingEmulation::new(10);
        let chunks = emu.split_into_chunks("");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.is_empty());
        assert!(chunks[0].is_final);
    }

    #[test]
    fn split_short_text_single_chunk() {
        let emu = StreamingEmulation::new(100);
        let chunks = emu.split_into_chunks("Hello");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello");
        assert!(chunks[0].is_final);
    }

    #[test]
    fn split_text_respects_word_boundaries() {
        let emu = StreamingEmulation::new(10);
        let chunks = emu.split_into_chunks("Hello beautiful world out there");
        for chunk in &chunks[..chunks.len() - 1] {
            assert!(!chunk.is_final);
        }
        assert!(chunks.last().unwrap().is_final);
    }

    #[test]
    fn split_fixed_ignores_word_boundaries() {
        let emu = StreamingEmulation::new(3);
        let chunks = emu.split_fixed("abcdef");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].content, "abc");
        assert_eq!(chunks[1].content, "def");
    }

    #[test]
    fn split_fixed_empty_text() {
        let emu = StreamingEmulation::new(5);
        let chunks = emu.split_fixed("");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.is_empty());
        assert!(chunks[0].is_final);
    }

    #[test]
    fn chunk_indices_are_sequential() {
        let emu = StreamingEmulation::new(5);
        let chunks = emu.split_into_chunks("This is a longer text for chunking.");
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }

    #[test]
    fn only_last_chunk_is_final() {
        let emu = StreamingEmulation::new(5);
        let chunks = emu.split_into_chunks("A longer text that needs multiple chunks.");
        for chunk in &chunks[..chunks.len() - 1] {
            assert!(!chunk.is_final);
        }
        assert!(chunks.last().unwrap().is_final);
    }

    #[test]
    fn reassemble_produces_original() {
        let emu = StreamingEmulation::new(8);
        let text = "The quick brown fox jumps over the lazy dog.";
        let chunks = emu.split_into_chunks(text);
        let reassembled = StreamingEmulation::reassemble(&chunks);
        assert_eq!(reassembled, text);
    }

    #[test]
    fn reassemble_fixed_chunks() {
        let emu = StreamingEmulation::new(4);
        let text = "abcdefghijklmnop";
        let chunks = emu.split_fixed(text);
        let reassembled = StreamingEmulation::reassemble(&chunks);
        assert_eq!(reassembled, text);
    }

    #[test]
    fn minimum_chunk_size_is_one() {
        let emu = StreamingEmulation::new(0);
        assert_eq!(emu.chunk_size(), 1);
    }

    #[test]
    fn default_chunk_size() {
        let emu = StreamingEmulation::default_chunk_size();
        assert_eq!(emu.chunk_size(), 20);
    }

    #[test]
    fn chunk_serde_roundtrip() {
        let chunk = StreamChunk {
            content: "hello".into(),
            index: 0,
            is_final: true,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let decoded: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, decoded);
    }

    #[test]
    fn large_text_chunked_correctly() {
        let text = "word ".repeat(200);
        let emu = StreamingEmulation::new(20);
        let chunks = emu.split_into_chunks(text.trim());
        assert!(chunks.len() > 1);
        let reassembled = StreamingEmulation::reassemble(&chunks);
        assert_eq!(reassembled, text.trim());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7  JSON Mode Emulation
// ═══════════════════════════════════════════════════════════════════════

mod json_mode_emulation {
    use super::*;

    #[test]
    fn structured_output_default_is_post_processing() {
        let s = default_strategy(&Capability::StructuredOutputJsonSchema);
        assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn emulate_structured_output_is_system_injection() {
        let s = emulate_structured_output();
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn emulate_structured_output_mentions_json() {
        let s = emulate_structured_output();
        if let EmulationStrategy::SystemPromptInjection { prompt } = s {
            assert!(prompt.contains("JSON"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn json_mode_can_be_emulated() {
        // StructuredOutputJsonSchema has a non-disabled default
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    }

    #[test]
    fn json_mode_post_processing_does_not_mutate() {
        let original = simple_conv();
        let mut conv = original.clone();
        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
        assert_eq!(conv, original);
    }

    #[test]
    fn json_mode_custom_override_injects_prompt() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::StructuredOutputJsonSchema,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Return valid JSON only.".into(),
            },
        );
        let mut conv = simple_conv();
        let engine = EmulationEngine::new(config);
        engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Return valid JSON only."));
    }

    #[test]
    fn json_mode_emulation_recorded_in_report() {
        let mut conv = simple_conv();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
        assert_eq!(report.applied.len(), 1);
        assert_eq!(
            report.applied[0].capability,
            Capability::StructuredOutputJsonSchema
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8  Feature Graceful Degradation
// ═══════════════════════════════════════════════════════════════════════

mod graceful_degradation {
    use super::*;

    #[test]
    fn disabled_capability_generates_warning() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[Capability::CodeExecution]);
        assert!(report.has_unemulatable());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn disabled_capability_not_in_applied() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[Capability::CodeExecution]);
        assert!(report.applied.is_empty());
    }

    #[test]
    fn emulatable_capability_in_applied() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[Capability::ExtendedThinking]);
        assert!(!report.applied.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn mixed_capabilities_split_correctly() {
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
    fn all_disabled_returns_only_warnings() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[
            Capability::CodeExecution,
            Capability::Streaming,
            Capability::ToolUse,
        ]);
        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 3);
    }

    #[test]
    fn empty_capabilities_produces_empty_report() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[]);
        assert!(report.is_empty());
    }

    #[test]
    fn disabled_apply_does_not_modify_conversation() {
        let original = simple_conv();
        let mut conv = original.clone();
        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::CodeExecution], &mut conv);
        assert_eq!(conv, original);
    }

    #[test]
    fn warning_message_contains_capability_name() {
        let engine = EmulationEngine::with_defaults();
        let report = engine.check_missing(&[Capability::CodeExecution]);
        assert!(report.warnings[0].contains("CodeExecution"));
    }

    #[test]
    fn can_emulate_returns_false_for_disabled_defaults() {
        assert!(!can_emulate(&Capability::CodeExecution));
        assert!(!can_emulate(&Capability::Streaming));
        assert!(!can_emulate(&Capability::ToolUse));
        assert!(!can_emulate(&Capability::ToolRead));
    }

    #[test]
    fn can_emulate_returns_true_for_available_strategies() {
        assert!(can_emulate(&Capability::ExtendedThinking));
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
        assert!(can_emulate(&Capability::ImageInput));
        assert!(can_emulate(&Capability::StopSequences));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9  Emulation Labeling in Receipts
// ═══════════════════════════════════════════════════════════════════════

mod emulation_labeling {
    use super::*;

    #[test]
    fn compute_fidelity_native_capabilities() {
        let native = vec![Capability::Streaming, Capability::ToolUse];
        let report = EmulationReport::default();
        let labels = compute_fidelity(&native, &report);
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
    }

    #[test]
    fn compute_fidelity_emulated_capabilities() {
        let native = vec![];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think.".into(),
                },
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);
        assert!(matches!(
            labels[&Capability::ExtendedThinking],
            FidelityLabel::Emulated { .. }
        ));
    }

    #[test]
    fn compute_fidelity_mixed() {
        let native = vec![Capability::Streaming];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think.".into(),
                },
            }],
            warnings: vec!["CodeExecution not available".into()],
        };
        let labels = compute_fidelity(&native, &report);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
        assert!(matches!(
            labels[&Capability::ExtendedThinking],
            FidelityLabel::Emulated { .. }
        ));
        // Warnings not in labels
        assert!(!labels.contains_key(&Capability::CodeExecution));
    }

    #[test]
    fn fidelity_label_native_serde() {
        let label = FidelityLabel::Native;
        let json = serde_json::to_string(&label).unwrap();
        let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(label, decoded);
    }

    #[test]
    fn fidelity_label_emulated_serde() {
        let label = FidelityLabel::Emulated {
            strategy: EmulationStrategy::PostProcessing {
                detail: "test".into(),
            },
        };
        let json = serde_json::to_string(&label).unwrap();
        let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(label, decoded);
    }

    #[test]
    fn fidelity_label_emulated_contains_strategy() {
        let label = FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "Think step by step.".into(),
            },
        };
        if let FidelityLabel::Emulated { strategy } = &label {
            assert!(matches!(
                strategy,
                EmulationStrategy::SystemPromptInjection { .. }
            ));
        } else {
            panic!("expected Emulated");
        }
    }

    #[test]
    fn compute_fidelity_empty_inputs() {
        let labels = compute_fidelity(&[], &EmulationReport::default());
        assert!(labels.is_empty());
    }

    #[test]
    fn emulated_overrides_native_in_fidelity() {
        let native = vec![Capability::ExtendedThinking];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "custom".into(),
                },
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native, &report);
        // Emulated should override native since it's processed after
        assert!(matches!(
            labels[&Capability::ExtendedThinking],
            FidelityLabel::Emulated { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §10  Emulation Metadata Tracking
// ═══════════════════════════════════════════════════════════════════════

mod emulation_metadata {
    use super::*;

    #[test]
    fn report_is_empty_when_no_actions() {
        let report = EmulationReport::default();
        assert!(report.is_empty());
    }

    #[test]
    fn report_not_empty_with_applied() {
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "test".into(),
                },
            }],
            warnings: vec![],
        };
        assert!(!report.is_empty());
    }

    #[test]
    fn report_not_empty_with_warnings() {
        let report = EmulationReport {
            applied: vec![],
            warnings: vec!["test warning".into()],
        };
        assert!(!report.is_empty());
        assert!(report.has_unemulatable());
    }

    #[test]
    fn report_has_unemulatable_false_when_no_warnings() {
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "test".into(),
                },
            }],
            warnings: vec![],
        };
        assert!(!report.has_unemulatable());
    }

    #[test]
    fn report_serde_roundtrip_with_all_fields() {
        let report = EmulationReport {
            applied: vec![
                EmulationEntry {
                    capability: Capability::ExtendedThinking,
                    strategy: EmulationStrategy::SystemPromptInjection {
                        prompt: "think".into(),
                    },
                },
                EmulationEntry {
                    capability: Capability::StructuredOutputJsonSchema,
                    strategy: EmulationStrategy::PostProcessing {
                        detail: "validate".into(),
                    },
                },
            ],
            warnings: vec!["CodeExecution disabled".into(), "Streaming disabled".into()],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, decoded);
    }

    #[test]
    fn config_set_and_retrieve() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::Disabled {
                reason: "user pref".into(),
            },
        );
        assert!(
            config
                .strategies
                .contains_key(&Capability::ExtendedThinking)
        );
    }

    #[test]
    fn config_override_replaces_previous() {
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
        match &config.strategies[&Capability::ExtendedThinking] {
            EmulationStrategy::SystemPromptInjection { prompt } => {
                assert_eq!(prompt, "second");
            }
            _ => panic!("expected SystemPromptInjection"),
        }
    }

    #[test]
    fn config_serde_roundtrip() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ImageInput,
            EmulationStrategy::SystemPromptInjection {
                prompt: "images replaced".into(),
            },
        );
        config.set(
            Capability::StopSequences,
            EmulationStrategy::PostProcessing {
                detail: "truncate".into(),
            },
        );
        let json = serde_json::to_string(&config).unwrap();
        let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, decoded);
    }

    #[test]
    fn engine_resolve_uses_config_override() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::Disabled {
                reason: "override".into(),
            },
        );
        let engine = EmulationEngine::new(config);
        let strategy = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(matches!(strategy, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn engine_resolve_falls_back_to_default() {
        let engine = EmulationEngine::with_defaults();
        let strategy = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn strategy_serde_all_variants() {
        let variants = vec![
            EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
            EmulationStrategy::PostProcessing {
                detail: "detail".into(),
            },
            EmulationStrategy::Disabled {
                reason: "reason".into(),
            },
        ];
        for s in &variants {
            let json = serde_json::to_string(s).unwrap();
            let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, decoded);
        }
    }

    #[test]
    fn entry_tracks_capability_and_strategy() {
        let entry = EmulationEntry {
            capability: Capability::ImageInput,
            strategy: emulate_image_input(),
        };
        assert_eq!(entry.capability, Capability::ImageInput);
        assert!(matches!(
            entry.strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §11  Named Strategy Factories
// ═══════════════════════════════════════════════════════════════════════

mod named_strategies {
    use super::*;

    #[test]
    fn emulate_structured_output_variant() {
        assert!(matches!(
            emulate_structured_output(),
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn emulate_code_execution_variant() {
        assert!(matches!(
            emulate_code_execution(),
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn emulate_extended_thinking_variant() {
        assert!(matches!(
            emulate_extended_thinking(),
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn emulate_image_input_variant() {
        assert!(matches!(
            emulate_image_input(),
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn emulate_stop_sequences_variant() {
        assert!(matches!(
            emulate_stop_sequences(),
            EmulationStrategy::PostProcessing { .. }
        ));
    }

    #[test]
    fn emulate_code_execution_mentions_simulate() {
        if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_code_execution() {
            assert!(
                prompt.to_lowercase().contains("reason")
                    || prompt.to_lowercase().contains("simulate")
                    || prompt.to_lowercase().contains("execute")
            );
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn emulate_image_input_mentions_image() {
        if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_image_input() {
            assert!(
                prompt.to_lowercase().contains("image") || prompt.to_lowercase().contains("vision")
            );
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn emulate_stop_sequences_mentions_truncate() {
        if let EmulationStrategy::PostProcessing { detail } = emulate_stop_sequences() {
            assert!(
                detail.to_lowercase().contains("truncat") || detail.to_lowercase().contains("stop")
            );
        } else {
            panic!("wrong variant");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12  Free Function & Integration
// ═══════════════════════════════════════════════════════════════════════

mod free_function_integration {
    use super::*;

    #[test]
    fn apply_emulation_free_function_works() {
        let config = EmulationConfig::new();
        let mut conv = simple_conv();
        let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
        assert_eq!(report.applied.len(), 1);
    }

    #[test]
    fn apply_emulation_with_custom_config() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::CodeExecution,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Simulate it.".into(),
            },
        );
        let mut conv = user_only_conv();
        let report = apply_emulation(&config, &[Capability::CodeExecution], &mut conv);
        assert_eq!(report.applied.len(), 1);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn apply_emulation_multiple_at_once() {
        let config = EmulationConfig::new();
        let mut conv = simple_conv();
        let report = apply_emulation(
            &config,
            &[
                Capability::ExtendedThinking,
                Capability::ImageInput,
                Capability::StopSequences,
                Capability::StructuredOutputJsonSchema,
            ],
            &mut conv,
        );
        assert_eq!(report.applied.len(), 4);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn default_strategy_image_input() {
        let s = default_strategy(&Capability::ImageInput);
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn default_strategy_stop_sequences() {
        let s = default_strategy(&Capability::StopSequences);
        assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn default_strategy_unknown_capability_is_disabled() {
        let s = default_strategy(&Capability::Audio);
        assert!(matches!(s, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn check_missing_matches_apply_report() {
        let engine = EmulationEngine::with_defaults();
        let caps = vec![
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::ImageInput,
        ];
        let check_report = engine.check_missing(&caps);

        let mut conv = simple_conv();
        let apply_report = engine.apply(&caps, &mut conv);

        assert_eq!(check_report.applied.len(), apply_report.applied.len());
        assert_eq!(check_report.warnings.len(), apply_report.warnings.len());
    }
}
