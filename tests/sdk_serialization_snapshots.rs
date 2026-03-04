// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive insta snapshot tests for all SDK type serialization.
//!
//! Covers request, response, streaming, event, error, WorkOrder, Receipt,
//! AgentEvent, and IR representations for every supported SDK dialect.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
};
use chrono::{TimeZone, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Fixed UUID for deterministic snapshots.
fn fixed_uuid() -> Uuid {
    Uuid::parse_str("a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d8").unwrap()
}

/// Fixed timestamp for deterministic snapshots.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

/// Build a deterministic receipt for conversion tests.
fn make_receipt(backend: &str) -> abp_core::Receipt {
    let ts = fixed_ts();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .work_order_id(fixed_uuid())
        .started_at(ts)
        .finished_at(ts + chrono::Duration::milliseconds(500))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        })
        .usage_raw(json!({"input_tokens": 100, "output_tokens": 50}))
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "Hello from the assistant.".into(),
            },
            ext: None,
        })
        .build()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI SDK
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::convert;
    use abp_shim_openai::types::*;

    // ── Request serialization ───────────────────────────────────────────

    #[test]
    fn request_minimal() {
        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hi".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_full() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are helpful.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Solve 2+2".into()),
                },
                ChatMessage::Assistant {
                    content: Some("4".into()),
                    tool_calls: None,
                },
            ],
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(1024),
            stream: Some(false),
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDef {
                    name: "get_weather".into(),
                    description: "Get weather".into(),
                    parameters: json!({"type": "object", "properties": {"loc": {"type": "string"}}}),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_with_tools() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Search docs".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: Some(vec![
                Tool {
                    tool_type: "function".into(),
                    function: FunctionDef {
                        name: "search".into(),
                        description: "Search documents".into(),
                        parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
                    },
                },
                Tool {
                    tool_type: "function".into(),
                    function: FunctionDef {
                        name: "read_file".into(),
                        description: "Read a file".into(),
                        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                    },
                },
            ]),
            tool_choice: Some(ToolChoice::Function {
                tool_type: "function".into(),
                function: ToolChoiceFunctionRef {
                    name: "search".into(),
                },
            }),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_streaming() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Stream me".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: Some(true),
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!(req);
    }

    // ── Response serialization ──────────────────────────────────────────

    #[test]
    fn response_success() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-001".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o-2024-05-13".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: Some("The answer is 4.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 25,
                completion_tokens: 8,
                total_tokens: 33,
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_error_content_filter() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-err".into(),
            object: "chat.completion".into(),
            created: 1700000100,
            model: "gpt-4o".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: None,
                },
                finish_reason: Some("content_filter".into()),
            }],
            usage: None,
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_tool_calls() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-tc".into(),
            object: "chat.completion".into(),
            created: 1700000200,
            model: "gpt-4o".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_xyz".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "get_weather".into(),
                            arguments: r#"{"location":"SF"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 20,
                total_tokens: 70,
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    // ── Streaming chunk ─────────────────────────────────────────────────

    #[test]
    fn stream_chunk_text() {
        let chunk = StreamChunk {
            id: "chatcmpl-s1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000300,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!(chunk);
    }

    #[test]
    fn stream_chunk_tool_call() {
        let chunk = StreamChunk {
            id: "chatcmpl-s2".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000400,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: Some("call_1".into()),
                        call_type: Some("function".into()),
                        function: Some(StreamFunctionCall {
                            name: Some("search".into()),
                            arguments: Some(r#"{"q":"rust"}"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!(chunk);
    }

    // ── WorkOrder from OpenAI request ───────────────────────────────────

    #[test]
    fn work_order_from_request() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "Be concise.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Explain Rust".into()),
                },
            ],
            temperature: Some(0.5),
            top_p: None,
            max_tokens: Some(512),
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        insta::assert_json_snapshot!(wo, { ".id" => "[uuid]" });
    }

    // ── Receipt conversion ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response() {
        let receipt = make_receipt("openai");
        let wo = WorkOrderBuilder::new("Test task").build();
        let resp = convert::from_receipt(&receipt, &wo);
        insta::assert_json_snapshot!(resp, {
            ".id" => "[id]",
            ".created" => "[created]",
        });
    }

    // ── IR representation ───────────────────────────────────────────────

    #[test]
    fn ir_from_openai_messages() {
        use abp_openai_sdk::dialect::OpenAIMessage;
        use abp_openai_sdk::lowering;

        let messages = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("You are a tutor.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("What is 2+2?".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = lowering::to_ir(&messages);
        insta::assert_yaml_snapshot!(ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Claude SDK
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::convert;
    use abp_shim_claude::types::*;

    // ── Request serialization ───────────────────────────────────────────

    #[test]
    fn request_minimal() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hello".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_full() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                ClaudeMessage {
                    role: "user".into(),
                    content: ClaudeContent::Text("What is Rust?".into()),
                },
                ClaudeMessage {
                    role: "assistant".into(),
                    content: ClaudeContent::Text("Rust is a systems language.".into()),
                },
            ],
            max_tokens: 4096,
            system: Some("You are an expert programmer.".into()),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            stream: Some(false),
            tools: Some(vec![ClaudeTool {
                name: "read_file".into(),
                description: Some("Read a file from disk".into()),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }]),
            tool_choice: Some(ClaudeToolChoice::Auto {}),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_with_tools() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Blocks(vec![ContentBlock::Text {
                    text: "Search for docs".into(),
                }]),
            }],
            max_tokens: 2048,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: Some(vec![
                ClaudeTool {
                    name: "search".into(),
                    description: Some("Search docs".into()),
                    input_schema: json!({"type": "object"}),
                },
                ClaudeTool {
                    name: "write_file".into(),
                    description: Some("Write content to a file".into()),
                    input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
                },
            ]),
            tool_choice: Some(ClaudeToolChoice::Any {}),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_streaming() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Stream this".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: Some(true),
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!(req);
    }

    // ── Response serialization ──────────────────────────────────────────

    #[test]
    fn response_success() {
        let resp = MessagesResponse {
            id: "msg_001".into(),
            type_field: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Hello! How can I help?".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            usage: ClaudeUsage {
                input_tokens: 10,
                output_tokens: 8,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_tool_use() {
        let resp = MessagesResponse {
            id: "msg_002".into(),
            type_field: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: "toolu_01".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("tool_use".into()),
            usage: ClaudeUsage {
                input_tokens: 20,
                output_tokens: 15,
                cache_creation_input_tokens: Some(5),
                cache_read_input_tokens: None,
            },
        };
        insta::assert_json_snapshot!(resp);
    }

    // ── Streaming events ────────────────────────────────────────────────

    #[test]
    fn stream_text_delta() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello world".into(),
            },
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn stream_message_start() {
        let event = StreamEvent::MessageStart {
            message: MessagesResponse {
                id: "msg_s01".into(),
                type_field: "message".into(),
                role: "assistant".into(),
                content: vec![],
                model: "claude-sonnet-4-20250514".into(),
                stop_reason: None,
                usage: ClaudeUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn stream_message_delta() {
        let event = StreamEvent::MessageDelta {
            delta: MessageDeltaBody {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: Some(ClaudeUsage {
                input_tokens: 0,
                output_tokens: 42,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn stream_ping() {
        let event = StreamEvent::Ping {};
        insta::assert_json_snapshot!(event);
    }

    // ── WorkOrder from Claude request ───────────────────────────────────

    #[test]
    fn work_order_from_request() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Explain Rust ownership".into()),
            }],
            max_tokens: 2048,
            system: Some("Be concise.".into()),
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        insta::assert_json_snapshot!(wo, { ".id" => "[uuid]" });
    }

    // ── Receipt conversion ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response() {
        let receipt = make_receipt("claude");
        let wo = WorkOrderBuilder::new("Test task").build();
        let resp = convert::from_receipt(&receipt, &wo);
        insta::assert_json_snapshot!(resp, {
            ".id" => "[id]",
        });
    }

    // ── IR representation ───────────────────────────────────────────────

    #[test]
    fn ir_from_claude_messages() {
        use abp_claude_sdk::dialect::ClaudeMessage as DialectMsg;
        use abp_claude_sdk::lowering;

        let messages = vec![
            DialectMsg {
                role: "user".into(),
                content: "Hello".into(),
            },
            DialectMsg {
                role: "assistant".into(),
                content: "Hi there!".into(),
            },
        ];
        let ir = lowering::to_ir(&messages, Some("System prompt"));
        insta::assert_yaml_snapshot!(ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Gemini SDK
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_gemini_sdk::dialect::{FunctionCallingMode, HarmBlockThreshold, HarmCategory};
    use abp_shim_gemini::types::*;

    // ── Request serialization ───────────────────────────────────────────

    #[test]
    fn request_minimal() {
        let req = GenerateContentRequest {
            model: "gemini-2.5-pro".into(),
            contents: vec![Content {
                role: "user".into(),
                parts: vec![Part::text("Hello")],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_full() {
        let req = GenerateContentRequest {
            model: "gemini-2.5-pro".into(),
            contents: vec![
                Content {
                    role: "user".into(),
                    parts: vec![Part::text("What is Rust?")],
                },
                Content {
                    role: "model".into(),
                    parts: vec![Part::text("Rust is a systems language.")],
                },
            ],
            system_instruction: Some(Content {
                role: "user".into(),
                parts: vec![Part::text("Be concise.")],
            }),
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                max_output_tokens: Some(2048),
                stop_sequences: Some(vec!["END".into()]),
                response_mime_type: None,
                response_schema: None,
            }),
            safety_settings: Some(vec![SafetySetting {
                category: HarmCategory::HarmCategoryDangerousContent,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            }]),
            tools: Some(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: "Get weather for a location".into(),
                    parameters: json!({"type": "object", "properties": {"loc": {"type": "string"}}}),
                }],
            }]),
            tool_config: Some(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            }),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_with_tools() {
        let req = GenerateContentRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![Content {
                role: "user".into(),
                parts: vec![Part::text("Call the search tool")],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: Some(vec![ToolDeclaration {
                function_declarations: vec![
                    FunctionDeclaration {
                        name: "search".into(),
                        description: "Search docs".into(),
                        parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
                    },
                    FunctionDeclaration {
                        name: "read_file".into(),
                        description: "Read a file".into(),
                        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                    },
                ],
            }]),
            tool_config: Some(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Any,
                    allowed_function_names: Some(vec!["search".into()]),
                },
            }),
        };
        insta::assert_json_snapshot!(req);
    }

    // ── Response serialization ──────────────────────────────────────────

    #[test]
    fn response_success() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".into(),
                    parts: vec![Part::text("The answer is 4.")],
                },
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 8,
                total_token_count: 18,
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_function_call() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".into(),
                    parts: vec![Part::FunctionCall {
                        name: "get_weather".into(),
                        args: json!({"location": "Tokyo"}),
                    }],
                },
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 15,
                candidates_token_count: 12,
                total_token_count: 27,
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    // ── Streaming chunk ─────────────────────────────────────────────────

    #[test]
    fn stream_event_text() {
        let event = StreamEvent {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".into(),
                    parts: vec![Part::text("Hello")],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        };
        insta::assert_json_snapshot!(event);
    }

    // ── WorkOrder from Gemini request ───────────────────────────────────

    #[test]
    fn work_order_from_request() {
        use abp_shim_gemini::convert;

        let req = GenerateContentRequest {
            model: "gemini-2.5-pro".into(),
            contents: vec![Content {
                role: "user".into(),
                parts: vec![Part::text("Explain ownership")],
            }],
            system_instruction: None,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.5),
                max_output_tokens: Some(1024),
                ..Default::default()
            }),
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let (ir_req, gen_config, _safety) = convert::request_to_ir(&req).unwrap();
        let wo = convert::ir_to_work_order(&ir_req, "gemini-2.5-pro", &gen_config);
        insta::assert_json_snapshot!(wo, { ".id" => "[uuid]" });
    }

    // ── IR representation ───────────────────────────────────────────────

    #[test]
    fn ir_from_gemini_contents() {
        use abp_gemini_sdk::dialect::GeminiContent;
        use abp_gemini_sdk::lowering;

        let contents = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                    "What is Rust?".into(),
                )],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                    "A systems language.".into(),
                )],
            },
        ];
        let ir = lowering::to_ir(&contents, None);
        insta::assert_yaml_snapshot!(ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Codex SDK
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::*;
    use abp_shim_codex::convert;

    // ── Request serialization ───────────────────────────────────────────

    #[test]
    fn request_minimal() {
        let req = CodexRequest {
            model: "codex-mini".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Hello".into(),
            }],
            max_output_tokens: None,
            temperature: None,
            tools: vec![],
            text: None,
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_full() {
        let req = CodexRequest {
            model: "codex-mini".into(),
            input: vec![
                CodexInputItem::Message {
                    role: "system".into(),
                    content: "You are a coding assistant.".into(),
                },
                CodexInputItem::Message {
                    role: "user".into(),
                    content: "Write a Rust function".into(),
                },
            ],
            max_output_tokens: Some(4096),
            temperature: Some(0.2),
            tools: vec![CodexTool::Function {
                function: CodexFunctionDef {
                    name: "run_tests".into(),
                    description: "Run test suite".into(),
                    parameters: json!({"type": "object"}),
                },
            }],
            text: Some(CodexTextFormat::Text {}),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_with_tools() {
        let req = CodexRequest {
            model: "codex-mini".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Analyze code".into(),
            }],
            max_output_tokens: None,
            temperature: None,
            tools: vec![
                CodexTool::Function {
                    function: CodexFunctionDef {
                        name: "read_file".into(),
                        description: "Read file".into(),
                        parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
                    },
                },
                CodexTool::CodeInterpreter {},
            ],
            text: None,
        };
        insta::assert_json_snapshot!(req);
    }

    // ── Response serialization ──────────────────────────────────────────

    #[test]
    fn response_success() {
        let resp = CodexResponse {
            id: "resp_001".into(),
            model: "codex-mini".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Here is your function.".into(),
                }],
            }],
            usage: Some(CodexUsage {
                input_tokens: 50,
                output_tokens: 30,
                total_tokens: 80,
            }),
            status: Some("completed".into()),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_with_function_call() {
        let resp = CodexResponse {
            id: "resp_002".into(),
            model: "codex-mini".into(),
            output: vec![CodexResponseItem::FunctionCall {
                id: "fc_01".into(),
                call_id: Some("call_01".into()),
                name: "run_tests".into(),
                arguments: r#"{"suite":"unit"}"#.into(),
            }],
            usage: Some(CodexUsage {
                input_tokens: 40,
                output_tokens: 25,
                total_tokens: 65,
            }),
            status: Some("completed".into()),
        };
        insta::assert_json_snapshot!(resp);
    }

    // ── Streaming events ────────────────────────────────────────────────

    #[test]
    fn stream_text_delta() {
        let event = CodexStreamEvent::OutputItemDelta {
            output_index: 0,
            delta: CodexStreamDelta::OutputTextDelta {
                text: "Hello world".into(),
            },
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn stream_error() {
        let event = CodexStreamEvent::Error {
            message: "Rate limit exceeded".into(),
            code: Some("rate_limit".into()),
        };
        insta::assert_json_snapshot!(event);
    }

    // ── WorkOrder from Codex request ────────────────────────────────────

    #[test]
    fn work_order_from_request() {
        let req = CodexRequest {
            model: "codex-mini".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Write tests".into(),
            }],
            max_output_tokens: Some(2048),
            temperature: Some(0.1),
            tools: vec![],
            text: None,
        };
        let wo = convert::request_to_work_order(&req);
        insta::assert_json_snapshot!(wo, { ".id" => "[uuid]" });
    }

    // ── Receipt conversion ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response() {
        let receipt = make_receipt("codex");
        let resp = convert::receipt_to_response(&receipt, "codex-mini");
        insta::assert_json_snapshot!(resp, { ".id" => "[id]" });
    }

    // ── IR representation ───────────────────────────────────────────────

    #[test]
    fn ir_from_codex_input() {
        let items = vec![
            CodexInputItem::Message {
                role: "system".into(),
                content: "You code.".into(),
            },
            CodexInputItem::Message {
                role: "user".into(),
                content: "Write hello world".into(),
            },
        ];
        let ir = abp_codex_sdk::lowering::input_to_ir(&items);
        insta::assert_yaml_snapshot!(ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Copilot SDK
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::*;
    use abp_shim_copilot::convert;

    // ── Request serialization ───────────────────────────────────────────

    #[test]
    fn request_minimal() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Hello".into(),
                name: None,
                copilot_references: vec![],
            }],
            tools: None,
            turn_history: vec![],
            references: vec![],
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_full() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotMessage {
                    role: "system".into(),
                    content: "You are a GitHub Copilot assistant.".into(),
                    name: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "user".into(),
                    content: "Explain this code".into(),
                    name: Some("user1".into()),
                    copilot_references: vec![CopilotReference {
                        ref_type: CopilotReferenceType::File,
                        id: "src/main.rs".into(),
                        data: json!({"content": "fn main() {}"}),
                        metadata: None,
                    }],
                },
            ],
            tools: Some(vec![CopilotTool {
                tool_type: CopilotToolType::Function,
                function: Some(CopilotFunctionDef {
                    name: "search_code".into(),
                    description: "Search codebase".into(),
                    parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
                }),
                confirmation: None,
            }]),
            turn_history: vec![CopilotTurnEntry {
                request: "Previous question".into(),
                response: "Previous answer".into(),
            }],
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::Repository,
                id: "myorg/myrepo".into(),
                data: json!({"url": "https://github.com/myorg/myrepo"}),
                metadata: None,
            }],
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_with_tools() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Search for auth code".into(),
                name: None,
                copilot_references: vec![],
            }],
            tools: Some(vec![
                CopilotTool {
                    tool_type: CopilotToolType::Function,
                    function: Some(CopilotFunctionDef {
                        name: "search".into(),
                        description: "Search code".into(),
                        parameters: json!({"type":"object"}),
                    }),
                    confirmation: None,
                },
                CopilotTool {
                    tool_type: CopilotToolType::Confirmation,
                    function: None,
                    confirmation: Some(CopilotConfirmation {
                        id: "confirm_1".into(),
                        title: "Confirm action".into(),
                        message: "Are you sure?".into(),
                        accepted: None,
                    }),
                },
            ]),
            turn_history: vec![],
            references: vec![],
        };
        insta::assert_json_snapshot!(req);
    }

    // ── Response serialization ──────────────────────────────────────────

    #[test]
    fn response_success() {
        let resp = CopilotResponse {
            message: "Here is the explanation.".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_with_errors() {
        let resp = CopilotResponse {
            message: "".into(),
            copilot_references: vec![],
            copilot_errors: vec![CopilotError {
                error_type: "rate_limit".into(),
                message: "Too many requests".into(),
                code: Some("429".into()),
                identifier: Some("req_001".into()),
            }],
            copilot_confirmation: None,
            function_call: None,
        };
        insta::assert_json_snapshot!(resp);
    }

    // ── Streaming events ────────────────────────────────────────────────

    #[test]
    fn stream_text_delta() {
        let event = CopilotStreamEvent::TextDelta {
            text: "Hello from Copilot".into(),
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn stream_function_call() {
        let event = CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "search_code".into(),
                arguments: r#"{"q":"auth"}"#.into(),
                id: Some("fc_01".into()),
            },
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn stream_done() {
        let event = CopilotStreamEvent::Done {};
        insta::assert_json_snapshot!(event);
    }

    // ── WorkOrder from Copilot request ──────────────────────────────────

    #[test]
    fn work_order_from_request() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Explain async Rust".into(),
                name: None,
                copilot_references: vec![],
            }],
            tools: None,
            turn_history: vec![],
            references: vec![],
        };
        let wo = convert::request_to_work_order(&req);
        insta::assert_json_snapshot!(wo, { ".id" => "[uuid]" });
    }

    // ── Receipt conversion ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response() {
        let receipt = make_receipt("copilot");
        let resp = convert::receipt_to_response(&receipt, "gpt-4o");
        insta::assert_json_snapshot!(resp);
    }

    // ── IR representation ───────────────────────────────────────────────

    #[test]
    fn ir_from_copilot_messages() {
        let messages = vec![
            CopilotMessage {
                role: "system".into(),
                content: "You are Copilot.".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "user".into(),
                content: "How do I write tests?".into(),
                name: None,
                copilot_references: vec![],
            },
        ];
        let ir = abp_copilot_sdk::lowering::to_ir(&messages);
        insta::assert_yaml_snapshot!(ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Kimi SDK
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::*;
    use abp_shim_kimi::convert;

    // ── Request serialization ───────────────────────────────────────────

    #[test]
    fn request_minimal() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_full() {
        let req = KimiRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![
                KimiMessage {
                    role: "system".into(),
                    content: Some("You are a helpful assistant.".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                KimiMessage {
                    role: "user".into(),
                    content: Some("Search for Rust tutorials".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: Some(false),
            tools: Some(vec![KimiTool::Function {
                function: KimiFunctionDef {
                    name: "web_search".into(),
                    description: "Search the web".into(),
                    parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
                },
            }]),
            use_search: Some(true),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn request_streaming() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage {
                role: "user".into(),
                content: Some("Stream response".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: Some(true),
            tools: None,
            use_search: None,
        };
        insta::assert_json_snapshot!(req);
    }

    // ── Response serialization ──────────────────────────────────────────

    #[test]
    fn response_success() {
        let resp = KimiResponse {
            id: "cmpl-kimi-001".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hello! How can I help?".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 10,
                completion_tokens: 8,
                total_tokens: 18,
            }),
            refs: None,
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn response_with_tool_calls() {
        let resp = KimiResponse {
            id: "cmpl-kimi-002".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![KimiToolCall {
                        id: "tc_01".into(),
                        call_type: "function".into(),
                        function: KimiFunctionCall {
                            name: "web_search".into(),
                            arguments: r#"{"q":"Rust tutorials"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                total_tokens: 35,
            }),
            refs: Some(vec![KimiRef {
                index: 0,
                url: "https://doc.rust-lang.org".into(),
                title: Some("The Rust Book".into()),
            }]),
        };
        insta::assert_json_snapshot!(resp);
    }

    // ── Streaming chunk ─────────────────────────────────────────────────

    #[test]
    fn stream_chunk_text() {
        let chunk = KimiChunk {
            id: "chunk-001".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        insta::assert_json_snapshot!(chunk);
    }

    #[test]
    fn stream_chunk_tool_call() {
        let chunk = KimiChunk {
            id: "chunk-002".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000100,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![KimiChunkToolCall {
                        index: 0,
                        id: Some("tc_s1".into()),
                        call_type: Some("function".into()),
                        function: Some(KimiChunkFunctionCall {
                            name: Some("web_search".into()),
                            arguments: Some(r#"{"q":"rust"}"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        insta::assert_json_snapshot!(chunk);
    }

    // ── WorkOrder from Kimi request ─────────────────────────────────────

    #[test]
    fn work_order_from_request() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage {
                role: "user".into(),
                content: Some("Explain ownership".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(2048),
            temperature: Some(0.5),
            stream: None,
            tools: None,
            use_search: None,
        };
        let wo = convert::request_to_work_order(&req);
        insta::assert_json_snapshot!(wo, { ".id" => "[uuid]" });
    }

    // ── Receipt conversion ──────────────────────────────────────────────

    #[test]
    fn receipt_to_response() {
        let receipt = make_receipt("kimi");
        let resp = convert::receipt_to_response(&receipt, "moonshot-v1-8k");
        insta::assert_json_snapshot!(resp, { ".id" => "[id]" });
    }

    // ── IR representation ───────────────────────────────────────────────

    #[test]
    fn ir_from_kimi_messages() {
        let messages = vec![
            KimiMessage {
                role: "system".into(),
                content: Some("Be concise.".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "user".into(),
                content: Some("What is Rust?".into()),
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        let ir = abp_kimi_sdk::lowering::to_ir(&messages);
        insta::assert_yaml_snapshot!(ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Cross-cutting: AgentEvent serialization (all event types)
// ═══════════════════════════════════════════════════════════════════════════

mod agent_events {
    use super::*;

    #[test]
    fn event_run_started() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "Starting execution".into(),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_run_completed() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunCompleted {
                message: "Done".into(),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_assistant_delta() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "partial ".into(),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_assistant_message() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "Full response text.".into(),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_tool_call() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_001".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_tool_call_nested() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "grep".into(),
                tool_use_id: Some("tu_002".into()),
                parent_tool_use_id: Some("tu_001".into()),
                input: json!({"pattern": "fn main"}),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_tool_result_success() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_001".into()),
                output: json!("fn main() { println!(\"hello\"); }"),
                is_error: false,
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_tool_result_error() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_001".into()),
                output: json!("file not found: missing.rs"),
                is_error: true,
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_file_changed() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Added new function".into(),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_command_executed() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("test result: ok. 42 passed".into()),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_warning() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Warning {
                message: "Approaching token limit".into(),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_error_without_code() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Error {
                message: "Backend timed out".into(),
                error_code: None,
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_error_with_code() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Error {
                message: "Backend crashed".into(),
                error_code: Some(abp_error::ErrorCode::BackendCrashed),
            },
            ext: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn event_with_extensions() {
        let mut ext = BTreeMap::new();
        ext.insert("vendor_id".into(), json!("v-123"));
        ext.insert("latency_ms".into(), json!(42));
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "With extensions".into(),
            },
            ext: Some(ext),
        };
        insta::assert_json_snapshot!(event);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Cross-cutting: Error serialization
// ═══════════════════════════════════════════════════════════════════════════

mod error_codes {
    use abp_error::ErrorCode;

    #[test]
    fn error_code_protocol_variants() {
        let codes = vec![
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ];
        insta::assert_yaml_snapshot!(codes);
    }

    #[test]
    fn error_code_backend_variants() {
        let codes = vec![
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
        ];
        insta::assert_yaml_snapshot!(codes);
    }

    #[test]
    fn error_code_mapping_variants() {
        let codes = vec![
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
        ];
        insta::assert_yaml_snapshot!(codes);
    }

    #[test]
    fn error_code_remaining_variants() {
        let codes = vec![
            ErrorCode::ExecutionToolFailed,
            ErrorCode::ExecutionWorkspaceError,
            ErrorCode::ExecutionPermissionDenied,
            ErrorCode::ContractVersionMismatch,
            ErrorCode::ContractSchemaViolation,
            ErrorCode::ContractInvalidReceipt,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::CapabilityEmulationFailed,
            ErrorCode::PolicyDenied,
            ErrorCode::PolicyInvalid,
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
            ErrorCode::IrInvalid,
            ErrorCode::IrLoweringFailed,
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
            ErrorCode::DialectUnknown,
            ErrorCode::DialectMappingFailed,
            ErrorCode::ConfigInvalid,
            ErrorCode::Internal,
        ];
        insta::assert_yaml_snapshot!(codes);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Cross-cutting: IR types
// ═══════════════════════════════════════════════════════════════════════════

mod ir_types {
    use super::*;

    #[test]
    fn ir_message_text() {
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(msg);
    }

    #[test]
    fn ir_message_tool_use() {
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentBlock::ToolUse {
                id: "tu_01".into(),
                name: "read_file".into(),
                input: json!({"path": "src/lib.rs"}),
            }],
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(msg);
    }

    #[test]
    fn ir_message_tool_result() {
        let msg = IrMessage {
            role: IrRole::Tool,
            content: vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_01".into(),
                content: vec![IrContentBlock::Text {
                    text: "file contents here".into(),
                }],
                is_error: false,
            }],
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(msg);
    }

    #[test]
    fn ir_message_with_metadata() {
        let mut metadata = BTreeMap::new();
        metadata.insert("source_sdk".into(), json!("openai"));
        metadata.insert("model".into(), json!("gpt-4o"));
        let msg = IrMessage {
            role: IrRole::System,
            content: vec![IrContentBlock::Text {
                text: "System prompt".into(),
            }],
            metadata,
        };
        insta::assert_yaml_snapshot!(msg);
    }

    #[test]
    fn ir_content_block_image() {
        let block = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        };
        insta::assert_json_snapshot!(block);
    }

    #[test]
    fn ir_content_block_thinking() {
        let block = IrContentBlock::Thinking {
            text: "Let me think about this step by step...".into(),
        };
        insta::assert_json_snapshot!(block);
    }

    #[test]
    fn ir_tool_definition() {
        let tool = IrToolDefinition {
            name: "get_weather".into(),
            description: "Get weather for a location".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"},
                    "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                },
                "required": ["location"]
            }),
        };
        insta::assert_json_snapshot!(tool);
    }

    #[test]
    fn ir_conversation_multi_turn() {
        let conv = IrConversation {
            messages: vec![
                IrMessage {
                    role: IrRole::System,
                    content: vec![IrContentBlock::Text {
                        text: "You are helpful.".into(),
                    }],
                    metadata: BTreeMap::new(),
                },
                IrMessage {
                    role: IrRole::User,
                    content: vec![IrContentBlock::Text {
                        text: "Hello".into(),
                    }],
                    metadata: BTreeMap::new(),
                },
                IrMessage {
                    role: IrRole::Assistant,
                    content: vec![IrContentBlock::Text {
                        text: "Hi! How can I help?".into(),
                    }],
                    metadata: BTreeMap::new(),
                },
            ],
        };
        insta::assert_yaml_snapshot!(conv);
    }

    #[test]
    fn ir_usage() {
        let usage = IrUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cache_read_tokens: 10,
            cache_write_tokens: 5,
        };
        insta::assert_json_snapshot!(usage);
    }

    #[test]
    fn ir_conversation_with_tool_use() {
        let conv = IrConversation {
            messages: vec![
                IrMessage {
                    role: IrRole::User,
                    content: vec![IrContentBlock::Text {
                        text: "Read main.rs".into(),
                    }],
                    metadata: BTreeMap::new(),
                },
                IrMessage {
                    role: IrRole::Assistant,
                    content: vec![IrContentBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "read_file".into(),
                        input: json!({"path": "src/main.rs"}),
                    }],
                    metadata: BTreeMap::new(),
                },
                IrMessage {
                    role: IrRole::Tool,
                    content: vec![IrContentBlock::ToolResult {
                        tool_use_id: "tu_1".into(),
                        content: vec![IrContentBlock::Text {
                            text: "fn main() {}".into(),
                        }],
                        is_error: false,
                    }],
                    metadata: BTreeMap::new(),
                },
            ],
        };
        insta::assert_yaml_snapshot!(conv);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Cross-cutting: Receipt serialization
// ═══════════════════════════════════════════════════════════════════════════

mod receipts {
    use super::*;
    use abp_core::{
        ArtifactRef, ExecutionMode, Outcome, ReceiptBuilder, VerificationReport,
    };

    #[test]
    fn receipt_complete_yaml() {
        let receipt = make_receipt("test-backend");
        insta::assert_yaml_snapshot!(receipt, {
            ".meta.run_id" => "[uuid]",
            ".meta.started_at" => "[ts]",
            ".meta.finished_at" => "[ts]",
            ".trace[].ts" => "[ts]",
        });
    }

    #[test]
    fn receipt_failed() {
        let ts = fixed_ts();
        let receipt = ReceiptBuilder::new("failing-backend")
            .outcome(Outcome::Failed)
            .work_order_id(fixed_uuid())
            .started_at(ts)
            .finished_at(ts + chrono::Duration::milliseconds(100))
            .add_trace_event(AgentEvent {
                ts,
                kind: AgentEventKind::Error {
                    message: "Backend crashed".into(),
                    error_code: Some(abp_error::ErrorCode::BackendCrashed),
                },
                ext: None,
            })
            .build();
        insta::assert_json_snapshot!(receipt, {
            ".meta.run_id" => "[uuid]",
            ".meta.started_at" => "[ts]",
            ".meta.finished_at" => "[ts]",
            ".trace[].ts" => "[ts]",
        });
    }

    #[test]
    fn receipt_partial_with_artifacts() {
        let ts = fixed_ts();
        let receipt = ReceiptBuilder::new("partial-backend")
            .outcome(Outcome::Partial)
            .work_order_id(fixed_uuid())
            .started_at(ts)
            .finished_at(ts + chrono::Duration::milliseconds(300))
            .add_artifact(ArtifactRef {
                kind: "patch".into(),
                path: "output/changes.patch".into(),
            })
            .add_artifact(ArtifactRef {
                kind: "log".into(),
                path: "output/run.log".into(),
            })
            .verification(VerificationReport {
                git_diff: Some("+fn new_function() {}".into()),
                git_status: Some("M src/lib.rs".into()),
                harness_ok: true,
            })
            .build();
        insta::assert_json_snapshot!(receipt, {
            ".meta.run_id" => "[uuid]",
            ".meta.started_at" => "[ts]",
            ".meta.finished_at" => "[ts]",
        });
    }

    #[test]
    fn receipt_passthrough_mode() {
        let ts = fixed_ts();
        let receipt = ReceiptBuilder::new("passthrough-backend")
            .outcome(Outcome::Complete)
            .work_order_id(fixed_uuid())
            .started_at(ts)
            .finished_at(ts + chrono::Duration::milliseconds(200))
            .mode(ExecutionMode::Passthrough)
            .build();
        insta::assert_json_snapshot!(receipt, {
            ".meta.run_id" => "[uuid]",
            ".meta.started_at" => "[ts]",
            ".meta.finished_at" => "[ts]",
        });
    }
}
