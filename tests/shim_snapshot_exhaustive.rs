#![allow(dead_code, unused_imports)]
#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive snapshot tests for all SDK shim request/response types.
//!
//! Covers OpenAI, Claude, Gemini, Codex, Copilot, and Kimi shims with 50+
//! tests to ensure JSON schema stability across releases.

use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI shim snapshots (10)
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::types::*;

    #[test]
    fn openai_chat_completion_request_minimal() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
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
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn openai_chat_completion_request_full() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are a helpful assistant.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Parts(vec![
                        ContentPart::Text {
                            text: "Describe this image".into(),
                        },
                        ContentPart::ImageUrl {
                            image_url: ImageUrl {
                                url: "https://example.com/img.png".into(),
                                detail: Some("high".into()),
                            },
                        },
                    ]),
                },
                ChatMessage::Assistant {
                    content: Some("I see a landscape.".into()),
                    tool_calls: None,
                },
            ],
            temperature: Some(0.7),
            top_p: Some(0.95),
            max_tokens: Some(2048),
            stream: Some(false),
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDef {
                    name: "get_weather".into(),
                    description: "Get current weather".into(),
                    parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn openai_chat_completion_response() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-snap001".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o-2024-05-13".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: Some("The answer is 42.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 30,
                completion_tokens: 10,
                total_tokens: 40,
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn openai_response_with_tool_calls() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-snap002".into(),
            object: "chat.completion".into(),
            created: 1700000100,
            model: "gpt-4o".into(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_abc".into(),
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
                completion_tokens: 25,
                total_tokens: 75,
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn openai_stream_chunk_text() {
        let chunk = StreamChunk {
            id: "chatcmpl-snap003".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000200,
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
    fn openai_stream_chunk_tool_call() {
        let chunk = StreamChunk {
            id: "chatcmpl-snap004".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000300,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: Some("call_xyz".into()),
                        call_type: Some("function".into()),
                        function: Some(StreamFunctionCall {
                            name: Some("read_file".into()),
                            arguments: Some(r#"{"path":"src/main.rs"}"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!(chunk);
    }

    #[test]
    fn openai_stream_chunk_finish() {
        let chunk = StreamChunk {
            id: "chatcmpl-snap005".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000400,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
        };
        insta::assert_json_snapshot!(chunk);
    }

    #[test]
    fn openai_error_response() {
        let err = ErrorResponse {
            error: ErrorDetail {
                message: "Rate limit exceeded".into(),
                error_type: "rate_limit_error".into(),
                param: None,
                code: Some("rate_limit".into()),
            },
        };
        insta::assert_json_snapshot!(err);
    }

    #[test]
    fn openai_embedding_request() {
        let req = EmbeddingRequest {
            model: "text-embedding-3-small".into(),
            input: EmbeddingInput::Multiple(vec!["Hello world".into(), "Rust programming".into()]),
            encoding_format: Some("float".into()),
            dimensions: Some(1536),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn openai_embedding_response() {
        let resp = EmbeddingResponse {
            object: "list".into(),
            data: vec![
                EmbeddingData {
                    object: "embedding".into(),
                    index: 0,
                    embedding: vec![0.1, 0.2, 0.3],
                },
                EmbeddingData {
                    object: "embedding".into(),
                    index: 1,
                    embedding: vec![0.4, 0.5, 0.6],
                },
            ],
            model: "text-embedding-3-small".into(),
            usage: EmbeddingUsage {
                prompt_tokens: 8,
                total_tokens: 8,
            },
        };
        insta::assert_json_snapshot!(resp);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Claude shim snapshots (10)
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::types::*;

    #[test]
    fn claude_messages_request_minimal() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hello, Claude!".into()),
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
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn claude_messages_request_full() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                ClaudeMessage {
                    role: "user".into(),
                    content: ClaudeContent::Blocks(vec![
                        ContentBlock::Text {
                            text: "Analyze this code".into(),
                        },
                        ContentBlock::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".into(),
                                data: "iVBORw0KGgo=".into(),
                            },
                        },
                    ]),
                },
                ClaudeMessage {
                    role: "assistant".into(),
                    content: ClaudeContent::Text("I see code.".into()),
                },
            ],
            max_tokens: 4096,
            system: Some("You are a code reviewer.".into()),
            temperature: Some(0.3),
            top_p: Some(0.9),
            top_k: Some(40),
            stream: Some(true),
            stop_sequences: Some(vec!["STOP".into()]),
            tools: Some(vec![ClaudeTool {
                name: "run_tests".into(),
                description: Some("Run test suite".into()),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }]),
            tool_choice: Some(ClaudeToolChoice::Auto {}),
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".into(),
                budget_tokens: 8192,
            }),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn claude_messages_response() {
        let resp = MessagesResponse {
            id: "msg_snap001".into(),
            type_field: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Here is my analysis.".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            usage: ClaudeUsage {
                input_tokens: 50,
                output_tokens: 100,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn claude_content_block_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "toolu_snap001".into(),
            name: "search_code".into(),
            input: json!({"query": "async fn main", "path": "src/"}),
        };
        insta::assert_json_snapshot!(block);
    }

    #[test]
    fn claude_content_block_tool_result() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_snap001".into(),
            content: "Found 3 matches in src/main.rs".into(),
            is_error: Some(false),
        };
        insta::assert_json_snapshot!(block);
    }

    #[test]
    fn claude_content_block_thinking() {
        let block = ContentBlock::Thinking {
            thinking: "Let me analyze the code structure...".into(),
            signature: Some("sig_abc123".into()),
        };
        insta::assert_json_snapshot!(block);
    }

    #[test]
    fn claude_stream_message_start() {
        let event = StreamEvent::MessageStart {
            message: MessagesResponse {
                id: "msg_snap002".into(),
                type_field: "message".into(),
                role: "assistant".into(),
                content: vec![],
                model: "claude-sonnet-4-20250514".into(),
                stop_reason: None,
                usage: ClaudeUsage {
                    input_tokens: 25,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn claude_stream_text_delta() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello, ".into(),
            },
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn claude_stream_message_delta() {
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
    fn claude_error_response() {
        let err = ErrorResponse {
            error_type: "invalid_request_error".into(),
            message: "max_tokens must be greater than 0".into(),
        };
        insta::assert_json_snapshot!(err);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Gemini shim snapshots (10)
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::types::*;
    use abp_shim_gemini::{FunctionCallingMode, HarmBlockThreshold, HarmCategory};

    #[test]
    fn gemini_generate_content_request_minimal() {
        let req = GenerateContentRequest {
            model: "gemini-2.0-flash".into(),
            contents: vec![Content {
                role: "user".into(),
                parts: vec![Part::Text("Hello Gemini".into())],
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
    fn gemini_generate_content_request_full() {
        let req = GenerateContentRequest {
            model: "gemini-2.0-flash".into(),
            contents: vec![
                Content {
                    role: "user".into(),
                    parts: vec![
                        Part::Text("What is this?".into()),
                        Part::InlineData {
                            mime_type: "image/jpeg".into(),
                            data: "/9j/4AAQ==".into(),
                        },
                    ],
                },
                Content {
                    role: "model".into(),
                    parts: vec![Part::Text("It appears to be a photo.".into())],
                },
            ],
            system_instruction: Some(Content {
                role: "system".into(),
                parts: vec![Part::Text("You are a visual analyst.".into())],
            }),
            generation_config: Some(GenerationConfig {
                max_output_tokens: Some(2048),
                temperature: Some(0.5),
                top_p: Some(0.9),
                top_k: Some(40),
                candidate_count: Some(1),
                stop_sequences: Some(vec!["END".into()]),
                response_mime_type: None,
                response_schema: None,
            }),
            safety_settings: Some(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            }]),
            tools: Some(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: "Get weather for a city".into(),
                    parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
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
    fn gemini_generate_content_response() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".into(),
                    parts: vec![Part::Text("The weather is sunny.".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: Some(vec![SafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Negligible,
                }]),
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 20,
                candidates_token_count: 15,
                total_token_count: 35,
            }),
            prompt_feedback: None,
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn gemini_function_call_part() {
        let content = Content {
            role: "model".into(),
            parts: vec![Part::FunctionCall {
                name: "search_docs".into(),
                args: json!({"query": "rust tokio", "limit": 10}),
            }],
        };
        insta::assert_json_snapshot!(content);
    }

    #[test]
    fn gemini_function_response_part() {
        let content = Content {
            role: "user".into(),
            parts: vec![Part::FunctionResponse {
                name: "search_docs".into(),
                response: json!({"results": [{"title": "Tokio Tutorial", "url": "https://tokio.rs"}]}),
            }],
        };
        insta::assert_json_snapshot!(content);
    }

    #[test]
    fn gemini_function_declarations() {
        let tool = ToolDeclaration {
            function_declarations: vec![
                FunctionDeclaration {
                    name: "read_file".into(),
                    description: "Read a file from disk".into(),
                    parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                },
                FunctionDeclaration {
                    name: "write_file".into(),
                    description: "Write content to a file".into(),
                    parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]}),
                },
            ],
        };
        insta::assert_json_snapshot!(tool);
    }

    #[test]
    fn gemini_safety_settings_all_categories() {
        let settings = vec![
            SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryHateSpeech,
                threshold: HarmBlockThreshold::BlockLowAndAbove,
            },
            SafetySetting {
                category: HarmCategory::HarmCategorySexuallyExplicit,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryDangerousContent,
                threshold: HarmBlockThreshold::BlockOnlyHigh,
            },
        ];
        insta::assert_json_snapshot!(settings);
    }

    #[test]
    fn gemini_stream_event() {
        let event = StreamEvent {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".into(),
                    parts: vec![Part::Text("Streaming...".into())],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn gemini_prompt_feedback_blocked() {
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
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn gemini_error_response() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 429,
                message: "Resource exhausted".into(),
                status: Some("RESOURCE_EXHAUSTED".into()),
            },
        };
        insta::assert_json_snapshot!(err);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Codex shim snapshots (10)
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::CodexInputItem;
    use abp_shim_codex::tools::{FileSearchRankingOptions, FunctionTool, ToolDefinition};
    use abp_shim_codex::types::*;
    use abp_shim_codex::{ApiErrorBody, ApiErrorEnvelope, CodexTextFormat};

    #[test]
    fn codex_response_simple() {
        let resp = Response {
            id: "resp_snap001".into(),
            object: "response".into(),
            status: Some("completed".into()),
            model: "codex-mini-latest".into(),
            output: vec![ResponseItem::Message {
                role: "assistant".into(),
                content: vec![ResponseContentPart::OutputText {
                    text: "Hello from Codex!".into(),
                }],
            }],
            usage: Some(Usage {
                input_tokens: 20,
                output_tokens: 10,
                total_tokens: 30,
            }),
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn codex_response_function_call() {
        let resp = Response {
            id: "resp_snap002".into(),
            object: "response".into(),
            status: Some("completed".into()),
            model: "codex-mini-latest".into(),
            output: vec![ResponseItem::FunctionCall {
                id: "fc_001".into(),
                call_id: Some("call_abc".into()),
                name: "edit_file".into(),
                arguments: r#"{"path":"src/main.rs","content":"fn main() {}"}"#.into(),
            }],
            usage: Some(Usage {
                input_tokens: 50,
                output_tokens: 30,
                total_tokens: 80,
            }),
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn codex_response_item_variants() {
        let items: Vec<ResponseItem> = vec![
            ResponseItem::Message {
                role: "assistant".into(),
                content: vec![ResponseContentPart::OutputText {
                    text: "Result text".into(),
                }],
            },
            ResponseItem::FunctionCall {
                id: "fc_002".into(),
                call_id: None,
                name: "run_cmd".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call_002".into(),
                output: "file1.txt\nfile2.txt".into(),
            },
            ResponseItem::FileSearchCall {
                id: "fs_001".into(),
                queries: vec!["error handling".into()],
                results: vec![FileSearchResult {
                    file_id: "file_abc".into(),
                    file_name: Some("error.rs".into()),
                    score: Some(0.95),
                    text: Some("impl Error for...".into()),
                }],
            },
            ResponseItem::CodeInterpreterCall {
                id: "ci_001".into(),
                code: Some("print('hello')".into()),
                outputs: vec![CodeInterpreterOutput::Logs {
                    logs: "hello".into(),
                }],
            },
            ResponseItem::Reasoning {
                summary: vec![ReasoningSummaryPart {
                    text: "I need to analyze the error handling patterns.".into(),
                }],
            },
        ];
        insta::assert_json_snapshot!(items);
    }

    #[test]
    fn codex_response_config() {
        let config = ResponseConfig {
            instructions: Some("You are a coding assistant.".into()),
            model: "codex-mini-latest".into(),
            tools: vec![ToolDefinition::Function(FunctionTool {
                name: "read_file".into(),
                description: Some("Read a file".into()),
                parameters: Some(
                    json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                ),
                strict: true,
            })],
            temperature: Some(0.2),
            max_output_tokens: Some(4096),
            top_p: None,
            reasoning: Some(ReasoningConfig {
                effort: ReasoningEffort::High,
                summary: true,
            }),
            text: None,
            stream: false,
            previous_response_id: None,
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(config);
    }

    #[test]
    fn codex_tool_definitions() {
        let tools: Vec<ToolDefinition> = vec![
            ToolDefinition::Function(FunctionTool {
                name: "bash".into(),
                description: Some("Execute a bash command".into()),
                parameters: Some(
                    json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
                ),
                strict: false,
            }),
            ToolDefinition::FileSearch(abp_shim_codex::FileSearchTool {
                vector_store_ids: vec!["vs_001".into()],
                max_num_results: Some(5),
                ranking_options: Some(FileSearchRankingOptions {
                    ranker: Some("auto".into()),
                    score_threshold: Some(0.7),
                }),
            }),
            ToolDefinition::CodeInterpreter(abp_shim_codex::CodeInterpreterTool {
                container: Some("python:3.12".into()),
                allowed_extensions: vec!["py".into(), "txt".into()],
            }),
        ];
        insta::assert_json_snapshot!(tools);
    }

    #[test]
    fn codex_extended_request() {
        let req = CodexExtendedRequest {
            model: "codex-mini-latest".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Fix the bug in main.rs".into(),
            }],
            instructions: Some("You are a Rust expert.".into()),
            context: vec![CodexContextItem {
                path: "src/main.rs".into(),
                content: Some("fn main() { panic!() }".into()),
            }],
            max_output_tokens: Some(2048),
            temperature: Some(0.1),
            tools: vec![],
            text: None,
            sandbox: Some(CodexSandboxConfig {
                container_image: Some("rust:1.85".into()),
                network_enabled: false,
                timeout_seconds: Some(120),
                memory_mb: Some(1024),
                env: BTreeMap::new(),
            }),
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn codex_extended_response() {
        let resp = CodexExtendedResponse {
            id: "resp_snap003".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
            }),
            status: Some("completed".into()),
            sandbox_result: Some(CodexSandboxResult {
                exit_code: Some(0),
                duration_ms: Some(2500),
                files_modified: vec!["src/main.rs".into()],
            }),
            metadata: BTreeMap::new(),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn codex_stream_events() {
        let events: Vec<CodexShimStreamEvent> = vec![
            CodexShimStreamEvent::ResponseCreated {
                sequence: 0,
                response_id: "resp_snap004".into(),
                model: "codex-mini-latest".into(),
            },
            CodexShimStreamEvent::TextDelta {
                sequence: 1,
                output_index: 0,
                text: "Here is the fix".into(),
            },
            CodexShimStreamEvent::FunctionCallDelta {
                sequence: 2,
                output_index: 1,
                delta: r#"{"path":"src/"#.into(),
            },
            CodexShimStreamEvent::ResponseCompleted {
                sequence: 3,
                response_id: "resp_snap004".into(),
                usage: Some(Usage {
                    input_tokens: 40,
                    output_tokens: 20,
                    total_tokens: 60,
                }),
            },
        ];
        insta::assert_json_snapshot!(events);
    }

    #[test]
    fn codex_error_response() {
        let err = ApiErrorEnvelope {
            error: ApiErrorBody {
                message: "Invalid model specified".into(),
                error_type: "invalid_request_error".into(),
                code: Some("model_not_found".into()),
                param: Some("model".into()),
            },
        };
        insta::assert_json_snapshot!(err);
    }

    #[test]
    fn codex_content_part_refusal() {
        let part = ResponseContentPart::Refusal {
            refusal: "I cannot help with that request.".into(),
        };
        insta::assert_json_snapshot!(part);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Copilot shim snapshots (5)
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::{
        CopilotConfirmation, CopilotError, CopilotFunctionCall, CopilotMessage, CopilotReference,
        CopilotReferenceType, CopilotTool, CopilotTurnEntry,
    };
    use abp_shim_copilot::types::*;
    use abp_shim_copilot::{CopilotFunctionDef, CopilotToolType};

    #[test]
    fn copilot_chat_request_full() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotMessage {
                    role: "system".into(),
                    content: "You are a Copilot agent.".into(),
                    name: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "user".into(),
                    content: "Explain this code".into(),
                    name: Some("developer".into()),
                    copilot_references: vec![CopilotReference {
                        ref_type: CopilotReferenceType::File,
                        id: "ref_001".into(),
                        data: json!({"path": "src/lib.rs", "content": "pub fn hello() {}"}),
                        metadata: None,
                    }],
                },
            ],
            tools: Some(vec![CopilotTool {
                tool_type: CopilotToolType::Function,
                function: Some(CopilotFunctionDef {
                    name: "run_tests".into(),
                    description: "Run the test suite".into(),
                    parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                }),
                confirmation: None,
            }]),
            intent: Some(CopilotIntent::Explain),
            doc_context: Some(CopilotDocContext {
                uri: "file:///workspace/src/lib.rs".into(),
                language: Some("rust".into()),
                cursor_line: Some(10),
                cursor_column: Some(4),
                selection: Some(SelectionRange {
                    start_line: 10,
                    start_column: 0,
                    end_line: 15,
                    end_column: 1,
                    text: Some("pub fn hello() {}".into()),
                }),
                content: None,
            }),
            references: vec![CopilotCodeReference {
                path: "src/lib.rs".into(),
                language: Some("rust".into()),
                selection: None,
                content: Some("pub fn hello() {}".into()),
            }],
            skills: vec![CopilotSkill {
                id: "skill_test".into(),
                name: "Test Generator".into(),
                description: Some("Generate unit tests".into()),
                parameters_schema: Some(
                    json!({"type": "object", "properties": {"style": {"type": "string"}}}),
                ),
            }],
            turn_history: vec![CopilotTurnEntry {
                request: "What does this function do?".into(),
                response: "It prints hello world.".into(),
            }],
            temperature: Some(0.3),
            max_tokens: Some(2048),
            stream: Some(true),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn copilot_chat_response() {
        let resp = CopilotChatResponse {
            message: "Here is an explanation of the code.".into(),
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::Snippet,
                id: "ref_002".into(),
                data: json!({"uri": "src/lib.rs", "range": {"start": 10, "end": 15}}),
                metadata: Some({
                    let mut m = BTreeMap::new();
                    m.insert("label".into(), json!("hello function"));
                    m
                }),
            }],
            copilot_errors: vec![],
            function_call: None,
            metadata: Some(CopilotResponseMetadata {
                intent: Some(CopilotIntent::Explain),
                model: Some("gpt-4o".into()),
                ext: BTreeMap::new(),
            }),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn copilot_references_all_types() {
        let refs = vec![
            CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "ref_file".into(),
                data: json!({"path": "Cargo.toml"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::Snippet,
                id: "ref_snippet".into(),
                data: json!({"uri": "src/main.rs", "text": "fn main() {}"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::Repository,
                id: "ref_repo".into(),
                data: json!({"owner": "example", "name": "repo"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::WebSearchResult,
                id: "ref_web".into(),
                data: json!({"url": "https://docs.rs", "title": "Docs.rs"}),
                metadata: None,
            },
        ];
        insta::assert_json_snapshot!(refs);
    }

    #[test]
    fn copilot_confirmations() {
        let confirmations = vec![
            CopilotConfirmation {
                id: "conf_001".into(),
                title: "Delete file".into(),
                message: "Are you sure you want to delete src/old.rs?".into(),
                accepted: None,
            },
            CopilotConfirmation {
                id: "conf_002".into(),
                title: "Run command".into(),
                message: "Execute `cargo build`?".into(),
                accepted: Some(true),
            },
        ];
        insta::assert_json_snapshot!(confirmations);
    }

    #[test]
    fn copilot_stream_events() {
        let events: Vec<CopilotLocalStreamEvent> = vec![
            CopilotLocalStreamEvent::CopilotReferences {
                references: vec![CopilotReference {
                    ref_type: CopilotReferenceType::File,
                    id: "ref_stream".into(),
                    data: json!({"path": "README.md"}),
                    metadata: None,
                }],
            },
            CopilotLocalStreamEvent::TextDelta {
                text: "Here is the ".into(),
            },
            CopilotLocalStreamEvent::FunctionCall {
                function_call: CopilotFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"src/lib.rs"}"#.into(),
                    id: Some("call_cp01".into()),
                },
            },
            CopilotLocalStreamEvent::CopilotErrors {
                errors: vec![CopilotError {
                    error_type: "tool_error".into(),
                    message: "File not found".into(),
                    code: Some("not_found".into()),
                    identifier: Some("err_001".into()),
                }],
            },
            CopilotLocalStreamEvent::Metadata {
                metadata: CopilotResponseMetadata {
                    intent: Some(CopilotIntent::Fix),
                    model: None,
                    ext: BTreeMap::new(),
                },
            },
            CopilotLocalStreamEvent::Done {},
        ];
        insta::assert_json_snapshot!(events);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Kimi shim snapshots (5)
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiFunctionDef, KimiToolCall};
    use abp_shim_kimi::types::*;
    use abp_shim_kimi::{KimiErrorBody, KimiErrorResponse, SearchTool};

    #[test]
    fn kimi_chat_request_full() {
        let req = KimiChatRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![
                Message::system("You are a helpful assistant."),
                Message::user("Summarize this document"),
            ],
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(4096),
            stream: Some(false),
            use_search: Some(true),
            ref_file_ids: Some(vec!["file-abc123".into()]),
            plugin_ids: Some(vec!["plugin-search".into()]),
            plugins: Some(vec![KimiPluginConfig {
                plugin_id: "plugin-search".into(),
                name: Some("Web Search".into()),
                enabled: Some(true),
                settings: BTreeMap::new(),
            }]),
        };
        insta::assert_json_snapshot!(req);
    }

    #[test]
    fn kimi_chat_response_with_search() {
        let resp = KimiChatResponse {
            id: "cmpl-snap001".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "moonshot-v1-128k".into(),
            choices: vec![KimiChatChoice {
                index: 0,
                message: KimiChatChoiceMessage {
                    role: "assistant".into(),
                    content: Some("Based on my search, here are the results.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
            search_results: Some(vec![
                KimiSearchResult {
                    index: 1,
                    url: "https://example.com/article1".into(),
                    title: Some("Rust Async Guide".into()),
                    snippet: Some("An introduction to async/await in Rust...".into()),
                },
                KimiSearchResult {
                    index: 2,
                    url: "https://example.com/article2".into(),
                    title: Some("Tokio Runtime".into()),
                    snippet: None,
                },
            ]),
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn kimi_response_with_tool_calls() {
        let resp = KimiChatResponse {
            id: "cmpl-snap002".into(),
            object: "chat.completion".into(),
            created: 1700000100,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChatChoice {
                index: 0,
                message: KimiChatChoiceMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![KimiToolCall {
                        id: "tc_kimi_001".into(),
                        call_type: "function".into(),
                        function: KimiFunctionCall {
                            name: "get_weather".into(),
                            arguments: r#"{"city":"Tokyo"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 60,
                completion_tokens: 20,
                total_tokens: 80,
            }),
            search_results: None,
        };
        insta::assert_json_snapshot!(resp);
    }

    #[test]
    fn kimi_stream_event() {
        let event = KimiStreamEvent {
            id: "cmpl-snap003".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000200,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello from Kimi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            search_results: None,
        };
        insta::assert_json_snapshot!(event);
    }

    #[test]
    fn kimi_error_response() {
        let err = KimiErrorResponse {
            error: KimiErrorBody {
                message: "Context length exceeded".into(),
                error_type: "invalid_request_error".into(),
                param: None,
                code: Some("context_length_exceeded".into()),
            },
        };
        insta::assert_json_snapshot!(err);
    }
}
