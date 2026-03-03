// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style scenario tests for the SDK shim layer.
//!
//! Each test follows Given/When/Then naming conventions and exercises
//! the lowering (SDK → IR) and lifting (IR → SDK) paths for OpenAI,
//! Claude, and Gemini dialects, as well as cross-SDK translation
//! and error handling scenarios.

#[cfg(test)]
mod tests {
    use abp_core::error::{MappingError, MappingErrorKind};
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
    use abp_core::{AgentEventKind, Capability, SupportLevel, WorkOrderBuilder};
    use serde_json::json;

    // ════════════════════════════════════════════════════════════════════
    // 1. Scenario: OpenAI request lifecycle (12+ tests)
    // ════════════════════════════════════════════════════════════════════

    #[test]
    /// Scenario: Given an OpenAI request with system/user/assistant messages,
    /// When lowered to IR, Then the IR contains all three roles correctly.
    fn given_openai_request_when_lowered_then_ir_has_messages() {
        use abp_openai_sdk::dialect::OpenAIMessage;
        use abp_openai_sdk::lowering::to_ir;

        let msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("You are a helpful assistant.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hi there!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let conv = to_ir(&msgs);

        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(
            conv.messages[0].text_content(),
            "You are a helpful assistant."
        );
        assert_eq!(conv.messages[1].text_content(), "Hello!");
        assert_eq!(conv.messages[2].text_content(), "Hi there!");
    }

    #[test]
    /// Scenario: Given an OpenAI request with tool definitions,
    /// When lowered to IR and lifted back, Then tool definitions are preserved.
    fn given_openai_request_with_tools_when_lowered_then_tool_defs_preserved() {
        use abp_openai_sdk::dialect::{
            tool_def_from_openai, tool_def_to_openai, CanonicalToolDef,
        };

        let canonical = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file from disk.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        };

        let openai_def = tool_def_to_openai(&canonical);
        assert_eq!(openai_def.tool_type, "function");
        assert_eq!(openai_def.function.name, "read_file");

        let roundtripped = tool_def_from_openai(&openai_def);
        assert_eq!(roundtripped, canonical);
    }

    #[test]
    /// Scenario: Given an OpenAI streaming response with text chunks,
    /// When chunks are mapped, Then assistant deltas arrive in order.
    fn given_openai_streaming_response_when_events_emitted_then_arrive_in_order() {
        use abp_openai_sdk::streaming::{
            map_chunk, ChatCompletionChunk, ChunkChoice, ChunkDelta,
        };

        let chunks: Vec<ChatCompletionChunk> = (0..3)
            .map(|i| ChatCompletionChunk {
                id: format!("chatcmpl-{i}"),
                object: "chat.completion.chunk".into(),
                created: 1700000000 + i,
                model: "gpt-4o".into(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: ChunkDelta {
                        role: if i == 0 { Some("assistant".into()) } else { None },
                        content: Some(format!("chunk{i}")),
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            })
            .collect();

        let mut all_texts = Vec::new();
        for chunk in &chunks {
            let events = map_chunk(chunk);
            for event in events {
                if let AgentEventKind::AssistantDelta { text } = &event.kind {
                    all_texts.push(text.clone());
                }
            }
        }

        assert_eq!(all_texts, vec!["chunk0", "chunk1", "chunk2"]);
    }

    #[test]
    /// Scenario: Given a function call response from OpenAI,
    /// When lifted back from IR, Then it matches the original OpenAI format.
    fn given_openai_function_call_when_lifted_back_then_matches_openai_format() {
        use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
        use abp_openai_sdk::lowering::{from_ir, to_ir};

        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_abc".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];

        let conv = to_ir(&msgs);
        let back = from_ir(&conv);

        assert_eq!(back[0].role, "assistant");
        assert!(back[0].content.is_none());
        let tc = &back[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_abc");
        assert_eq!(tc.call_type, "function");
        assert_eq!(tc.function.name, "read_file");
    }

    #[test]
    /// Scenario: Given an OpenAI tool result message,
    /// When lowered to IR, Then tool_call_id is preserved as tool_use_id.
    fn given_openai_tool_result_when_lowered_then_tool_call_id_preserved() {
        use abp_openai_sdk::dialect::OpenAIMessage;
        use abp_openai_sdk::lowering::to_ir;

        let msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("file contents here".into()),
            tool_calls: None,
            tool_call_id: Some("call_123".into()),
        }];

        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "call_123");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given an OpenAI response with multiple tool calls,
    /// When mapped to events, Then each tool call becomes a separate event.
    fn given_openai_response_with_multiple_tool_calls_when_mapped_then_separate_events() {
        use abp_openai_sdk::dialect::{
            map_response, OpenAIChoice, OpenAIFunctionCall, OpenAIMessage, OpenAIResponse,
            OpenAIToolCall,
        };

        let resp = OpenAIResponse {
            id: "chatcmpl-multi".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![
                        OpenAIToolCall {
                            id: "call_1".into(),
                            call_type: "function".into(),
                            function: OpenAIFunctionCall {
                                name: "read_file".into(),
                                arguments: r#"{"path":"a.rs"}"#.into(),
                            },
                        },
                        OpenAIToolCall {
                            id: "call_2".into(),
                            call_type: "function".into(),
                            function: OpenAIFunctionCall {
                                name: "write_file".into(),
                                arguments: r#"{"path":"b.rs","content":"hello"}"#.into(),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };

        let events = map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file"
        ));
        assert!(matches!(
            &events[1].kind,
            AgentEventKind::ToolCall { tool_name, .. } if tool_name == "write_file"
        ));
    }

    #[test]
    /// Scenario: Given an OpenAI work order mapping,
    /// When the work order has a model override, Then the request uses that model.
    fn given_openai_work_order_when_model_override_then_request_uses_override() {
        use abp_openai_sdk::dialect::{map_work_order, OpenAIConfig};

        let wo = WorkOrderBuilder::new("Fix bug").model("gpt-4-turbo").build();
        let cfg = OpenAIConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert_eq!(req.model, "gpt-4-turbo");
    }

    #[test]
    /// Scenario: Given an OpenAI model name,
    /// When canonicalized, Then it has the openai/ prefix.
    fn given_openai_model_when_canonicalized_then_has_prefix() {
        use abp_openai_sdk::dialect::{from_canonical_model, to_canonical_model};

        let canonical = to_canonical_model("gpt-4o");
        assert_eq!(canonical, "openai/gpt-4o");

        let vendor = from_canonical_model("openai/gpt-4o");
        assert_eq!(vendor, "gpt-4o");
    }

    #[test]
    /// Scenario: Given a known OpenAI model name,
    /// When checked, Then it is recognized.
    fn given_openai_known_model_when_checked_then_recognized() {
        use abp_openai_sdk::dialect::is_known_model;

        assert!(is_known_model("gpt-4o"));
        assert!(is_known_model("gpt-4o-mini"));
        assert!(!is_known_model("nonexistent-model"));
    }

    #[test]
    /// Scenario: Given OpenAI streaming tool call fragments,
    /// When accumulated, Then they produce complete tool call events.
    fn given_openai_streaming_tool_call_when_accumulated_then_complete_events() {
        use abp_openai_sdk::streaming::{ChunkFunctionCall, ChunkToolCall, ToolCallAccumulator};

        let mut acc = ToolCallAccumulator::new();

        // First fragment: id + name
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("call_stream".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some("{\"q\":".into()),
            }),
        }]);

        // Second fragment: more arguments
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(ChunkFunctionCall {
                name: None,
                arguments: Some("\"rust\"}".into()),
            }),
        }]);

        let events = acc.finish();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(tool_use_id.as_deref(), Some("call_stream"));
                assert_eq!(input, &json!({"q": "rust"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given an OpenAI capability manifest,
    /// When inspected, Then streaming is native and MCP is unsupported.
    fn given_openai_capability_manifest_when_inspected_then_correct_levels() {
        use abp_openai_sdk::dialect::capability_manifest;

        let caps = capability_manifest();
        assert!(matches!(caps[&Capability::Streaming], SupportLevel::Native));
        assert!(matches!(
            caps[&Capability::StructuredOutputJsonSchema],
            SupportLevel::Native
        ));
        assert!(matches!(caps[&Capability::McpClient], SupportLevel::Unsupported));
        assert!(matches!(caps[&Capability::McpServer], SupportLevel::Unsupported));
        assert!(matches!(caps[&Capability::ToolRead], SupportLevel::Emulated));
    }

    #[test]
    /// Scenario: Given OpenAI extended request fields with logprobs,
    /// When validated for mapped mode, Then validation fails.
    fn given_openai_extended_fields_with_logprobs_when_validated_then_fails() {
        use abp_openai_sdk::validation::{validate_for_mapped_mode, ExtendedRequestFields};

        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            top_logprobs: None,
            logit_bias: None,
            seed: None,
        };

        let result = validate_for_mapped_mode(&fields);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.param == "logprobs"));
    }

    // ════════════════════════════════════════════════════════════════════
    // 2. Scenario: Claude request lifecycle (12+ tests)
    // ════════════════════════════════════════════════════════════════════

    #[test]
    /// Scenario: Given a Claude messages request with a system prompt,
    /// When lowered to IR, Then system prompt is a separate IR system message.
    fn given_claude_request_when_lowered_then_system_prompt_separated() {
        use abp_claude_sdk::dialect::ClaudeMessage;
        use abp_claude_sdk::lowering::to_ir;

        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello!".into(),
        }];

        let conv = to_ir(&msgs, Some("You are a code reviewer."));

        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(
            conv.messages[0].text_content(),
            "You are a code reviewer."
        );
        assert_eq!(conv.messages[1].role, IrRole::User);
    }

    #[test]
    /// Scenario: Given a Claude request with thinking blocks,
    /// When lowered to IR, Then thinking blocks appear as IrContentBlock::Thinking.
    fn given_claude_request_with_thinking_when_lowered_then_thinking_in_ir() {
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
        use abp_claude_sdk::lowering::to_ir;

        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "Let me analyze the code...".into(),
                signature: Some("sig_abc".into()),
            },
            ClaudeContentBlock::Text {
                text: "Here is my answer.".into(),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let conv = to_ir(&msgs, None);
        assert_eq!(conv.messages[0].content.len(), 2);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert_eq!(text, "Let me analyze the code...");
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
        match &conv.messages[0].content[1] {
            IrContentBlock::Text { text } => assert_eq!(text, "Here is my answer."),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude tool_use response,
    /// When events are mapped, Then tool calls arrive correctly.
    fn given_claude_tool_use_response_when_events_mapped_then_tool_calls_correct() {
        use abp_claude_sdk::dialect::{map_response, ClaudeContentBlock, ClaudeResponse};

        let resp = ClaudeResponse {
            id: "msg_tool".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
            stop_reason: Some("tool_use".into()),
            usage: None,
        };

        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
                assert_eq!(input, &json!({"path": "src/main.rs"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude system prompt in IR,
    /// When extracted, Then it returns the system text.
    fn given_claude_ir_with_system_when_extracted_then_returns_system_text() {
        use abp_claude_sdk::lowering::extract_system_prompt;

        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise and precise."),
            IrMessage::text(IrRole::User, "Hello"),
        ]);

        let sys = extract_system_prompt(&conv);
        assert_eq!(sys.as_deref(), Some("Be concise and precise."));
    }

    #[test]
    /// Scenario: Given Claude messages with system in IR,
    /// When converted back with from_ir, Then system messages are skipped.
    fn given_claude_ir_with_system_when_from_ir_then_system_skipped() {
        use abp_claude_sdk::lowering::from_ir;

        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
            IrMessage::text(IrRole::Assistant, "hi"),
        ]);

        let claude_msgs = from_ir(&conv);
        assert_eq!(claude_msgs.len(), 2);
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[1].role, "assistant");
    }

    #[test]
    /// Scenario: Given a Claude tool_result with is_error=true,
    /// When roundtripped through IR, Then the error flag is preserved.
    fn given_claude_tool_result_error_when_roundtripped_then_error_preserved() {
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
        use abp_claude_sdk::lowering::{from_ir, to_ir};

        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_err".into(),
            content: Some("permission denied".into()),
            is_error: Some(true),
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }

        let back = from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude model name,
    /// When canonicalized, Then it has the anthropic/ prefix.
    fn given_claude_model_when_canonicalized_then_has_prefix() {
        use abp_claude_sdk::dialect::{from_canonical_model, to_canonical_model};

        let canonical = to_canonical_model("claude-sonnet-4-20250514");
        assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");

        let vendor = from_canonical_model("anthropic/claude-sonnet-4-20250514");
        assert_eq!(vendor, "claude-sonnet-4-20250514");
    }

    #[test]
    /// Scenario: Given a Claude capability manifest,
    /// When inspected, Then tool capabilities are native (not emulated).
    fn given_claude_capability_manifest_when_inspected_then_tools_native() {
        use abp_claude_sdk::dialect::capability_manifest;

        let caps = capability_manifest();
        assert!(matches!(caps[&Capability::ToolRead], SupportLevel::Native));
        assert!(matches!(caps[&Capability::ToolWrite], SupportLevel::Native));
        assert!(matches!(caps[&Capability::ToolBash], SupportLevel::Native));
        assert!(matches!(caps[&Capability::McpClient], SupportLevel::Native));
        assert!(matches!(caps[&Capability::McpServer], SupportLevel::Unsupported));
    }

    #[test]
    /// Scenario: Given a Claude work order mapping with system prompt config,
    /// When mapped, Then the request has the system prompt set.
    fn given_claude_work_order_with_system_prompt_when_mapped_then_system_set() {
        use abp_claude_sdk::dialect::{map_work_order, ClaudeConfig};

        let wo = WorkOrderBuilder::new("Explain async").build();
        let cfg = ClaudeConfig {
            system_prompt: Some("You are a Rust expert.".into()),
            ..Default::default()
        };

        let req = map_work_order(&wo, &cfg);
        assert_eq!(req.system.as_deref(), Some("You are a Rust expert."));
    }

    #[test]
    /// Scenario: Given a Claude stream event with text delta,
    /// When mapped, Then it produces an AssistantDelta event.
    fn given_claude_stream_text_delta_when_mapped_then_assistant_delta() {
        use abp_claude_sdk::dialect::{map_stream_event, ClaudeStreamDelta, ClaudeStreamEvent};

        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "Hello ".into(),
            },
        };

        let events = map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello "),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude stream tool_use block start,
    /// When mapped, Then it produces a ToolCall event.
    fn given_claude_stream_tool_use_start_when_mapped_then_tool_call() {
        use abp_claude_sdk::dialect::{map_stream_event, ClaudeContentBlock, ClaudeStreamEvent};

        let event = ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::ToolUse {
                id: "tu_stream".into(),
                name: "grep".into(),
                input: json!({"pattern": "fn main"}),
            },
        };

        let events = map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "grep");
                assert_eq!(tool_use_id.as_deref(), Some("tu_stream"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude image block with base64 data,
    /// When lowered to IR, Then it becomes IrContentBlock::Image.
    fn given_claude_image_block_when_lowered_then_ir_image() {
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
        use abp_claude_sdk::lowering::to_ir;

        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "iVBORw0KGgo=");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    // ════════════════════════════════════════════════════════════════════
    // 3. Scenario: Gemini request lifecycle (10+ tests)
    // ════════════════════════════════════════════════════════════════════

    #[test]
    /// Scenario: Given a Gemini generateContent request with text parts,
    /// When lowered to IR, Then parts map to IR text content blocks.
    fn given_gemini_request_when_lowered_then_parts_map_to_ir() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir;

        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Explain Rust traits.".into())],
        }];

        let conv = to_ir(&contents, None);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Explain Rust traits.");
    }

    #[test]
    /// Scenario: Given Gemini function declarations,
    /// When lowered to canonical, Then they match IR tool format.
    fn given_gemini_function_declarations_when_lowered_then_match_ir_format() {
        use abp_gemini_sdk::dialect::{
            tool_def_from_gemini, tool_def_to_gemini, CanonicalToolDef,
        };

        let canonical = CanonicalToolDef {
            name: "search".into(),
            description: "Search for files.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        };

        let gemini_def = tool_def_to_gemini(&canonical);
        assert_eq!(gemini_def.name, "search");
        assert_eq!(gemini_def.description, "Search for files.");

        let roundtripped = tool_def_from_gemini(&gemini_def);
        assert_eq!(roundtripped, canonical);
    }

    #[test]
    /// Scenario: Given Gemini system_instruction,
    /// When lowered, Then it becomes an IrRole::System message.
    fn given_gemini_system_instruction_when_lowered_then_ir_system() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir;

        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be concise.".into())],
        };
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }];

        let conv = to_ir(&contents, Some(&sys));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
    }

    #[test]
    /// Scenario: Given a Gemini FunctionCall part,
    /// When lowered to IR, Then it becomes ToolUse with synthesized ID.
    fn given_gemini_function_call_when_lowered_then_tool_use_with_synthesized_id() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir;

        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"query": "rust async"}),
            }],
        }];

        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "gemini_search");
                assert_eq!(name, "search");
                assert_eq!(input, &json!({"query": "rust async"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Gemini FunctionResponse,
    /// When lowered to IR, Then it becomes ToolResult with synthesized ID.
    fn given_gemini_function_response_when_lowered_then_tool_result() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir;

        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("results here"),
            }],
        }];

        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "gemini_search");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Gemini model name,
    /// When canonicalized, Then it has the google/ prefix.
    fn given_gemini_model_when_canonicalized_then_has_prefix() {
        use abp_gemini_sdk::dialect::{from_canonical_model, to_canonical_model};

        let canonical = to_canonical_model("gemini-2.5-flash");
        assert_eq!(canonical, "google/gemini-2.5-flash");

        let vendor = from_canonical_model("google/gemini-2.5-flash");
        assert_eq!(vendor, "gemini-2.5-flash");
    }

    #[test]
    /// Scenario: Given a Gemini InlineData part (image),
    /// When lowered to IR, Then it becomes IrContentBlock::Image.
    fn given_gemini_inline_data_when_lowered_then_ir_image() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir;

        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/jpeg".into(),
                data: "base64data".into(),
            })],
        }];

        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "base64data");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Gemini response with text candidate,
    /// When mapped to events, Then it produces an AssistantMessage.
    fn given_gemini_response_when_mapped_then_assistant_message() {
        use abp_gemini_sdk::dialect::{
            map_response, GeminiCandidate, GeminiContent, GeminiPart, GeminiResponse,
        };

        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Here is the answer.".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let events = map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "Here is the answer.");
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Gemini stream chunk with text,
    /// When mapped, Then it produces an AssistantDelta.
    fn given_gemini_stream_chunk_when_mapped_then_assistant_delta() {
        use abp_gemini_sdk::dialect::{
            map_stream_chunk, GeminiCandidate, GeminiContent, GeminiPart, GeminiStreamChunk,
        };

        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("streaming token".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };

        let events = map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "streaming token"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Gemini capability manifest,
    /// When inspected, Then ToolGlob and ToolGrep are unsupported.
    fn given_gemini_capability_manifest_when_inspected_then_glob_grep_unsupported() {
        use abp_gemini_sdk::dialect::capability_manifest;

        let caps = capability_manifest();
        assert!(matches!(caps[&Capability::ToolGlob], SupportLevel::Unsupported));
        assert!(matches!(caps[&Capability::ToolGrep], SupportLevel::Unsupported));
        assert!(matches!(caps[&Capability::Streaming], SupportLevel::Native));
    }

    // ════════════════════════════════════════════════════════════════════
    // 4. Scenario: Cross-SDK translation (15+ tests)
    // ════════════════════════════════════════════════════════════════════

    #[test]
    /// Scenario: Given an OpenAI request with system message,
    /// When mapped to Claude dialect, Then system message becomes system parameter.
    fn given_openai_request_when_mapped_to_claude_then_system_becomes_parameter() {
        use abp_claude_sdk::lowering::{extract_system_prompt, from_ir};
        use abp_openai_sdk::dialect::OpenAIMessage;
        use abp_openai_sdk::lowering::to_ir;

        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("You are a code reviewer.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Review this code.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let ir = to_ir(&openai_msgs);
        let system_prompt = extract_system_prompt(&ir);
        let claude_msgs = from_ir(&ir);

        assert_eq!(
            system_prompt.as_deref(),
            Some("You are a code reviewer.")
        );
        assert_eq!(claude_msgs.len(), 1); // system skipped
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[0].content, "Review this code.");
    }

    #[test]
    /// Scenario: Given a Claude request with thinking blocks,
    /// When mapped to OpenAI, Then thinking text is included in output.
    fn given_claude_request_with_thinking_when_mapped_to_openai_then_thinking_included() {
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
        use abp_claude_sdk::lowering::to_ir as claude_to_ir;
        use abp_openai_sdk::lowering::from_ir as openai_from_ir;

        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "I need to analyze the code...".into(),
                signature: None,
            },
            ClaudeContentBlock::Text {
                text: "Here is my analysis.".into(),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let ir = claude_to_ir(&msgs, None);
        let openai_msgs = openai_from_ir(&ir);

        // OpenAI from_ir concatenates thinking text with regular text
        assert_eq!(openai_msgs.len(), 1);
        assert_eq!(openai_msgs[0].role, "assistant");
        let content = openai_msgs[0].content.as_deref().unwrap();
        assert!(content.contains("I need to analyze the code..."));
        assert!(content.contains("Here is my analysis."));
    }

    #[test]
    /// Scenario: Given an OpenAI request,
    /// When mapped through IR to Gemini, Then roles are correctly translated.
    fn given_openai_request_when_mapped_to_gemini_then_roles_translated() {
        use abp_gemini_sdk::lowering::from_ir as gemini_from_ir;
        use abp_openai_sdk::dialect::OpenAIMessage;
        use abp_openai_sdk::lowering::to_ir;

        let openai_msgs = vec![
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hi!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let ir = to_ir(&openai_msgs);
        let gemini_contents = gemini_from_ir(&ir);

        assert_eq!(gemini_contents.len(), 2);
        assert_eq!(gemini_contents[0].role, "user");
        assert_eq!(gemini_contents[1].role, "model");
    }

    #[test]
    /// Scenario: Given a Claude request,
    /// When mapped through IR to OpenAI, Then roles are correctly translated.
    fn given_claude_request_when_mapped_to_openai_then_roles_translated() {
        use abp_claude_sdk::dialect::ClaudeMessage;
        use abp_claude_sdk::lowering::to_ir as claude_to_ir;
        use abp_openai_sdk::lowering::from_ir as openai_from_ir;

        let msgs = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Hello".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Hi!".into(),
            },
        ];

        let ir = claude_to_ir(&msgs, None);
        let openai_msgs = openai_from_ir(&ir);

        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, "user");
        assert_eq!(openai_msgs[1].role, "assistant");
    }

    #[test]
    /// Scenario: Given a Gemini request with system instruction,
    /// When mapped to OpenAI, Then system instruction becomes system message.
    fn given_gemini_with_system_when_mapped_to_openai_then_system_message() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir as gemini_to_ir;
        use abp_openai_sdk::lowering::from_ir as openai_from_ir;

        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be helpful.".into())],
        };
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }];

        let ir = gemini_to_ir(&contents, Some(&sys));
        let openai_msgs = openai_from_ir(&ir);

        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Be helpful."));
        assert_eq!(openai_msgs[1].role, "user");
    }

    #[test]
    /// Scenario: Given an OpenAI tool call,
    /// When mapped through IR to Claude, Then tool_use block is emitted.
    fn given_openai_tool_call_when_mapped_to_claude_then_tool_use_emitted() {
        use abp_claude_sdk::dialect::ClaudeContentBlock;
        use abp_claude_sdk::lowering::from_ir as claude_from_ir;
        use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
        use abp_openai_sdk::lowering::to_ir;

        let openai_msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];

        let ir = to_ir(&openai_msgs);
        let claude_msgs = claude_from_ir(&ir);

        assert_eq!(claude_msgs.len(), 1);
        assert_eq!(claude_msgs[0].role, "assistant");
        let parsed: Vec<ClaudeContentBlock> =
            serde_json::from_str(&claude_msgs[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given an OpenAI tool call,
    /// When mapped through IR to Gemini, Then FunctionCall part is emitted.
    fn given_openai_tool_call_when_mapped_to_gemini_then_function_call_emitted() {
        use abp_gemini_sdk::dialect::GeminiPart;
        use abp_gemini_sdk::lowering::from_ir as gemini_from_ir;
        use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
        use abp_openai_sdk::lowering::to_ir;

        let openai_msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];

        let ir = to_ir(&openai_msgs);
        let gemini_contents = gemini_from_ir(&ir);

        assert_eq!(gemini_contents.len(), 1);
        assert_eq!(gemini_contents[0].role, "model");
        match &gemini_contents[0].parts[0] {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args, &json!({"q": "rust"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude tool result,
    /// When mapped through IR to OpenAI, Then the message retains user role
    /// because Claude maps tool results as IrRole::User (not IrRole::Tool).
    fn given_claude_tool_result_when_mapped_to_openai_then_user_role_with_tool_result() {
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
        use abp_claude_sdk::lowering::to_ir as claude_to_ir;
        use abp_openai_sdk::lowering::from_ir as openai_from_ir;

        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("file data here".into()),
            is_error: None,
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let ir = claude_to_ir(&msgs, None);
        // Claude maps tool results as IrRole::User, so OpenAI from_ir
        // cannot detect them as tool results — they remain "user" role.
        let openai_msgs = openai_from_ir(&ir);

        assert_eq!(openai_msgs.len(), 1);
        // Cross-SDK fidelity loss: Claude tool results mapped through IR
        // arrive as "user" in OpenAI format because Claude's IR role is User.
        assert_eq!(openai_msgs[0].role, "user");
    }

    #[test]
    /// Scenario: Given a Gemini function call/response cycle,
    /// When mapped through IR to OpenAI, Then tool call message is correct
    /// and function response retains user role due to IR role mapping.
    fn given_gemini_function_cycle_when_mapped_to_openai_then_correct_messages() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir as gemini_to_ir;
        use abp_openai_sdk::lowering::from_ir as openai_from_ir;

        let contents = vec![
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "read".into(),
                    args: json!({"file": "a.rs"}),
                }],
            },
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "read".into(),
                    response: json!("file content"),
                }],
            },
        ];

        let ir = gemini_to_ir(&contents, None);
        let openai_msgs = openai_from_ir(&ir);

        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, "assistant");
        assert!(openai_msgs[0].tool_calls.is_some());
        // Gemini maps function responses as IrRole::User, so OpenAI
        // cannot detect them as tool results — they remain "user" role.
        assert_eq!(openai_msgs[1].role, "user");
    }

    #[test]
    /// Scenario: Given an OpenAI tool definition,
    /// When mapped to Claude canonical, Then field names are translated.
    fn given_openai_tool_def_when_mapped_to_claude_then_field_names_translated() {
        use abp_claude_sdk::dialect::{tool_def_to_claude, CanonicalToolDef as ClaudeCanonical};
        use abp_openai_sdk::dialect::{tool_def_from_openai, OpenAIFunctionDef, OpenAIToolDef};

        let openai_def = OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "search".into(),
                description: "Search for content.".into(),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            },
        };

        let canonical = tool_def_from_openai(&openai_def);

        let claude_canonical = ClaudeCanonical {
            name: canonical.name.clone(),
            description: canonical.description.clone(),
            parameters_schema: canonical.parameters_schema.clone(),
        };
        let claude_def = tool_def_to_claude(&claude_canonical);

        assert_eq!(claude_def.name, "search");
        assert_eq!(claude_def.description, "Search for content.");
        assert_eq!(
            claude_def.input_schema,
            json!({"type": "object", "properties": {"q": {"type": "string"}}})
        );
    }

    #[test]
    /// Scenario: Given an OpenAI tool definition,
    /// When mapped to Gemini function declaration, Then parameters map.
    fn given_openai_tool_def_when_mapped_to_gemini_then_parameters_map() {
        use abp_gemini_sdk::dialect::{
            tool_def_to_gemini, CanonicalToolDef as GeminiCanonical,
        };
        use abp_openai_sdk::dialect::{tool_def_from_openai, OpenAIFunctionDef, OpenAIToolDef};

        let openai_def = OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "ls".into(),
                description: "List files.".into(),
                parameters: json!({"type": "object"}),
            },
        };

        let canonical = tool_def_from_openai(&openai_def);

        let gemini_canonical = GeminiCanonical {
            name: canonical.name.clone(),
            description: canonical.description.clone(),
            parameters_schema: canonical.parameters_schema.clone(),
        };
        let gemini_def = tool_def_to_gemini(&gemini_canonical);

        assert_eq!(gemini_def.name, "ls");
        assert_eq!(gemini_def.parameters, json!({"type": "object"}));
    }

    #[test]
    /// Scenario: Given a multi-turn Claude conversation with tool use,
    /// When mapped to OpenAI, Then the sequence of messages is preserved.
    fn given_claude_multi_turn_with_tools_when_mapped_to_openai_then_sequence_preserved() {
        use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
        use abp_claude_sdk::lowering::to_ir as claude_to_ir;
        use abp_openai_sdk::lowering::from_ir as openai_from_ir;

        let tool_use = vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read".into(),
            input: json!({}),
        }];
        let tool_result = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("data".into()),
            is_error: None,
        }];

        let msgs = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Do something".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: serde_json::to_string(&tool_use).unwrap(),
            },
            ClaudeMessage {
                role: "user".into(),
                content: serde_json::to_string(&tool_result).unwrap(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Done.".into(),
            },
        ];

        let ir = claude_to_ir(&msgs, None);
        let openai_msgs = openai_from_ir(&ir);

        assert_eq!(openai_msgs.len(), 4);
        assert_eq!(openai_msgs[0].role, "user");
        assert_eq!(openai_msgs[1].role, "assistant");
        assert!(openai_msgs[1].tool_calls.is_some());
        // Claude maps tool results as IrRole::User, so OpenAI sees "user" role
        assert_eq!(openai_msgs[2].role, "user");
        assert_eq!(openai_msgs[3].role, "assistant");
    }

    #[test]
    /// Scenario: Given a Gemini conversation,
    /// When mapped through IR to Claude, Then model role becomes assistant.
    fn given_gemini_conversation_when_mapped_to_claude_then_model_is_assistant() {
        use abp_claude_sdk::lowering::from_ir as claude_from_ir;
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering::to_ir as gemini_to_ir;

        let contents = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hi there!".into())],
            },
        ];

        let ir = gemini_to_ir(&contents, None);
        let claude_msgs = claude_from_ir(&ir);

        assert_eq!(claude_msgs.len(), 2);
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[1].role, "assistant");
        assert_eq!(claude_msgs[1].content, "Hi there!");
    }

    #[test]
    /// Scenario: Given capability mismatch between OpenAI and Claude,
    /// When checked, Then capabilities differ.
    fn given_capability_mismatch_when_checked_then_differences_found() {
        let openai_caps = abp_openai_sdk::dialect::capability_manifest();
        let claude_caps = abp_claude_sdk::dialect::capability_manifest();

        // OpenAI has tools as emulated, Claude has them as native
        assert!(matches!(openai_caps[&Capability::ToolRead], SupportLevel::Emulated));
        assert!(matches!(claude_caps[&Capability::ToolRead], SupportLevel::Native));

        // OpenAI doesn't support MCP, Claude does
        assert!(matches!(openai_caps[&Capability::McpClient], SupportLevel::Unsupported));
        assert!(matches!(claude_caps[&Capability::McpClient], SupportLevel::Native));
    }

    #[test]
    /// Scenario: Given empty messages in any SDK,
    /// When mapped through IR, Then empty conversation roundtrips safely.
    fn given_empty_messages_when_mapped_through_ir_then_empty_roundtrip() {
        use abp_claude_sdk::lowering::{from_ir as claude_from_ir, to_ir as claude_to_ir};
        use abp_gemini_sdk::lowering::{from_ir as gemini_from_ir, to_ir as gemini_to_ir};
        use abp_openai_sdk::lowering::{from_ir as openai_from_ir, to_ir as openai_to_ir};

        let openai_ir = openai_to_ir(&[]);
        let claude_ir = claude_to_ir(&[], None);
        let gemini_ir = gemini_to_ir(&[], None);

        assert!(openai_ir.is_empty());
        assert!(claude_ir.is_empty());
        assert!(gemini_ir.is_empty());

        assert!(openai_from_ir(&openai_ir).is_empty());
        assert!(claude_from_ir(&claude_ir).is_empty());
        assert!(gemini_from_ir(&gemini_ir).is_empty());
    }

    // ════════════════════════════════════════════════════════════════════
    // 5. Scenario: Error paths (10+ tests)
    // ════════════════════════════════════════════════════════════════════

    #[test]
    /// Scenario: Given an unknown model name for OpenAI,
    /// When checked with is_known_model, Then it returns false.
    fn given_unknown_openai_model_when_checked_then_not_known() {
        use abp_openai_sdk::dialect::is_known_model;

        assert!(!is_known_model("gpt-99-turbo"));
        assert!(!is_known_model("claude-sonnet-4-20250514"));
        assert!(!is_known_model(""));
    }

    #[test]
    /// Scenario: Given an unknown model name for Claude,
    /// When checked with is_known_model, Then it returns false.
    fn given_unknown_claude_model_when_checked_then_not_known() {
        use abp_claude_sdk::dialect::is_known_model;

        assert!(!is_known_model("gpt-4o"));
        assert!(!is_known_model("claude-nonexistent"));
        assert!(!is_known_model(""));
    }

    #[test]
    /// Scenario: Given an unknown model name for Gemini,
    /// When checked with is_known_model, Then it returns false.
    fn given_unknown_gemini_model_when_checked_then_not_known() {
        use abp_gemini_sdk::dialect::is_known_model;

        assert!(!is_known_model("gpt-4o"));
        assert!(!is_known_model("gemini-99"));
        assert!(!is_known_model(""));
    }

    #[test]
    /// Scenario: Given a MappingError::IncompatibleModel,
    /// When created, Then it is classified as fatal.
    fn given_incompatible_model_error_when_created_then_fatal() {
        let err = MappingError::IncompatibleModel {
            requested: "gpt-4o".into(),
            dialect: "anthropic".into(),
            suggestion: Some("claude-sonnet-4-20250514".into()),
        };

        assert_eq!(err.kind(), MappingErrorKind::Fatal);
        let display = format!("{err}");
        assert!(display.contains("gpt-4o"));
        assert!(display.contains("anthropic"));
    }

    #[test]
    /// Scenario: Given a MappingError::UnsupportedCapability,
    /// When created, Then it is classified as fatal.
    fn given_unsupported_capability_error_when_created_then_fatal() {
        let err = MappingError::UnsupportedCapability {
            capability: "mcp_server".into(),
            dialect: "openai".into(),
        };

        assert_eq!(err.kind(), MappingErrorKind::Fatal);
        let display = format!("{err}");
        assert!(display.contains("mcp_server"));
        assert!(display.contains("openai"));
    }

    #[test]
    /// Scenario: Given a MappingError::FidelityLoss,
    /// When created, Then it is classified as degraded.
    fn given_fidelity_loss_error_when_created_then_degraded() {
        let err = MappingError::FidelityLoss {
            field: "thinking".into(),
            source_dialect: "claude".into(),
            target_dialect: "openai".into(),
            detail: "Thinking blocks have no native equivalent".into(),
        };

        assert_eq!(err.kind(), MappingErrorKind::Degraded);
        let display = format!("{err}");
        assert!(display.contains("thinking"));
        assert!(display.contains("fidelity loss"));
    }

    #[test]
    /// Scenario: Given a MappingError::EmulationRequired,
    /// When created, Then it is classified as emulated.
    fn given_emulation_required_error_when_created_then_emulated() {
        let err = MappingError::EmulationRequired {
            feature: "tool_bash".into(),
            detail: "OpenAI does not natively support bash tool".into(),
        };

        assert_eq!(err.kind(), MappingErrorKind::Emulated);
    }

    #[test]
    /// Scenario: Given OpenAI extended fields with seed,
    /// When validated for mapped mode, Then validation fails for seed.
    fn given_openai_extended_fields_with_seed_when_validated_then_seed_error() {
        use abp_openai_sdk::validation::{validate_for_mapped_mode, ExtendedRequestFields};

        let fields = ExtendedRequestFields {
            logprobs: None,
            top_logprobs: None,
            logit_bias: None,
            seed: Some(42),
        };

        let result = validate_for_mapped_mode(&fields);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.param == "seed"));
    }

    #[test]
    /// Scenario: Given OpenAI extended fields with logit_bias,
    /// When validated for mapped mode, Then validation fails for logit_bias.
    fn given_openai_extended_fields_with_logit_bias_when_validated_then_logit_bias_error() {
        use abp_openai_sdk::validation::{validate_for_mapped_mode, ExtendedRequestFields};
        use std::collections::BTreeMap;

        let mut bias = BTreeMap::new();
        bias.insert("50256".into(), -100.0);

        let fields = ExtendedRequestFields {
            logprobs: None,
            top_logprobs: None,
            logit_bias: Some(bias),
            seed: None,
        };

        let result = validate_for_mapped_mode(&fields);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.param == "logit_bias"));
    }

    #[test]
    /// Scenario: Given OpenAI extended fields with all unmappable params,
    /// When validated, Then all errors are reported.
    fn given_openai_all_unmappable_params_when_validated_then_all_errors_reported() {
        use abp_openai_sdk::validation::{validate_for_mapped_mode, ExtendedRequestFields};
        use std::collections::BTreeMap;

        let mut bias = BTreeMap::new();
        bias.insert("100".into(), 5.0);

        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            top_logprobs: Some(5),
            logit_bias: Some(bias),
            seed: Some(123),
        };

        let result = validate_for_mapped_mode(&fields);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.len() >= 3);
    }

    #[test]
    /// Scenario: Given OpenAI extended fields with no unmappable params,
    /// When validated, Then validation succeeds.
    fn given_openai_no_unmappable_params_when_validated_then_succeeds() {
        use abp_openai_sdk::validation::{validate_for_mapped_mode, ExtendedRequestFields};

        let fields = ExtendedRequestFields {
            logprobs: None,
            top_logprobs: None,
            logit_bias: None,
            seed: None,
        };

        assert!(validate_for_mapped_mode(&fields).is_ok());
    }

    #[test]
    /// Scenario: Given a Claude API error during streaming,
    /// When mapped to event, Then Error event is produced with message.
    fn given_claude_stream_error_when_mapped_then_error_event() {
        use abp_claude_sdk::dialect::{map_stream_event, ClaudeApiError, ClaudeStreamEvent};

        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "rate_limit_error".into(),
                message: "Too many requests".into(),
            },
        };

        let events = map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("rate_limit_error"));
                assert!(message.contains("Too many requests"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given malformed tool arguments in OpenAI,
    /// When lowered to IR, Then arguments are preserved as string value.
    fn given_malformed_openai_tool_args_when_lowered_then_preserved_as_string() {
        use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
        use abp_openai_sdk::lowering::to_ir;

        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "broken".into(),
                    arguments: "not valid json {{{".into(),
                },
            }]),
            tool_call_id: None,
        }];

        let conv = to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert_eq!(
                    input,
                    &serde_json::Value::String("not valid json {{{".into())
                );
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    // ════════════════════════════════════════════════════════════════════
    // Additional cross-SDK scenarios for comprehensive coverage
    // ════════════════════════════════════════════════════════════════════

    #[test]
    /// Scenario: Given a Claude passthrough event,
    /// When roundtripped, Then the original event is preserved.
    fn given_claude_passthrough_event_when_roundtripped_then_preserved() {
        use abp_claude_sdk::dialect::{
            from_passthrough_event, to_passthrough_event, ClaudeStreamDelta, ClaudeStreamEvent,
        };

        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "hello world".into(),
            },
        };

        let wrapped = to_passthrough_event(&event);
        let ext = wrapped.ext.as_ref().unwrap();
        assert_eq!(
            ext.get("dialect").unwrap(),
            &serde_json::Value::String("claude".into())
        );

        let recovered = from_passthrough_event(&wrapped);
        assert_eq!(recovered.as_ref(), Some(&event));
    }

    #[test]
    /// Scenario: Given Claude passthrough fidelity check,
    /// When verified with multiple events, Then all events roundtrip.
    fn given_claude_passthrough_fidelity_when_verified_then_all_roundtrip() {
        use abp_claude_sdk::dialect::{
            verify_passthrough_fidelity, ClaudeResponse, ClaudeStreamDelta, ClaudeStreamEvent,
        };

        let events = vec![
            ClaudeStreamEvent::MessageStart {
                message: ClaudeResponse {
                    id: "msg_123".into(),
                    model: "claude-sonnet-4-20250514".into(),
                    role: "assistant".into(),
                    content: vec![],
                    stop_reason: None,
                    usage: None,
                },
            },
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta {
                    text: "Hello".into(),
                },
            },
            ClaudeStreamEvent::MessageStop {},
        ];

        assert!(verify_passthrough_fidelity(&events));
    }

    #[test]
    /// Scenario: Given a Claude tool result helper,
    /// When constructed, Then the message has correct format.
    fn given_claude_tool_result_helper_when_constructed_then_correct_format() {
        use abp_claude_sdk::dialect::{map_tool_result, ClaudeContentBlock};

        let msg = map_tool_result("tu_1", "file contents here", false);
        assert_eq!(msg.role, "user");

        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tu_1");
                assert_eq!(content.as_deref(), Some("file contents here"));
                assert!(is_error.is_none());
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Claude tool result helper with error,
    /// When constructed, Then is_error is Some(true).
    fn given_claude_tool_result_error_helper_when_constructed_then_error_flag_set() {
        use abp_claude_sdk::dialect::{map_tool_result, ClaudeContentBlock};

        let msg = map_tool_result("tu_err", "not found", true);
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    /// Scenario: Given a Gemini request converted to WorkOrder,
    /// When the request has system instruction, Then it appears as context snippet.
    fn given_gemini_request_to_work_order_when_system_instruction_then_context_snippet() {
        use abp_core::WorkOrder;
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart, GeminiRequest};

        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            }],
            system_instruction: Some(GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Be concise.".into())],
            }),
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };

        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Hello");
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].name, "system_instruction");
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    /// Scenario: Given a Claude stop reason string,
    /// When parsed, Then the typed enum is returned.
    fn given_claude_stop_reason_when_parsed_then_typed_enum() {
        use abp_claude_sdk::dialect::{parse_stop_reason, ClaudeStopReason};

        assert_eq!(
            parse_stop_reason("end_turn"),
            Some(ClaudeStopReason::EndTurn)
        );
        assert_eq!(
            parse_stop_reason("tool_use"),
            Some(ClaudeStopReason::ToolUse)
        );
        assert_eq!(
            parse_stop_reason("max_tokens"),
            Some(ClaudeStopReason::MaxTokens)
        );
        assert_eq!(
            parse_stop_reason("stop_sequence"),
            Some(ClaudeStopReason::StopSequence)
        );
        assert_eq!(parse_stop_reason("unknown"), None);
    }

    #[test]
    /// Scenario: Given a Claude ThinkingConfig,
    /// When created, Then type is "enabled" and budget is set.
    fn given_claude_thinking_config_when_created_then_correct_fields() {
        use abp_claude_sdk::dialect::ThinkingConfig;

        let config = ThinkingConfig::new(16384);
        assert_eq!(config.thinking_type, "enabled");
        assert_eq!(config.budget_tokens, 16384);
    }

    #[test]
    /// Scenario: Given an OpenAI response format,
    /// When json_schema is created, Then strict is set to true.
    fn given_openai_response_format_json_schema_when_created_then_strict() {
        use abp_openai_sdk::response_format::ResponseFormat;

        let fmt = ResponseFormat::json_schema(
            "answer",
            json!({"type": "object", "properties": {"answer": {"type": "string"}}}),
        );

        match &fmt {
            ResponseFormat::JsonSchema { json_schema } => {
                assert_eq!(json_schema.name, "answer");
                assert_eq!(json_schema.strict, Some(true));
            }
            other => panic!("expected JsonSchema, got {other:?}"),
        }
    }
}
