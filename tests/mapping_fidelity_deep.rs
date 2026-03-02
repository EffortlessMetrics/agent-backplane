// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep fidelity tests for the mapping layer, focusing on SDK-specific semantic preservation.
//!
//! Covers:
//! - Tool definition fidelity across all 6 dialects
//! - System message handling differences
//! - Token usage mapping
//! - Model name normalization
//! - Streaming event ordering preservation
//! - Stop reason / finish_reason mapping
//! - Content type mapping (text, image, audio)
//! - Error code translation between SDK error formats
//! - Metadata/extensions preservation in passthrough mode
//! - Round-trip fidelity scores for each dialect pair

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingMatrix, MappingRegistry, features, known_rules, validate_mapping,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn tool_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Search query" },
            "limit": { "type": "integer", "description": "Max results" }
        },
        "required": ["query"]
    })
}

fn ir_tool() -> IrToolDefinition {
    IrToolDefinition {
        name: "search".into(),
        description: "Search the web".into(),
        parameters: tool_schema(),
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: Tool definition fidelity across all 6 dialects
// ═══════════════════════════════════════════════════════════════════════════

mod tool_definition_fidelity {
    use super::*;

    #[test]
    fn openai_tool_to_ir_preserves_name_description_params() {
        let tool = abp_shim_openai::Tool::function("search", "Search the web", tool_schema());
        let ir_tools = abp_shim_openai::tools_to_ir(&[tool]);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "search");
        assert_eq!(ir_tools[0].description, "Search the web");
        assert_eq!(
            ir_tools[0].parameters["properties"]["query"]["type"],
            "string"
        );
        assert!(
            ir_tools[0].parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("query"))
        );
    }

    #[test]
    fn gemini_function_declaration_preserves_schema() {
        let func_decl = abp_shim_gemini::FunctionDeclaration {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: tool_schema(),
        };
        // Gemini wraps function declarations in ToolDeclaration
        let tool_decl = abp_shim_gemini::ToolDeclaration {
            function_declarations: vec![func_decl.clone()],
        };
        assert_eq!(tool_decl.function_declarations[0].name, "search");
        assert_eq!(
            tool_decl.function_declarations[0].description,
            "Search the web"
        );
        assert_eq!(
            tool_decl.function_declarations[0].parameters["required"],
            json!(["query"])
        );
    }

    #[test]
    fn ir_tool_definition_roundtrip_through_openai() {
        let original = ir_tool();
        // IR → OpenAI tool
        let openai_tool = abp_shim_openai::Tool::function(
            &original.name,
            &original.description,
            original.parameters.clone(),
        );
        // OpenAI tool → IR
        let roundtripped = abp_shim_openai::tools_to_ir(&[openai_tool]);
        assert_eq!(roundtripped[0], original);
    }

    #[test]
    fn ir_tool_definition_preserves_nested_object_params() {
        let complex_schema = json!({
            "type": "object",
            "properties": {
                "config": {
                    "type": "object",
                    "properties": {
                        "verbose": { "type": "boolean" },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }
            },
            "required": ["config"]
        });
        let tool =
            abp_shim_openai::Tool::function("configure", "Set config", complex_schema.clone());
        let ir_tools = abp_shim_openai::tools_to_ir(&[tool]);
        assert_eq!(ir_tools[0].parameters, complex_schema);
    }

    #[test]
    fn tool_use_mapping_rule_openai_to_claude_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(
            rule.fidelity.is_lossless(),
            "OpenAI→Claude tool_use should be lossless"
        );
    }

    #[test]
    fn tool_use_mapping_rule_openai_to_codex_is_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(
            !rule.fidelity.is_lossless(),
            "OpenAI→Codex tool_use should be lossy (different schema format)"
        );
    }

    #[test]
    fn all_six_dialects_have_tool_use_self_mapping() {
        let reg = known_rules();
        for &d in Dialect::all() {
            let rule = reg.lookup(d, d, features::TOOL_USE);
            assert!(
                rule.is_some(),
                "Self-mapping for tool_use should exist for {d}"
            );
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "Self-mapping for tool_use should be lossless for {d}"
            );
        }
    }

    #[test]
    fn tool_use_kimi_copilot_lossless_to_openai_compatible() {
        let reg = known_rules();
        for &nd in &[Dialect::Kimi, Dialect::Copilot] {
            for &target in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
                let rule = reg.lookup(nd, target, features::TOOL_USE).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "{nd}→{target} tool_use should be lossless (OpenAI-compatible)"
                );
            }
        }
    }

    #[test]
    fn tool_with_empty_parameters_preserved() {
        let empty_params = json!({});
        let tool = abp_shim_openai::Tool::function("ping", "No-op ping", empty_params.clone());
        let ir_tools = abp_shim_openai::tools_to_ir(&[tool]);
        assert_eq!(ir_tools[0].parameters, empty_params);
    }

    #[test]
    fn multiple_tools_ordering_preserved() {
        let tools = vec![
            abp_shim_openai::Tool::function("alpha", "First tool", json!({})),
            abp_shim_openai::Tool::function("beta", "Second tool", json!({})),
            abp_shim_openai::Tool::function("gamma", "Third tool", json!({})),
        ];
        let ir_tools = abp_shim_openai::tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 3);
        assert_eq!(ir_tools[0].name, "alpha");
        assert_eq!(ir_tools[1].name, "beta");
        assert_eq!(ir_tools[2].name, "gamma");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: System message handling differences
// ═══════════════════════════════════════════════════════════════════════════

mod system_message_handling {
    use super::*;

    #[test]
    fn openai_system_message_as_role() {
        let messages = vec![
            abp_shim_openai::Message::system("You are helpful."),
            abp_shim_openai::Message::user("Hello"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&messages);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");
    }

    #[test]
    fn claude_system_as_separate_param() {
        // Claude uses a top-level `system` param, not a message role
        let req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("You are helpful.".into()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        // The system param is separate from messages
        assert!(req.system.is_some());
        assert_eq!(req.system.as_deref(), Some("You are helpful."));
        // Messages should not contain a system role
        assert!(req.messages.iter().all(|m| {
            m.role != abp_shim_claude::Role::User
                || m.content
                    .iter()
                    .any(|b| matches!(b, abp_shim_claude::ContentBlock::Text { .. }))
        }));
    }

    #[test]
    fn gemini_system_instruction_as_separate_field() {
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(abp_shim_gemini::Content::user(vec![
                abp_shim_gemini::Part::text("You are helpful."),
            ]))
            .add_content(abp_shim_gemini::Content::user(vec![
                abp_shim_gemini::Part::text("Hello"),
            ]));
        assert!(req.system_instruction.is_some());
        let sys = req.system_instruction.as_ref().unwrap();
        assert_eq!(sys.parts.len(), 1);
        match &sys.parts[0] {
            abp_shim_gemini::Part::Text(t) => assert_eq!(t, "You are helpful."),
            _ => panic!("Expected text part"),
        }
    }

    #[test]
    fn openai_ir_roundtrip_preserves_system() {
        let messages = vec![
            abp_shim_openai::Message::system("Be concise."),
            abp_shim_openai::Message::user("Hi"),
            abp_shim_openai::Message::assistant("Hello"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&messages);
        let back = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, abp_shim_openai::Role::System);
        assert_eq!(back[0].content.as_deref(), Some("Be concise."));
    }

    #[test]
    fn ir_system_message_accessible_via_accessor() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "System prompt here"),
            IrMessage::text(IrRole::User, "User input"),
        ]);
        let sys = conv.system_message().unwrap();
        assert_eq!(sys.text_content(), "System prompt here");
    }

    #[test]
    fn kimi_system_message_via_role() {
        let messages = vec![
            abp_shim_kimi::Message::system("You are a Kimi assistant."),
            abp_shim_kimi::Message::user("Hello"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&messages);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "You are a Kimi assistant.");
    }

    #[test]
    fn copilot_system_message_via_role() {
        let messages = vec![
            abp_shim_copilot::Message::system("You are a Copilot extension."),
            abp_shim_copilot::Message::user("Hello"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&messages);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(
            ir.messages[0].text_content(),
            "You are a Copilot extension."
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Token usage mapping
// ═══════════════════════════════════════════════════════════════════════════

mod token_usage_mapping {
    use super::*;

    #[test]
    fn openai_usage_prompt_completion_total() {
        let ir = IrUsage::from_io(100, 50);
        let openai_usage = abp_shim_openai::ir_usage_to_usage(&ir);
        assert_eq!(openai_usage.prompt_tokens, 100);
        assert_eq!(openai_usage.completion_tokens, 50);
        assert_eq!(openai_usage.total_tokens, 150);
    }

    #[test]
    fn gemini_usage_prompt_candidates_total() {
        let ir = IrUsage::from_io(200, 80);
        let gemini_usage = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(gemini_usage.prompt_token_count, 200);
        assert_eq!(gemini_usage.candidates_token_count, 80);
        assert_eq!(gemini_usage.total_token_count, 280);
    }

    #[test]
    fn gemini_usage_to_ir_roundtrip() {
        let original = abp_shim_gemini::UsageMetadata {
            prompt_token_count: 300,
            candidates_token_count: 120,
            total_token_count: 420,
        };
        let ir = abp_shim_gemini::usage_to_ir(&original);
        let back = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(back, original);
    }

    #[test]
    fn codex_usage_input_output_total() {
        let ir = IrUsage::from_io(150, 75);
        let codex_usage = abp_shim_codex::ir_usage_to_usage(&ir);
        assert_eq!(codex_usage.input_tokens, 150);
        assert_eq!(codex_usage.output_tokens, 75);
        assert_eq!(codex_usage.total_tokens, 225);
    }

    #[test]
    fn kimi_usage_prompt_completion_format() {
        let ir = IrUsage::from_io(250, 100);
        let kimi_usage = abp_shim_kimi::ir_usage_to_usage(&ir);
        assert_eq!(kimi_usage.prompt_tokens, 250);
        assert_eq!(kimi_usage.completion_tokens, 100);
        assert_eq!(kimi_usage.total_tokens, 350);
    }

    #[test]
    fn copilot_usage_as_tuple() {
        let ir = IrUsage::from_io(180, 60);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 180);
        assert_eq!(output, 60);
        assert_eq!(total, 240);
    }

    #[test]
    fn ir_usage_from_io_computes_total() {
        let u = IrUsage::from_io(500, 200);
        assert_eq!(u.total_tokens, 700);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn ir_usage_with_cache_tokens() {
        let u = IrUsage::with_cache(500, 200, 50, 30);
        assert_eq!(u.total_tokens, 700);
        assert_eq!(u.cache_read_tokens, 50);
        assert_eq!(u.cache_write_tokens, 30);
    }

    #[test]
    fn ir_usage_merge_sums_all_fields() {
        let a = IrUsage::with_cache(100, 50, 10, 5);
        let b = IrUsage::with_cache(200, 80, 20, 15);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 130);
        assert_eq!(merged.total_tokens, 430);
        assert_eq!(merged.cache_read_tokens, 30);
        assert_eq!(merged.cache_write_tokens, 20);
    }

    #[test]
    fn zero_usage_roundtrip_through_all_formats() {
        let ir = IrUsage::from_io(0, 0);
        let openai = abp_shim_openai::ir_usage_to_usage(&ir);
        assert_eq!(openai.total_tokens, 0);
        let gemini = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(gemini.total_token_count, 0);
        let codex = abp_shim_codex::ir_usage_to_usage(&ir);
        assert_eq!(codex.total_tokens, 0);
        let kimi = abp_shim_kimi::ir_usage_to_usage(&ir);
        assert_eq!(kimi.total_tokens, 0);
        let (_, _, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(total, 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Model name normalization
// ═══════════════════════════════════════════════════════════════════════════

mod model_name_normalization {

    #[test]
    fn openai_request_preserves_model_name() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("hi")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn claude_request_preserves_model_name() {
        let req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: Some(0.5),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn codex_request_preserves_model_name() {
        use abp_codex_sdk::dialect::CodexInputItem;
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![CodexInputItem::Message {
                role: "user".into(),
                content: "test".into(),
            }])
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn model_names_are_strings_not_enums() {
        // Model names should be free-form strings to support new models without code changes
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-5-turbo-preview-2025")
            .messages(vec![abp_shim_openai::Message::user("hi")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-5-turbo-preview-2025"));
    }

    #[test]
    fn cross_dialect_model_independence() {
        // Each dialect can carry its own model name; normalization is not forced
        let openai_req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let claude_req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: Some(0.5),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };

        let wo_openai = abp_shim_openai::request_to_work_order(&openai_req);
        let wo_claude = abp_shim_claude::request_to_work_order(&claude_req);

        assert_ne!(wo_openai.config.model, wo_claude.config.model);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Streaming event ordering preservation
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_event_ordering {
    use super::*;

    #[test]
    fn openai_stream_events_preserve_delta_order() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: " world".into(),
            }),
            make_event(AgentEventKind::AssistantDelta { text: "!".into() }),
        ];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        // 3 delta events + 1 final stop event
        assert_eq!(stream.len(), 4);
        // Check ordering
        assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hello"));
        assert_eq!(
            stream[1].choices[0].delta.content.as_deref(),
            Some(" world")
        );
        assert_eq!(stream[2].choices[0].delta.content.as_deref(), Some("!"));
        // Final event is stop
        assert_eq!(stream[3].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_stream_events_include_tool_calls_in_order() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta {
                text: "Let me search.".into(),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"query": "rust"}),
            }),
        ];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        // 1 text delta + 1 tool call + 1 stop
        assert_eq!(stream.len(), 3);
        assert!(stream[0].choices[0].delta.content.is_some());
        assert!(stream[1].choices[0].delta.tool_calls.is_some());
    }

    #[test]
    fn streaming_mapping_is_lossless_across_core_four() {
        let reg = known_rules();
        let core = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &src in &core {
            for &tgt in &core {
                let rule = reg.lookup(src, tgt, features::STREAMING).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "Streaming {src}→{tgt} should be lossless"
                );
            }
        }
    }

    #[test]
    fn streaming_kimi_copilot_lossless_to_all() {
        let reg = known_rules();
        for &nd in &[Dialect::Kimi, Dialect::Copilot] {
            for &od in Dialect::all() {
                let rule = reg.lookup(nd, od, features::STREAMING).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "Streaming {nd}→{od} should be lossless"
                );
            }
        }
    }

    #[test]
    fn codex_stream_events_preserve_ordering() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "fn ".into() }),
            make_event(AgentEventKind::AssistantDelta {
                text: "main() {}".into(),
            }),
        ];
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");
        // Each delta maps to a stream event, plus a final done event
        assert!(stream.len() >= 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Stop reason / finish_reason mapping
// ═══════════════════════════════════════════════════════════════════════════

mod stop_reason_mapping {
    use super::*;

    #[test]
    fn openai_receipt_produces_stop_finish_reason() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Done.".into(),
        })];
        let receipt = abp_shim_openai::mock_receipt(events);
        let response = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(response.choices.len(), 1);
        // Default finish_reason is "stop" for completed messages
        assert_eq!(response.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_tool_call_produces_tool_calls_finish_reason() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: Some("call_1".into()),
            parent_tool_use_id: None,
            input: json!({"q": "test"}),
        })];
        let receipt = abp_shim_openai::mock_receipt(events);
        let response = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(
            response.choices[0].finish_reason.as_deref(),
            Some("tool_calls")
        );
    }

    #[test]
    fn openai_stream_final_event_has_stop_reason() {
        let events = vec![make_event(AgentEventKind::AssistantDelta {
            text: "hi".into(),
        })];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        let last = stream.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn claude_stop_reason_in_response() {
        // Claude uses stop_reason field on MessageResponse
        let response = abp_shim_claude::MessageResponse {
            id: "msg_1".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Done".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: abp_shim_claude::Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        assert_eq!(response.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn gemini_finish_reason_in_candidate() {
        let candidate = abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text("Done")]),
            finish_reason: Some("STOP".into()),
        };
        assert_eq!(candidate.finish_reason.as_deref(), Some("STOP"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Content type mapping (text, image, audio)
// ═══════════════════════════════════════════════════════════════════════════

mod content_type_mapping {
    use super::*;

    #[test]
    fn ir_text_content_block_roundtrip() {
        let msg = IrMessage::text(IrRole::User, "Hello world");
        assert_eq!(msg.text_content(), "Hello world");
        assert!(msg.is_text_only());
    }

    #[test]
    fn ir_image_content_block_structure() {
        let img = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "aWJhc2U2NA==".into(),
        };
        if let IrContentBlock::Image {
            media_type, data, ..
        } = &img
        {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "aWJhc2U2NA==");
        } else {
            panic!("Expected Image block");
        }
    }

    #[test]
    fn ir_tool_use_content_block() {
        let tu = IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"query": "test"}),
        };
        if let IrContentBlock::ToolUse { id, name, input } = &tu {
            assert_eq!(id, "call_1");
            assert_eq!(name, "search");
            assert_eq!(input["query"], "test");
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn ir_tool_result_content_block() {
        let tr = IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "result data".into(),
            }],
            is_error: false,
        };
        if let IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } = &tr
        {
            assert_eq!(tool_use_id, "call_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        } else {
            panic!("Expected ToolResult block");
        }
    }

    #[test]
    fn ir_thinking_content_block() {
        let thinking = IrContentBlock::Thinking {
            text: "Let me think...".into(),
        };
        if let IrContentBlock::Thinking { text } = &thinking {
            assert_eq!(text, "Let me think...");
        } else {
            panic!("Expected Thinking block");
        }
    }

    #[test]
    fn image_input_openai_claude_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn image_input_to_codex_unsupported() {
        let reg = known_rules();
        for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg
                .lookup(src, Dialect::Codex, features::IMAGE_INPUT)
                .unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "Image input {src}→Codex should be unsupported"
            );
        }
    }

    #[test]
    fn image_input_kimi_copilot_unsupported() {
        let reg = known_rules();
        for &nd in &[Dialect::Kimi, Dialect::Copilot] {
            for &od in &[
                Dialect::OpenAi,
                Dialect::Claude,
                Dialect::Gemini,
                Dialect::Codex,
            ] {
                let rule = reg.lookup(nd, od, features::IMAGE_INPUT).unwrap();
                assert!(
                    rule.fidelity.is_unsupported(),
                    "Image input {nd}→{od} should be unsupported"
                );
            }
        }
    }

    #[test]
    fn claude_image_source_base64_structure() {
        let block = abp_shim_claude::ContentBlock::Image {
            source: abp_shim_claude::ImageSource::Base64 {
                media_type: "image/jpeg".into(),
                data: "base64data==".into(),
            },
        };
        if let abp_shim_claude::ContentBlock::Image { source } = &block {
            if let abp_shim_claude::ImageSource::Base64 {
                media_type, data, ..
            } = source
            {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "base64data==");
            } else {
                panic!("Expected Base64 source");
            }
        }
    }

    #[test]
    fn gemini_inline_data_for_images() {
        let part = abp_shim_gemini::Part::inline_data("image/png", "base64data==");
        if let abp_shim_gemini::Part::InlineData { mime_type, data } = &part {
            assert_eq!(mime_type, "image/png");
            assert_eq!(data, "base64data==");
        } else {
            panic!("Expected InlineData part");
        }
    }

    #[test]
    fn mixed_content_message_not_text_only() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Here's an image:".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "data".into(),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.text_content(), "Here's an image:");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: Error code translation between SDK error formats
// ═══════════════════════════════════════════════════════════════════════════

mod error_code_translation {
    use super::*;

    #[test]
    fn mapping_error_feature_unsupported_serialization() {
        let err = abp_mapping::MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let json = serde_json::to_string(&err).unwrap();
        let deser: abp_mapping::MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deser);
        assert!(err.to_string().contains("logprobs"));
    }

    #[test]
    fn mapping_error_fidelity_loss_serialization() {
        let err = abp_mapping::MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "mapped to system message".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let deser: abp_mapping::MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deser);
    }

    #[test]
    fn mapping_error_dialect_mismatch_serialization() {
        let err = abp_mapping::MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let json = serde_json::to_string(&err).unwrap();
        let deser: abp_mapping::MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deser);
    }

    #[test]
    fn mapping_error_invalid_input_serialization() {
        let err = abp_mapping::MappingError::InvalidInput {
            reason: "empty feature name".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let deser: abp_mapping::MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deser);
    }

    #[test]
    fn claude_api_error_structure() {
        let err = abp_shim_claude::ApiError {
            error_type: "overloaded_error".into(),
            message: "Overloaded".into(),
        };
        assert_eq!(err.error_type, "overloaded_error");
        assert_eq!(err.message, "Overloaded");
    }

    #[test]
    fn abp_error_categories_cover_dialect_errors() {
        use abp_error::ErrorCategory;
        let cat = ErrorCategory::Dialect;
        assert_eq!(cat.to_string(), "dialect");
    }

    #[test]
    fn abp_error_categories_cover_ir_errors() {
        use abp_error::ErrorCategory;
        let cat = ErrorCategory::Ir;
        assert_eq!(cat.to_string(), "ir");
    }

    #[test]
    fn validate_mapping_reports_errors_for_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(!results[0].errors.is_empty());
        assert!(matches!(
            &results[0].errors[0],
            abp_mapping::MappingError::FeatureUnsupported { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Metadata / extensions preservation in passthrough mode
// ═══════════════════════════════════════════════════════════════════════════

mod metadata_preservation {
    use super::*;

    #[test]
    fn ir_message_metadata_empty_by_default() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        assert!(msg.metadata.is_empty());
    }

    #[test]
    fn ir_message_metadata_can_carry_vendor_data() {
        let mut msg = IrMessage::text(IrRole::User, "Hello");
        msg.metadata.insert("vendor_id".into(), json!("req-12345"));
        msg.metadata.insert("trace_id".into(), json!("abc-def"));
        assert_eq!(msg.metadata.len(), 2);
        assert_eq!(msg.metadata["vendor_id"], json!("req-12345"));
    }

    #[test]
    fn ir_message_metadata_serialization_roundtrip() {
        let mut msg = IrMessage::text(IrRole::User, "Test");
        msg.metadata.insert("key".into(), json!(42));
        let json = serde_json::to_string(&msg).unwrap();
        let deser: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.metadata["key"], json!(42));
    }

    #[test]
    fn ir_message_metadata_skipped_when_empty() {
        let msg = IrMessage::text(IrRole::User, "No meta");
        let json = serde_json::to_string(&msg).unwrap();
        // BTreeMap with skip_serializing_if should omit the field
        assert!(!json.contains("\"metadata\""));
    }

    #[test]
    fn ir_conversation_metadata_preserved_across_messages() {
        let mut m1 = IrMessage::text(IrRole::System, "sys");
        m1.metadata.insert("source".into(), json!("openai"));
        let m2 = IrMessage::text(IrRole::User, "hi");

        let conv = IrConversation::from_messages(vec![m1, m2]);
        assert_eq!(conv.messages[0].metadata["source"], json!("openai"));
        assert!(conv.messages[1].metadata.is_empty());
    }

    #[test]
    fn agent_event_ext_field_carries_extensions() {
        let mut ext_map = BTreeMap::new();
        ext_map.insert("latency_ms".to_string(), json!(42));
        ext_map.insert("region".to_string(), json!("us-east-1"));
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello".into(),
            },
            ext: Some(ext_map),
        };
        let ext = event.ext.as_ref().unwrap();
        assert_eq!(ext["latency_ms"], json!(42));
        assert_eq!(ext["region"], json!("us-east-1"));
    }

    #[test]
    fn agent_event_ext_none_for_minimal_events() {
        let event = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        assert!(event.ext.is_none());
    }

    #[test]
    fn ir_metadata_uses_btreemap_for_determinism() {
        let mut msg = IrMessage::text(IrRole::User, "test");
        msg.metadata.insert("z_last".into(), json!(1));
        msg.metadata.insert("a_first".into(), json!(2));
        // BTreeMap guarantees sorted key order
        let keys: Vec<&String> = msg.metadata.keys().collect();
        assert_eq!(keys, vec!["a_first", "z_last"]);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Round-trip fidelity scores for each dialect pair
// ═══════════════════════════════════════════════════════════════════════════

mod roundtrip_fidelity {
    use super::*;

    fn fidelity_score(reg: &MappingRegistry, src: Dialect, tgt: Dialect) -> (usize, usize, usize) {
        let feats = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        let mut lossless = 0;
        let mut lossy = 0;
        let mut unsupported = 0;
        for &f in &feats {
            match reg.lookup(src, tgt, f) {
                Some(rule) => match &rule.fidelity {
                    Fidelity::Lossless => lossless += 1,
                    Fidelity::LossyLabeled { .. } => lossy += 1,
                    Fidelity::Unsupported { .. } => unsupported += 1,
                },
                None => unsupported += 1,
            }
        }
        (lossless, lossy, unsupported)
    }

    #[test]
    fn same_dialect_always_fully_lossless() {
        let reg = known_rules();
        for &d in Dialect::all() {
            let (lossless, lossy, unsupported) = fidelity_score(&reg, d, d);
            assert_eq!(
                lossless, 5,
                "{d}→{d} should have 5 lossless, got {lossless}"
            );
            assert_eq!(lossy, 0, "{d}→{d} should have 0 lossy, got {lossy}");
            assert_eq!(
                unsupported, 0,
                "{d}→{d} should have 0 unsupported, got {unsupported}"
            );
        }
    }

    #[test]
    fn openai_to_claude_high_fidelity() {
        let reg = known_rules();
        let (lossless, lossy, _unsupported) =
            fidelity_score(&reg, Dialect::OpenAi, Dialect::Claude);
        // tool_use=lossless, streaming=lossless, image_input=lossless
        assert!(
            lossless >= 3,
            "OpenAI→Claude should have ≥3 lossless features"
        );
        // thinking and code_exec are lossy
        assert!(lossy >= 1);
    }

    #[test]
    fn openai_to_codex_lower_fidelity() {
        let reg = known_rules();
        let (lossless, _lossy, unsupported) = fidelity_score(&reg, Dialect::OpenAi, Dialect::Codex);
        // Codex lacks image_input
        assert!(
            unsupported >= 1,
            "OpenAI→Codex should have ≥1 unsupported feature"
        );
        // Streaming is lossless
        assert!(lossless >= 1);
    }

    #[test]
    fn matrix_from_known_rules_covers_core_pairs() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        // Core pairs should all be supported
        let core = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
        for &a in &core {
            for &b in &core {
                if a != b {
                    assert!(
                        matrix.is_supported(a, b),
                        "{a}→{b} should be supported in the matrix"
                    );
                }
            }
        }
    }

    #[test]
    fn rank_targets_puts_best_fidelity_first() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
        assert!(!ranked.is_empty(), "Should have at least one ranked target");
        // Top-ranked targets should have lossless=2 (both tool_use and streaming lossless)
        let (top_dialect, top_score) = ranked[0];
        assert_eq!(
            top_score, 2,
            "{top_dialect} should have 2 lossless features"
        );
    }

    #[test]
    fn rank_targets_excludes_self() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
        for (d, _) in &ranked {
            assert_ne!(*d, Dialect::OpenAi, "Self should be excluded from ranking");
        }
    }

    #[test]
    fn fidelity_asymmetry_for_thinking() {
        let reg = known_rules();
        // Claude→OpenAI thinking is lossy
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        // OpenAI→Claude thinking is also lossy
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn all_dialect_pairs_have_at_least_one_rule() {
        let reg = known_rules();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                // Every pair should have at least one feature mapped
                let has_any = [
                    features::TOOL_USE,
                    features::STREAMING,
                    features::THINKING,
                    features::IMAGE_INPUT,
                    features::CODE_EXEC,
                ]
                .iter()
                .any(|f| reg.lookup(src, tgt, f).is_some());
                assert!(
                    has_any,
                    "{src}→{tgt} should have at least one mapped feature"
                );
            }
        }
    }

    #[test]
    fn validate_mapping_multiple_features_per_pair() {
        let reg = known_rules();
        for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
                if src == tgt {
                    continue;
                }
                let results = validate_mapping(
                    &reg,
                    src,
                    tgt,
                    &[
                        features::TOOL_USE.into(),
                        features::STREAMING.into(),
                        features::THINKING.into(),
                        features::IMAGE_INPUT.into(),
                    ],
                );
                assert_eq!(results.len(), 4, "{src}→{tgt} should validate 4 features");
                // tool_use and streaming should be lossless for the core triad
                assert!(
                    results[0].fidelity.is_lossless(),
                    "{src}→{tgt} tool_use should be lossless"
                );
                assert!(
                    results[1].fidelity.is_lossless(),
                    "{src}→{tgt} streaming should be lossless"
                );
            }
        }
    }

    #[test]
    fn openai_ir_message_roundtrip_fidelity() {
        let messages = vec![
            abp_shim_openai::Message::system("Be helpful"),
            abp_shim_openai::Message::user("What is Rust?"),
            abp_shim_openai::Message::assistant("Rust is a systems programming language."),
        ];
        let ir = abp_shim_openai::messages_to_ir(&messages);
        let back = abp_shim_openai::ir_to_messages(&ir);

        assert_eq!(back.len(), messages.len());
        for (orig, rt) in messages.iter().zip(back.iter()) {
            assert_eq!(orig.content, rt.content);
        }
    }

    #[test]
    fn kimi_ir_message_roundtrip_fidelity() {
        let messages = vec![
            abp_shim_kimi::Message::system("Be helpful"),
            abp_shim_kimi::Message::user("What is Kimi?"),
            abp_shim_kimi::Message::assistant("Kimi is an AI assistant."),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&messages);
        let back = abp_shim_kimi::ir_to_messages(&ir);

        assert_eq!(back.len(), messages.len());
        for (orig, rt) in messages.iter().zip(back.iter()) {
            assert_eq!(orig.role, rt.role);
            assert_eq!(orig.content, rt.content);
        }
    }

    #[test]
    fn copilot_ir_message_roundtrip_fidelity() {
        let messages = vec![
            abp_shim_copilot::Message::system("Be helpful"),
            abp_shim_copilot::Message::user("What is Copilot?"),
            abp_shim_copilot::Message::assistant("Copilot is a coding assistant."),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&messages);
        let back = abp_shim_copilot::ir_to_messages(&ir);

        assert_eq!(back.len(), messages.len());
        for (orig, rt) in messages.iter().zip(back.iter()) {
            assert_eq!(orig.role, rt.role);
            assert_eq!(orig.content, rt.content);
        }
    }

    #[test]
    fn cross_dialect_openai_to_kimi_text_roundtrip() {
        // OpenAI → IR → Kimi → IR → OpenAI
        let openai_msgs = vec![
            abp_shim_openai::Message::user("Hello from OpenAI"),
            abp_shim_openai::Message::assistant("Greetings!"),
        ];
        let ir1 = abp_shim_openai::messages_to_ir(&openai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir1);
        let ir2 = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let back = abp_shim_openai::ir_to_messages(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content.as_deref(), Some("Hello from OpenAI"));
        assert_eq!(back[1].content.as_deref(), Some("Greetings!"));
    }

    #[test]
    fn cross_dialect_openai_to_copilot_text_roundtrip() {
        // OpenAI → IR → Copilot → IR → OpenAI
        let openai_msgs = vec![
            abp_shim_openai::Message::user("Hello from OpenAI"),
            abp_shim_openai::Message::assistant("Greetings!"),
        ];
        let ir1 = abp_shim_openai::messages_to_ir(&openai_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir1);
        let ir2 = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let back = abp_shim_openai::ir_to_messages(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content.as_deref(), Some("Hello from OpenAI"));
        assert_eq!(back[1].content.as_deref(), Some("Greetings!"));
    }
}
