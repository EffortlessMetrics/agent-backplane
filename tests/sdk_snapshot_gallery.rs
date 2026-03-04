#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot gallery for all SDK wire-format types.
//!
//! Each test constructs a realistic, populated instance and snapshots its
//! JSON serialization via `insta::assert_json_snapshot!`.

use std::collections::BTreeMap;

// ── OpenAI ──────────────────────────────────────────────────────────────

mod openai {
    use abp_shim_openai::types::*;
    use serde_json::json;

    #[test]
    fn chat_completion_request_full() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are a helpful assistant.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("What is 2+2?".into()),
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
                    description: "Get the current weather".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" }
                        },
                        "required": ["location"]
                    }),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        };
        insta::assert_json_snapshot!("openai_request_full", req);
    }

    #[test]
    fn chat_completion_request_minimal() {
        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hello".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!("openai_request_minimal", req);
    }

    #[test]
    fn chat_completion_request_multimodal() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Parts(vec![
                    ContentPart::Text {
                        text: "What is in this image?".into(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl {
                            url: "https://example.com/image.png".into(),
                            detail: Some("high".into()),
                        },
                    },
                ]),
            }],
            temperature: None,
            top_p: None,
            max_tokens: Some(512),
            stream: None,
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!("openai_request_multimodal", req);
    }

    #[test]
    fn chat_completion_request_tool_result() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::Assistant {
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_abc123".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "get_weather".into(),
                            arguments: r#"{"location":"London"}"#.into(),
                        },
                    }]),
                },
                ChatMessage::Tool {
                    content: r#"{"temp": 15, "unit": "celsius"}"#.into(),
                    tool_call_id: "call_abc123".into(),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!("openai_request_tool_result", req);
    }

    #[test]
    fn chat_completion_response_full() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-abc123".into(),
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
        insta::assert_json_snapshot!("openai_response_full", resp);
    }

    #[test]
    fn chat_completion_response_with_tool_calls() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-tool456".into(),
            object: "chat.completion".into(),
            created: 1700000100,
            model: "gpt-4o".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_xyz789".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "search_docs".into(),
                            arguments: r#"{"query":"rust async"}"#.into(),
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
        insta::assert_json_snapshot!("openai_response_tool_calls", resp);
    }

    #[test]
    fn stream_chunk_text_delta() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream001".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000200,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("openai_stream_chunk_text", chunk);
    }

    #[test]
    fn stream_chunk_tool_call() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream002".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000300,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: Some("call_stream01".into()),
                        call_type: Some("function".into()),
                        function: Some(StreamFunctionCall {
                            name: Some("get_weather".into()),
                            arguments: Some(r#"{"loc"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("openai_stream_chunk_tool_call", chunk);
    }

    #[test]
    fn stream_chunk_finish() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream003".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000400,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta::default(),
                finish_reason: Some("stop".into()),
            }],
        };
        insta::assert_json_snapshot!("openai_stream_chunk_finish", chunk);
    }

    #[test]
    fn tool_choice_specific_function() {
        let tc = ToolChoice::Function {
            tool_type: "function".into(),
            function: ToolChoiceFunctionRef {
                name: "get_weather".into(),
            },
        };
        insta::assert_json_snapshot!("openai_tool_choice_function", tc);
    }
}

// ── Claude ──────────────────────────────────────────────────────────────

mod claude {
    use abp_shim_claude::types::*;
    use serde_json::json;

    #[test]
    fn messages_request_full() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                ClaudeMessage {
                    role: "user".into(),
                    content: ClaudeContent::Text("Explain quantum computing.".into()),
                },
                ClaudeMessage {
                    role: "assistant".into(),
                    content: ClaudeContent::Text("Quantum computing uses qubits...".into()),
                },
            ],
            max_tokens: 4096,
            system: Some("You are a physics tutor.".into()),
            temperature: Some(0.5),
            top_p: Some(0.95),
            top_k: Some(40),
            stream: Some(false),
            tools: Some(vec![ClaudeTool {
                name: "calculator".into(),
                description: Some("Evaluate math expressions".into()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string" }
                    },
                    "required": ["expression"]
                }),
            }]),
            tool_choice: Some(ClaudeToolChoice::Auto {}),
            stop_sequences: None,
            thinking: None,
        };
        insta::assert_json_snapshot!("claude_request_full", req);
    }

    #[test]
    fn messages_request_minimal() {
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
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        insta::assert_json_snapshot!("claude_request_minimal", req);
    }

    #[test]
    fn messages_request_with_content_blocks() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "What is in this image?".into(),
                    },
                    ContentBlock::Image {
                        source: ImageSource::Base64 {
                            media_type: "image/png".into(),
                            data: "iVBORw0KGgo=".into(),
                        },
                    },
                ]),
            }],
            max_tokens: 2048,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        insta::assert_json_snapshot!("claude_request_content_blocks", req);
    }

    #[test]
    fn messages_request_tool_result() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                ClaudeMessage {
                    role: "assistant".into(),
                    content: ClaudeContent::Blocks(vec![ContentBlock::ToolUse {
                        id: "toolu_01A".into(),
                        name: "calculator".into(),
                        input: json!({"expression": "2+2"}),
                    }]),
                },
                ClaudeMessage {
                    role: "user".into(),
                    content: ClaudeContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "toolu_01A".into(),
                        content: "4".into(),
                        is_error: None,
                    }]),
                },
            ],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        insta::assert_json_snapshot!("claude_request_tool_result", req);
    }

    #[test]
    fn messages_response_full() {
        let resp = MessagesResponse {
            id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
            type_field: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "The answer is 42.".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            usage: ClaudeUsage {
                input_tokens: 100,
                output_tokens: 25,
                cache_creation_input_tokens: Some(50),
                cache_read_input_tokens: Some(30),
            },
        };
        insta::assert_json_snapshot!("claude_response_full", resp);
    }

    #[test]
    fn messages_response_with_tool_use() {
        let resp = MessagesResponse {
            id: "msg_02TOOL".into(),
            type_field: "message".into(),
            role: "assistant".into(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me calculate that.".into(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_calc01".into(),
                    name: "calculator".into(),
                    input: json!({"expression": "sqrt(144)"}),
                },
            ],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("tool_use".into()),
            usage: ClaudeUsage {
                input_tokens: 80,
                output_tokens: 45,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        insta::assert_json_snapshot!("claude_response_tool_use", resp);
    }

    #[test]
    fn stream_event_message_start() {
        let evt = StreamEvent::MessageStart {
            message: MessagesResponse {
                id: "msg_stream01".into(),
                type_field: "message".into(),
                role: "assistant".into(),
                content: vec![],
                model: "claude-sonnet-4-20250514".into(),
                stop_reason: None,
                usage: ClaudeUsage {
                    input_tokens: 50,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
        };
        insta::assert_json_snapshot!("claude_stream_message_start", evt);
    }

    #[test]
    fn stream_event_content_block_delta_text() {
        let evt = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello, ".into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_text_delta", evt);
    }

    #[test]
    fn stream_event_content_block_delta_json() {
        let evt = StreamEvent::ContentBlockDelta {
            index: 1,
            delta: StreamDelta::InputJsonDelta {
                partial_json: r#"{"expr"#.into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_input_json_delta", evt);
    }

    #[test]
    fn stream_event_message_delta() {
        let evt = StreamEvent::MessageDelta {
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
        insta::assert_json_snapshot!("claude_stream_message_delta", evt);
    }

    #[test]
    fn stream_event_ping() {
        let evt = StreamEvent::Ping {};
        insta::assert_json_snapshot!("claude_stream_ping", evt);
    }

    #[test]
    fn tool_choice_specific_tool() {
        let tc = ClaudeToolChoice::Tool {
            name: "calculator".into(),
        };
        insta::assert_json_snapshot!("claude_tool_choice_specific", tc);
    }

    #[test]
    fn tool_choice_any() {
        let tc = ClaudeToolChoice::Any {};
        insta::assert_json_snapshot!("claude_tool_choice_any", tc);
    }
}

// ── Gemini ──────────────────────────────────────────────────────────────

mod gemini {
    use abp_gemini_sdk::types::*;
    use serde_json::json;

    #[test]
    fn generate_content_request_full() {
        let req = GenerateContentRequest {
            contents: vec![
                Content {
                    role: Some("user".into()),
                    parts: vec![Part::Text("Explain photosynthesis.".into())],
                },
                Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text("Photosynthesis is...".into())],
                },
            ],
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part::Text("You are a biology teacher.".into())],
            }),
            tools: Some(vec![GeminiTool {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_diagram".into(),
                    description: "Retrieve a biology diagram".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "topic": { "type": "string" }
                        }
                    }),
                }],
            }]),
            tool_config: Some(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            }),
            generation_config: Some(GenerationConfig {
                temperature: Some(0.8),
                top_p: Some(0.95),
                top_k: Some(40),
                max_output_tokens: Some(2048),
                candidate_count: Some(1),
                stop_sequences: Some(vec!["END".into()]),
            }),
            safety_settings: Some(vec![
                SafetySetting {
                    category: HarmCategory::HarmCategoryHarassment,
                    threshold: HarmBlockThreshold::BlockMediumAndAbove,
                },
                SafetySetting {
                    category: HarmCategory::HarmCategoryHateSpeech,
                    threshold: HarmBlockThreshold::BlockOnlyHigh,
                },
            ]),
        };
        insta::assert_json_snapshot!("gemini_request_full", req);
    }

    #[test]
    fn generate_content_request_minimal() {
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::Text("Hi".into())],
            }],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        insta::assert_json_snapshot!("gemini_request_minimal", req);
    }

    #[test]
    fn generate_content_request_function_call_turn() {
        let req = GenerateContentRequest {
            contents: vec![
                Content {
                    role: Some("model".into()),
                    parts: vec![Part::FunctionCall {
                        name: "get_weather".into(),
                        args: json!({"city": "Tokyo"}),
                    }],
                },
                Content {
                    role: Some("user".into()),
                    parts: vec![Part::FunctionResponse {
                        name: "get_weather".into(),
                        response: json!({"temp": 22, "unit": "celsius"}),
                    }],
                },
            ],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        insta::assert_json_snapshot!("gemini_request_function_call_turn", req);
    }

    #[test]
    fn generate_content_response_full() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text(
                        "Photosynthesis converts light energy into chemical energy.".into(),
                    )],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: Some(vec![SafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Negligible,
                }]),
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 30,
                candidates_token_count: 15,
                total_token_count: 45,
            }),
            prompt_feedback: None,
        };
        insta::assert_json_snapshot!("gemini_response_full", resp);
    }

    #[test]
    fn generate_content_response_with_function_call() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::FunctionCall {
                        name: "get_diagram".into(),
                        args: json!({"topic": "chloroplast"}),
                    }],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 40,
                candidates_token_count: 12,
                total_token_count: 52,
            }),
            prompt_feedback: None,
        };
        insta::assert_json_snapshot!("gemini_response_function_call", resp);
    }

    #[test]
    fn generate_content_response_blocked() {
        let resp = GenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
            prompt_feedback: Some(PromptFeedback {
                block_reason: Some("SAFETY".into()),
                safety_ratings: Some(vec![SafetyRating {
                    category: HarmCategory::HarmCategoryDangerousContent,
                    probability: HarmProbability::High,
                }]),
            }),
        };
        insta::assert_json_snapshot!("gemini_response_blocked", resp);
    }

    #[test]
    fn tool_config_restricted() {
        let tc = ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Any,
                allowed_function_names: Some(vec!["get_weather".into(), "get_time".into()]),
            },
        };
        insta::assert_json_snapshot!("gemini_tool_config_restricted", tc);
    }

    #[test]
    fn content_with_inline_data() {
        let content = Content {
            role: Some("user".into()),
            parts: vec![
                Part::Text("Describe this image.".into()),
                Part::InlineData {
                    mime_type: "image/jpeg".into(),
                    data: "/9j/4AAQ==".into(),
                },
            ],
        };
        insta::assert_json_snapshot!("gemini_content_inline_data", content);
    }
}

// ── Codex ───────────────────────────────────────────────────────────────

mod codex {
    use abp_codex_sdk::types::*;
    use serde_json::json;

    #[test]
    fn codex_request_full() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![
                CodexMessage::System {
                    content: "You are a coding assistant.".into(),
                },
                CodexMessage::User {
                    content: "Refactor this function to use async/await.".into(),
                },
            ],
            instructions: Some("Follow Rust best practices.".into()),
            temperature: Some(0.3),
            top_p: Some(0.9),
            max_tokens: Some(4096),
            stream: Some(false),
            tools: Some(vec![CodexTool {
                tool_type: "function".into(),
                function: CodexFunctionDef {
                    name: "apply_edit".into(),
                    description: "Apply a code edit to a file".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }),
                },
            }]),
            tool_choice: Some(CodexToolChoice::Mode(CodexToolChoiceMode::Auto)),
        };
        insta::assert_json_snapshot!("codex_request_full", req);
    }

    #[test]
    fn codex_request_minimal() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![CodexMessage::User {
                content: "Hello".into(),
            }],
            instructions: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!("codex_request_minimal", req);
    }

    #[test]
    fn codex_response_full() {
        let resp = CodexResponse {
            id: "codex-resp-001".into(),
            object: "chat.completion".into(),
            created: 1700001000,
            model: "codex-mini-latest".into(),
            choices: vec![CodexChoice {
                index: 0,
                message: CodexChoiceMessage {
                    role: "assistant".into(),
                    content: Some("Here is the refactored code...".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(CodexUsage {
                prompt_tokens: 120,
                completion_tokens: 250,
                total_tokens: 370,
            }),
        };
        insta::assert_json_snapshot!("codex_response_full", resp);
    }

    #[test]
    fn codex_response_with_tool_calls() {
        let resp = CodexResponse {
            id: "codex-resp-002".into(),
            object: "chat.completion".into(),
            created: 1700001100,
            model: "codex-mini-latest".into(),
            choices: vec![CodexChoice {
                index: 0,
                message: CodexChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![CodexToolCall {
                        id: "call_codex01".into(),
                        call_type: "function".into(),
                        function: CodexFunctionCall {
                            name: "apply_edit".into(),
                            arguments: r#"{"path":"src/main.rs","content":"fn main() {}"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(CodexUsage {
                prompt_tokens: 80,
                completion_tokens: 30,
                total_tokens: 110,
            }),
        };
        insta::assert_json_snapshot!("codex_response_tool_calls", resp);
    }

    #[test]
    fn codex_file_change_create() {
        let fc = CodexFileChange {
            path: "src/lib.rs".into(),
            operation: FileOperation::Create,
            content: Some("pub fn hello() -> &'static str { \"hi\" }".into()),
            diff: None,
        };
        insta::assert_json_snapshot!("codex_file_change_create", fc);
    }

    #[test]
    fn codex_file_change_patch() {
        let fc = CodexFileChange {
            path: "src/main.rs".into(),
            operation: FileOperation::Patch,
            content: None,
            diff: Some("@@ -1,3 +1,3 @@\n-fn main() {\n+fn main() -> Result<()> {".into()),
        };
        insta::assert_json_snapshot!("codex_file_change_patch", fc);
    }

    #[test]
    fn codex_file_change_delete() {
        let fc = CodexFileChange {
            path: "tmp/scratch.txt".into(),
            operation: FileOperation::Delete,
            content: None,
            diff: None,
        };
        insta::assert_json_snapshot!("codex_file_change_delete", fc);
    }

    #[test]
    fn codex_command_full() {
        let cmd = CodexCommand {
            command: "cargo test".into(),
            cwd: Some("src/".into()),
            timeout_seconds: Some(120),
            stdout: Some("test result: ok. 42 passed\n".into()),
            stderr: Some("".into()),
            exit_code: Some(0),
        };
        insta::assert_json_snapshot!("codex_command_full", cmd);
    }

    #[test]
    fn codex_stream_chunk_text() {
        let chunk = CodexStreamChunk {
            id: "codex-stream-001".into(),
            object: "chat.completion.chunk".into(),
            created: 1700001200,
            model: "codex-mini-latest".into(),
            choices: vec![CodexStreamChoice {
                index: 0,
                delta: CodexStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Here".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("codex_stream_chunk_text", chunk);
    }

    #[test]
    fn codex_tool_choice_function() {
        let tc = CodexToolChoice::Function {
            tool_type: "function".into(),
            function: CodexToolChoiceFunctionRef {
                name: "apply_edit".into(),
            },
        };
        insta::assert_json_snapshot!("codex_tool_choice_function", tc);
    }
}

// ── Kimi ────────────────────────────────────────────────────────────────

mod kimi {
    use abp_kimi_sdk::types::*;
    use serde_json::json;

    #[test]
    fn kimi_chat_request_full() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are a helpful assistant with web search.".into(),
                },
                ChatMessage::User {
                    content: "What happened in the news today?".into(),
                },
            ],
            temperature: Some(0.6),
            top_p: Some(0.85),
            max_tokens: Some(2048),
            stream: Some(false),
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDef {
                    name: "web_search".into(),
                    description: "Search the web for information".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        },
                        "required": ["query"]
                    }),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
            use_search: Some(true),
            search_options: Some(SearchOptions {
                mode: SearchMode::Auto,
                result_count: Some(5),
            }),
        };
        insta::assert_json_snapshot!("kimi_request_full", req);
    }

    #[test]
    fn kimi_chat_request_minimal() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![ChatMessage::User {
                content: "Hello".into(),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            use_search: None,
            search_options: None,
        };
        insta::assert_json_snapshot!("kimi_request_minimal", req);
    }

    #[test]
    fn kimi_chat_response_full() {
        let resp = KimiChatResponse {
            id: "kimi-resp-001".into(),
            object: "chat.completion".into(),
            created: 1700002000,
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: Some("Based on my search results...".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 60,
                completion_tokens: 150,
                total_tokens: 210,
                search_tokens: Some(45),
            }),
        };
        insta::assert_json_snapshot!("kimi_response_full", resp);
    }

    #[test]
    fn kimi_chat_response_with_tool_calls() {
        let resp = KimiChatResponse {
            id: "kimi-resp-002".into(),
            object: "chat.completion".into(),
            created: 1700002100,
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_kimi01".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "web_search".into(),
                            arguments: r#"{"query":"latest news today"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 40,
                completion_tokens: 20,
                total_tokens: 60,
                search_tokens: None,
            }),
        };
        insta::assert_json_snapshot!("kimi_response_tool_calls", resp);
    }

    #[test]
    fn kimi_stream_chunk_text() {
        let chunk = KimiStreamChunk {
            id: "kimi-stream-001".into(),
            object: "chat.completion.chunk".into(),
            created: 1700002200,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: Some("Based on".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        insta::assert_json_snapshot!("kimi_stream_chunk_text", chunk);
    }

    #[test]
    fn kimi_stream_chunk_final() {
        let chunk = KimiStreamChunk {
            id: "kimi-stream-002".into(),
            object: "chat.completion.chunk".into(),
            created: 1700002300,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta::default(),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 60,
                completion_tokens: 80,
                total_tokens: 140,
                search_tokens: Some(20),
            }),
        };
        insta::assert_json_snapshot!("kimi_stream_chunk_final", chunk);
    }

    #[test]
    fn kimi_search_options_always() {
        let opts = SearchOptions {
            mode: SearchMode::Always,
            result_count: Some(10),
        };
        insta::assert_json_snapshot!("kimi_search_options_always", opts);
    }
}

// ── Copilot ─────────────────────────────────────────────────────────────

mod copilot {
    use super::*;
    use abp_copilot_sdk::types::*;
    use serde_json::json;

    #[test]
    fn copilot_chat_request_full() {
        let mut meta = BTreeMap::new();
        meta.insert("language".into(), json!("rust"));
        meta.insert("startLine".into(), json!(10));

        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotChatMessage {
                    role: "system".into(),
                    content: Some("You are a coding assistant.".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                CopilotChatMessage {
                    role: "user".into(),
                    content: Some("Explain this code.".into()),
                    name: Some("developer".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: Some(0.4),
            top_p: Some(0.9),
            max_tokens: Some(2048),
            stream: Some(true),
            tools: Some(vec![CopilotTool {
                tool_type: "function".into(),
                function: CopilotToolFunction {
                    name: "read_file".into(),
                    description: "Read file contents".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }),
                },
            }]),
            tool_choice: Some(json!("auto")),
            intent: Some("code-review".into()),
            references: Some(vec![
                Reference {
                    ref_type: ReferenceType::File,
                    id: "ref-001".into(),
                    uri: Some("file:///workspace/src/main.rs".into()),
                    content: Some("fn main() {}".into()),
                    metadata: Some(meta),
                },
                Reference {
                    ref_type: ReferenceType::Selection,
                    id: "ref-002".into(),
                    uri: None,
                    content: Some("let x = 42;".into()),
                    metadata: None,
                },
            ]),
        };
        insta::assert_json_snapshot!("copilot_request_full", req);
    }

    #[test]
    fn copilot_chat_request_minimal() {
        let req = CopilotChatRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![CopilotChatMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            intent: None,
            references: None,
        };
        insta::assert_json_snapshot!("copilot_request_minimal", req);
    }

    #[test]
    fn copilot_chat_response_full() {
        let resp = CopilotChatResponse {
            id: "copilot-resp-001".into(),
            object: "chat.completion".into(),
            created: 1700003000,
            model: "gpt-4o".into(),
            choices: vec![CopilotChatChoice {
                index: 0,
                message: CopilotChatChoiceMessage {
                    role: "assistant".into(),
                    content: Some("This code defines the main entry point of the program.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(CopilotUsage {
                prompt_tokens: 200,
                completion_tokens: 50,
                total_tokens: 250,
                copilot_tokens: Some(15),
            }),
        };
        insta::assert_json_snapshot!("copilot_response_full", resp);
    }

    #[test]
    fn copilot_chat_response_with_tool_calls() {
        let resp = CopilotChatResponse {
            id: "copilot-resp-002".into(),
            object: "chat.completion".into(),
            created: 1700003100,
            model: "gpt-4o".into(),
            choices: vec![CopilotChatChoice {
                index: 0,
                message: CopilotChatChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![CopilotToolCall {
                        id: "call_copilot01".into(),
                        call_type: "function".into(),
                        function: CopilotFunctionCall {
                            name: "read_file".into(),
                            arguments: r#"{"path":"src/lib.rs"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(CopilotUsage {
                prompt_tokens: 100,
                completion_tokens: 25,
                total_tokens: 125,
                copilot_tokens: None,
            }),
        };
        insta::assert_json_snapshot!("copilot_response_tool_calls", resp);
    }

    #[test]
    fn copilot_stream_chunk_text() {
        let chunk = CopilotStreamChunk {
            id: "copilot-stream-001".into(),
            object: "chat.completion.chunk".into(),
            created: 1700003200,
            model: "gpt-4o".into(),
            choices: vec![CopilotStreamChoice {
                index: 0,
                delta: CopilotStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("This".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("copilot_stream_chunk_text", chunk);
    }

    #[test]
    fn copilot_stream_chunk_tool_call() {
        let chunk = CopilotStreamChunk {
            id: "copilot-stream-002".into(),
            object: "chat.completion.chunk".into(),
            created: 1700003300,
            model: "gpt-4o".into(),
            choices: vec![CopilotStreamChoice {
                index: 0,
                delta: CopilotStreamDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![CopilotStreamToolCall {
                        index: 0,
                        id: Some("call_stream_cp01".into()),
                        call_type: Some("function".into()),
                        function: Some(CopilotStreamFunctionCall {
                            name: Some("read_file".into()),
                            arguments: Some(r#"{"path"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("copilot_stream_chunk_tool_call", chunk);
    }

    #[test]
    fn copilot_reference_git_diff() {
        let reference = Reference {
            ref_type: ReferenceType::GitDiff,
            id: "diff-001".into(),
            uri: Some("git://HEAD~1..HEAD".into()),
            content: Some("@@ -1,3 +1,4 @@\n+use std::io;\n fn main() {}".into()),
            metadata: None,
        };
        insta::assert_json_snapshot!("copilot_reference_git_diff", reference);
    }

    #[test]
    fn copilot_reference_terminal() {
        let reference = Reference {
            ref_type: ReferenceType::Terminal,
            id: "term-001".into(),
            uri: None,
            content: Some("error[E0308]: mismatched types".into()),
            metadata: None,
        };
        insta::assert_json_snapshot!("copilot_reference_terminal", reference);
    }
}
