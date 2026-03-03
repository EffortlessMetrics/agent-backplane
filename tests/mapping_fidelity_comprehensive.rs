// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive mapping fidelity tests — validates that SDK dialect translations
//! preserve semantics correctly across JSON-level and IR-level mappers.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_error::{ErrorCategory, ErrorCode};
use abp_mapper::{
    ClaudeToOpenAiMapper, DialectRequest, IdentityMapper, IrMapper,
    MapError, Mapper, MappingError, OpenAiClaudeIrMapper, OpenAiGeminiIrMapper,
    ClaudeGeminiIrMapper, IrIdentityMapper,
    default_ir_mapper, supported_ir_pairs, OpenAiToClaudeMapper,
};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Fidelity level semantics (~15 tests)
//    Since FidelityLevel doesn't exist as a type, we test the observable
//    fidelity characteristics of the mapper system: lossless passthrough,
//    lossy conversions (thinking blocks), and degraded/unmappable cases.
// ═══════════════════════════════════════════════════════════════════════════

mod fidelity_level_semantics {
    use super::*;

    #[test]
    fn identity_mapper_is_lossless() {
        let mapper = IdentityMapper;
        let body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.7,
            "max_tokens": 100
        });
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: body.clone(),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result, body, "identity mapper must be perfectly lossless");
    }

    #[test]
    fn ir_identity_mapper_is_lossless() {
        let mapper = IrIdentityMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "Hi there!"),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn same_dialect_ir_mapping_is_lossless() {
        for &d in Dialect::all() {
            let m = default_ir_mapper(d, d);
            assert!(m.is_some(), "same-dialect pair {d} -> {d} must have mapper");
            let mapper = m.unwrap();
            let conv = IrConversation::from_messages(vec![
                IrMessage::text(IrRole::User, "test"),
            ]);
            let result = mapper.map_request(d, d, &conv).unwrap();
            assert_eq!(result, conv, "same-dialect mapping must be lossless for {d}");
        }
    }

    #[test]
    fn thinking_blocks_lost_openai_to_claude_roundtrip() {
        // Claude -> OpenAI drops thinking blocks (lossy)
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Think hard"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "deep thoughts".into(),
                    },
                    IrContentBlock::Text {
                        text: "Answer".into(),
                    },
                ],
            ),
        ]);
        // Claude -> OpenAI should drop thinking blocks
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let asst = mapped.last_assistant().unwrap();
        let has_thinking = asst
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(!has_thinking, "thinking blocks must be dropped for OpenAI");
        // But text content must survive
        assert_eq!(asst.text_content(), "Answer");
    }

    #[test]
    fn text_content_preserved_across_openai_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "What is 2+2?"),
        ]);
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(mapped.messages[0].text_content(), "What is 2+2?");
    }

    #[test]
    fn text_content_preserved_across_openai_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello Gemini"),
        ]);
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(mapped.messages[0].text_content(), "Hello Gemini");
    }

    #[test]
    fn text_content_preserved_across_claude_gemini() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello from Claude"),
        ]);
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(mapped.messages[0].text_content(), "Hello from Claude");
    }

    #[test]
    fn unsupported_pair_returns_none_from_factory() {
        // Kimi and Copilot don't have cross-dialect IR mappers yet
        let m = default_ir_mapper(Dialect::Kimi, Dialect::Copilot);
        assert!(m.is_none(), "unsupported pairs must return None");
    }

    #[test]
    fn map_error_lossy_conversion_is_informative() {
        let err = MapError::LossyConversion {
            field: "extended_thinking".into(),
            reason: "target dialect does not support thinking blocks".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("lossy"));
        assert!(msg.contains("extended_thinking"));
    }

    #[test]
    fn mapping_error_fidelity_loss_captures_dialects() {
        let err = MappingError::FidelityLoss {
            field: "logprobs".into(),
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            detail: "Claude does not support logprobs".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("OpenAI"));
        assert!(msg.contains("Claude"));
        assert!(msg.contains("fidelity loss"));
    }

    #[test]
    fn tool_use_blocks_preserved_across_ir_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "call a tool"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                }],
            ),
        ]);
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tools = mapped.last_assistant().unwrap().tool_use_blocks();
        assert_eq!(tools.len(), 1);
        if let IrContentBlock::ToolUse { name, input, .. } = tools[0] {
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "main.rs");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn system_message_preserved_in_ir_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are a coding assistant"),
            IrMessage::text(IrRole::User, "Hello"),
        ]);
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let sys = mapped.system_message();
        assert!(sys.is_some(), "system message must survive mapping");
        assert_eq!(sys.unwrap().text_content(), "You are a coding assistant");
    }

    #[test]
    fn multi_turn_message_order_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "first"),
            IrMessage::text(IrRole::Assistant, "reply1"),
            IrMessage::text(IrRole::User, "second"),
            IrMessage::text(IrRole::Assistant, "reply2"),
        ]);
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(mapped.messages.len(), 4);
        assert_eq!(mapped.messages[0].role, IrRole::User);
        assert_eq!(mapped.messages[1].role, IrRole::Assistant);
        assert_eq!(mapped.messages[2].role, IrRole::User);
        assert_eq!(mapped.messages[3].role, IrRole::Assistant);
    }

    #[test]
    fn map_error_serde_roundtrip() {
        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "dropped".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn mapping_error_debug_impl() {
        let err = MappingError::UnsupportedCapability {
            capability: "logprobs".into(),
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
        };
        let dbg = format!("{err:?}");
        assert!(dbg.contains("UnsupportedCapability"));
        assert!(dbg.contains("logprobs"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Mapper factory operations (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod mapper_factory_operations {
    use super::*;

    #[test]
    fn factory_returns_mapper_for_openai_to_claude() {
        let m = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(m.is_some());
    }

    #[test]
    fn factory_returns_mapper_for_claude_to_openai() {
        let m = default_ir_mapper(Dialect::Claude, Dialect::OpenAi);
        assert!(m.is_some());
    }

    #[test]
    fn factory_returns_mapper_for_openai_to_gemini() {
        let m = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini);
        assert!(m.is_some());
    }

    #[test]
    fn factory_returns_mapper_for_gemini_to_openai() {
        let m = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi);
        assert!(m.is_some());
    }

    #[test]
    fn factory_returns_mapper_for_claude_to_gemini() {
        let m = default_ir_mapper(Dialect::Claude, Dialect::Gemini);
        assert!(m.is_some());
    }

    #[test]
    fn factory_returns_mapper_for_gemini_to_claude() {
        let m = default_ir_mapper(Dialect::Gemini, Dialect::Claude);
        assert!(m.is_some());
    }

    #[test]
    fn factory_returns_identity_for_same_dialect() {
        for &d in Dialect::all() {
            let m = default_ir_mapper(d, d);
            assert!(m.is_some(), "identity mapper expected for {d} -> {d}");
        }
    }

    #[test]
    fn factory_returns_none_for_kimi_to_copilot() {
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn factory_returns_none_for_codex_to_kimi() {
        assert!(default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
    }

    #[test]
    fn factory_returns_none_for_copilot_to_openai() {
        assert!(default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).is_none());
    }

    #[test]
    fn supported_pairs_includes_all_identity_mappings() {
        let pairs = supported_ir_pairs();
        for &d in Dialect::all() {
            assert!(
                pairs.contains(&(d, d)),
                "supported pairs must include identity ({d}, {d})"
            );
        }
    }

    #[test]
    fn supported_pairs_includes_cross_dialect() {
        let pairs = supported_ir_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
    }

    #[test]
    fn supported_pairs_count_is_correct() {
        let pairs = supported_ir_pairs();
        // 6 identity + 6 cross-dialect = 12
        assert_eq!(pairs.len(), 12);
    }

    #[test]
    fn ir_mapper_supported_pairs_match_factory() {
        let mapper = OpenAiClaudeIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn ir_mapper_rejects_unsupported_pair() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "test"),
        ]);
        let err = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Input mapping validation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod input_mapping_validation {
    use super::*;

    #[test]
    fn openai_simple_user_message_to_claude() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}],
                "max_tokens": 512
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
        assert_eq!(result["max_tokens"], 512);
    }

    #[test]
    fn openai_system_message_extracted_to_claude_top_level() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "You are a coding assistant"},
                    {"role": "user", "content": "Hi"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["system"], "You are a coding assistant");
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn openai_tool_calls_mapped_to_claude_tool_use() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "What's the weather?"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_abc",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"city\":\"NYC\"}"
                            }
                        }]
                    }
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        let asst_content = msgs[1]["content"].as_array().unwrap();
        assert_eq!(asst_content[0]["type"], "tool_use");
        assert_eq!(asst_content[0]["name"], "get_weather");
        assert_eq!(asst_content[0]["input"]["city"], "NYC");
    }

    #[test]
    fn openai_function_definitions_mapped_to_claude_tools() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "search",
                        "description": "Search the web",
                        "parameters": {
                            "type": "object",
                            "properties": {"q": {"type": "string"}},
                            "required": ["q"]
                        }
                    }
                }]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "search");
        assert_eq!(tools[0]["description"], "Search the web");
        assert!(tools[0]["input_schema"]["properties"]["q"].is_object());
    }

    #[test]
    fn claude_simple_user_message_to_openai() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello"}]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn claude_system_string_to_openai_system_message() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "system": "Be helpful",
                "messages": [{"role": "user", "content": "Go"}]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be helpful");
    }

    #[test]
    fn claude_tool_use_mapped_to_openai_tool_calls() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "messages": [
                    {"role": "user", "content": "Search for cats"},
                    {
                        "role": "assistant",
                        "content": [{
                            "type": "tool_use",
                            "id": "tu_1",
                            "name": "search",
                            "input": {"q": "cats"}
                        }]
                    }
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        let tc = &msgs[1]["tool_calls"].as_array().unwrap()[0];
        assert_eq!(tc["id"], "tu_1");
        assert_eq!(tc["function"]["name"], "search");
    }

    #[test]
    fn openai_tool_result_mapped_to_claude_format() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]
                    },
                    {"role": "tool", "tool_call_id": "c1", "content": "result data"}
                ]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        // Tool result becomes user message with tool_result block
        assert_eq!(msgs[2]["role"], "user");
        let content = msgs[2]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "c1");
    }

    #[test]
    fn openai_mapper_rejects_wrong_dialect() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({"model": "x"}),
        };
        assert!(matches!(
            mapper.map_request(&req).unwrap_err(),
            MappingError::UnmappableRequest { .. }
        ));
    }

    #[test]
    fn claude_mapper_rejects_wrong_dialect() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "x"}),
        };
        assert!(matches!(
            mapper.map_request(&req).unwrap_err(),
            MappingError::UnmappableRequest { .. }
        ));
    }

    #[test]
    fn openai_mapper_rejects_non_object_body() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!("string body"),
        };
        assert!(matches!(
            mapper.map_request(&req).unwrap_err(),
            MappingError::UnmappableRequest { .. }
        ));
    }

    #[test]
    fn claude_mapper_rejects_non_object_body() {
        let mapper = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!(42),
        };
        assert!(matches!(
            mapper.map_request(&req).unwrap_err(),
            MappingError::UnmappableRequest { .. }
        ));
    }

    #[test]
    fn openai_default_max_tokens_injected() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["max_tokens"], 4096);
    }

    #[test]
    fn openai_stop_string_becomes_stop_sequences_array() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": "END"
            }),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result["stop_sequences"], json!(["END"]));
    }

    #[test]
    fn model_field_passes_through() {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let req_o = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]}),
        };
        assert_eq!(o2c.map_request(&req_o).unwrap()["model"], "gpt-4o");

        let req_c = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({"model": "claude-3", "max_tokens": 100, "messages": [{"role": "user", "content": "hi"}]}),
        };
        assert_eq!(c2o.map_request(&req_c).unwrap()["model"], "claude-3");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Output mapping validation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod output_mapping_validation {
    use super::*;

    #[test]
    fn openai_to_claude_response_tagged_as_claude() {
        let mapper = OpenAiToClaudeMapper;
        let body = json!({"content": [{"type": "text", "text": "done"}]});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.dialect, Dialect::Claude);
        assert_eq!(resp.body, body);
    }

    #[test]
    fn claude_to_openai_response_tagged_as_openai() {
        let mapper = ClaudeToOpenAiMapper;
        let body = json!({"choices": [{"message": {"content": "hi"}}]});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.dialect, Dialect::OpenAi);
        assert_eq!(resp.body, body);
    }

    #[test]
    fn identity_response_preserves_body() {
        let mapper = IdentityMapper;
        let body = json!({"nested": {"data": [1,2,3]}});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.body, body);
    }

    #[test]
    fn streaming_event_assistant_delta_to_claude() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "token".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "content_block_delta");
        assert_eq!(result["delta"]["type"], "text_delta");
        assert_eq!(result["delta"]["text"], "token");
    }

    #[test]
    fn streaming_event_assistant_delta_to_openai() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "tok".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion.chunk");
        assert_eq!(result["choices"][0]["delta"]["content"], "tok");
    }

    #[test]
    fn complete_message_event_to_claude() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Full response".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "message");
        assert_eq!(result["role"], "assistant");
        assert_eq!(result["content"][0]["text"], "Full response");
    }

    #[test]
    fn complete_message_event_to_openai() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["choices"][0]["message"]["content"], "Done");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn tool_call_event_to_claude_format() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_42".into()),
                parent_tool_use_id: None,
                input: json!({"command": "ls -la"}),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "content_block_start");
        assert_eq!(result["content_block"]["type"], "tool_use");
        assert_eq!(result["content_block"]["name"], "bash");
        assert_eq!(result["content_block"]["id"], "tu_42");
    }

    #[test]
    fn tool_call_event_to_openai_format() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion.chunk");
        let tc = &result["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "read_file");
        assert_eq!(tc["id"], "call_1");
    }

    #[test]
    fn tool_result_event_to_claude_format() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_42".into()),
                output: json!("file1\nfile2"),
                is_error: false,
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "tu_42");
        assert_eq!(result["is_error"], false);
    }

    #[test]
    fn tool_result_event_to_openai_format() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                output: json!("contents here"),
                is_error: false,
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["role"], "tool");
        assert_eq!(result["content"], "contents here");
        assert_eq!(result["tool_call_id"], "call_1");
    }

    #[test]
    fn error_event_falls_back_to_generic_serialization() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: Some(ErrorCode::BackendCrashed),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "error");
        assert_eq!(result["message"], "something broke");
    }

    #[test]
    fn warning_event_fallback_serialization() {
        let mapper = ClaudeToOpenAiMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "rate limited".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "warning");
    }

    #[test]
    fn tool_result_error_flag_preserved() {
        let mapper = OpenAiToClaudeMapper;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_99".into()),
                output: json!("command not found"),
                is_error: true,
            },
            ext: None,
        };
        let result = mapper.map_event(&event).unwrap();
        assert_eq!(result["is_error"], true);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Fidelity loss detection (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod fidelity_loss_detection {
    use super::*;

    #[test]
    fn thinking_blocks_dropped_claude_to_openai_ir() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Think about this"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "I'm thinking deeply".into(),
                    },
                    IrContentBlock::Text {
                        text: "Here's my answer".into(),
                    },
                ],
            ),
        ]);
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let asst = mapped.last_assistant().unwrap();
        assert!(
            !asst
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. })),
            "thinking blocks must be dropped in Claude->OpenAI"
        );
        assert_eq!(asst.text_content(), "Here's my answer");
    }

    #[test]
    fn thinking_blocks_dropped_claude_to_gemini_ir() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Reason carefully"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "reasoning...".into(),
                    },
                    IrContentBlock::Text {
                        text: "conclusion".into(),
                    },
                ],
            ),
        ]);
        let mapped = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let asst = mapped.last_assistant().unwrap();
        assert!(
            !asst
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. })),
            "thinking blocks must be dropped in Claude->Gemini"
        );
    }

    #[test]
    fn map_error_lossy_conversion_contains_field_info() {
        let err = MapError::LossyConversion {
            field: "system_instruction".into(),
            reason: "no top-level system in target".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("system_instruction"));
        assert!(msg.contains("lossy"));
    }

    #[test]
    fn map_error_unmappable_tool_is_descriptive() {
        let err = MapError::UnmappableTool {
            name: "computer_use".into(),
            reason: "target dialect has no equivalent".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("computer_use"));
        assert!(msg.contains("unmappable tool"));
    }

    #[test]
    fn mapping_error_unsupported_capability_is_directional() {
        let err = MappingError::UnsupportedCapability {
            capability: "extended_thinking".into(),
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
        };
        let msg = err.to_string();
        assert!(msg.contains("Claude"));
        assert!(msg.contains("OpenAI"));
        assert!(msg.contains("extended_thinking"));
    }

    #[test]
    fn mapping_error_incompatible_types_on_unknown_role() {
        let mapper = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "developer", "content": "hi"}]
            }),
        };
        let err = mapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::IncompatibleTypes { .. }));
    }

    #[test]
    fn roundtrip_openai_claude_preserves_user_text() {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;
        let original = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello world"}],
            "max_tokens": 100
        });
        let claude = o2c
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: original.clone(),
            })
            .unwrap();
        let back = c2o
            .map_request(&DialectRequest {
                dialect: Dialect::Claude,
                body: claude,
            })
            .unwrap();
        let msgs = back["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"], "Hello world");
    }

    #[test]
    fn roundtrip_system_message_survives() {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;
        let original = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be concise"},
                {"role": "user", "content": "Hi"}
            ],
            "max_tokens": 50
        });
        let claude = o2c
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: original,
            })
            .unwrap();
        assert_eq!(claude["system"], "Be concise");
        let back = c2o
            .map_request(&DialectRequest {
                dialect: Dialect::Claude,
                body: claude,
            })
            .unwrap();
        let msgs = back["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be concise");
    }

    #[test]
    fn roundtrip_tool_definitions_preserve_schema() {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;
        let original = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "calc",
                    "description": "Calculator",
                    "parameters": {
                        "type": "object",
                        "properties": {"expr": {"type": "string"}}
                    }
                }
            }]
        });
        let claude = o2c
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: original,
            })
            .unwrap();
        assert_eq!(claude["tools"][0]["name"], "calc");
        let back = c2o
            .map_request(&DialectRequest {
                dialect: Dialect::Claude,
                body: claude,
            })
            .unwrap();
        assert_eq!(back["tools"][0]["function"]["name"], "calc");
        assert!(back["tools"][0]["function"]["parameters"]["properties"]["expr"].is_object());
    }

    #[test]
    fn metadata_preserved_in_ir_roundtrip() {
        let mapper = IrIdentityMapper;
        let mut msg = IrMessage::text(IrRole::User, "test");
        msg.metadata
            .insert("custom_key".into(), json!("custom_value"));
        let conv = IrConversation::from_messages(vec![msg]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(
            result.messages[0].metadata.get("custom_key").unwrap(),
            &json!("custom_value")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error taxonomy for mapping failures (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod error_taxonomy {
    use super::*;

    #[test]
    fn mapping_error_variants_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MappingError>();
    }

    #[test]
    fn map_error_variants_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MapError>();
    }

    #[test]
    fn mapping_error_is_clone() {
        let err = MappingError::UnsupportedCapability {
            capability: "vision".into(),
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Gemini,
        };
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }

    #[test]
    fn map_error_is_clone() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        };
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn error_code_mapping_category() {
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingLossyConversion.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingUnmappableTool.category(),
            ErrorCategory::Mapping
        );
    }

    #[test]
    fn error_code_mapping_as_str() {
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.as_str(),
            "mapping_unsupported_capability"
        );
        assert_eq!(
            ErrorCode::MappingLossyConversion.as_str(),
            "mapping_lossy_conversion"
        );
        assert_eq!(
            ErrorCode::MappingUnmappableTool.as_str(),
            "mapping_unmappable_tool"
        );
        assert_eq!(
            ErrorCode::MappingDialectMismatch.as_str(),
            "mapping_dialect_mismatch"
        );
    }

    #[test]
    fn error_code_mapping_messages_are_descriptive() {
        let msg = ErrorCode::MappingLossyConversion.message();
        assert!(msg.contains("lost"), "message should mention information loss: {msg}");

        let msg2 = ErrorCode::MappingUnmappableTool.message();
        assert!(msg2.contains("tool"), "message should mention tool: {msg2}");
    }

    #[test]
    fn mapping_errors_are_not_retryable() {
        assert!(!ErrorCode::MappingUnsupportedCapability.is_retryable());
        assert!(!ErrorCode::MappingDialectMismatch.is_retryable());
        assert!(!ErrorCode::MappingLossyConversion.is_retryable());
        assert!(!ErrorCode::MappingUnmappableTool.is_retryable());
    }

    #[test]
    fn map_error_serde_roundtrip_all_variants() {
        let errors = vec![
            MapError::UnsupportedPair {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MapError::LossyConversion {
                field: "f".into(),
                reason: "r".into(),
            },
            MapError::UnmappableTool {
                name: "n".into(),
                reason: "r".into(),
            },
            MapError::IncompatibleCapability {
                capability: "c".into(),
                reason: "r".into(),
            },
        ];
        for err in &errors {
            let json = serde_json::to_string(err).unwrap();
            let back: MapError = serde_json::from_str(&json).unwrap();
            assert_eq!(err, &back);
        }
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let code = ErrorCode::MappingLossyConversion;
        let json = serde_json::to_string(&code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, back);
    }

    #[test]
    fn dialect_error_codes_exist() {
        assert_eq!(
            ErrorCode::DialectUnknown.category(),
            ErrorCategory::Dialect
        );
        assert_eq!(
            ErrorCode::DialectMappingFailed.category(),
            ErrorCategory::Dialect
        );
    }
}
