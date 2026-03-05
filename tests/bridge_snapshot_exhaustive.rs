#![allow(dead_code, unused_imports, unused_variables)]
#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive snapshot tests for all bridge crate types (Claude, Gemini, OpenAI).
//!
//! Covers request/response types, streaming events, content blocks, edge cases,
//! and cross-bridge consistency for the same logical payloads.

use serde_json::json;

use claude_bridge::claude_types::{
    ApiError as ClaudeApiError, CacheControl, ContentBlock, ImageSource, Message, MessageContent,
    MessageDelta, MessagesRequest, MessagesResponse, RequestMetadata, Role, StopReason,
    StreamDelta, StreamEvent, SystemBlock, SystemMessage, ThinkingConfig, ToolChoice,
    ToolDefinition as ClaudeToolDefinition, Usage as ClaudeUsage,
};
use gemini_bridge::gemini_types::{
    Candidate, CitationMetadata, CitationSource, Content, FunctionCall as GeminiFunctionCall,
    FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration, FunctionResponse,
    GeminiErrorDetail, GeminiErrorResponse, GeminiTool, GenerateContentRequest,
    GenerateContentResponse, GenerationConfig, HarmBlockThreshold, HarmCategory, HarmProbability,
    InlineData, Part, PromptFeedback, SafetyRating, SafetySetting, StreamGenerateContentResponse,
    ToolConfig, UsageMetadata,
};
use openai_bridge::openai_types::{
    ApiError as OpenAIApiError, ApiErrorDetail, ChatCompletionChoice, ChatCompletionChunk,
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatMessageRole,
    FunctionCall as OpenAIFunctionCall, FunctionDefinition, StreamChoice, StreamDelta as OaiDelta,
    StreamFunctionCall, StreamToolCall, ToolCall, ToolDefinition as OpenAIToolDefinition,
    Usage as OpenAIUsage,
};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Claude type snapshots
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;

    #[test]
    fn claude_messages_request_minimal() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hello".into()),
            }],
            max_tokens: 1024,
            system: None,
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        };
        insta::assert_json_snapshot!("claude_request_minimal", &req);
    }

    #[test]
    fn claude_messages_request_full() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Text {
                            text: "Analyze this image".into(),
                        },
                        ContentBlock::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".into(),
                                data: "iVBORw0KGgo=".into(),
                            },
                        },
                    ]),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                        id: "tool_01".into(),
                        name: "read_file".into(),
                        input: json!({"path": "/tmp/data.txt"}),
                    }]),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "tool_01".into(),
                        content: Some("file contents here".into()),
                        is_error: Some(false),
                    }]),
                },
            ],
            max_tokens: 4096,
            system: Some(SystemMessage::Blocks(vec![SystemBlock::Text {
                text: "You are a helpful assistant.".into(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral".into(),
                }),
            }])),
            tools: Some(vec![ClaudeToolDefinition {
                name: "read_file".into(),
                description: "Read file contents".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                }),
            }]),
            metadata: Some(RequestMetadata {
                user_id: Some("user-123".into()),
            }),
            stream: Some(true),
            stop_sequences: Some(vec!["END".into(), "STOP".into()]),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            tool_choice: Some(ToolChoice::Auto {}),
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".into(),
                budget_tokens: 10000,
            }),
        };
        insta::assert_json_snapshot!("claude_request_full", &req);
    }

    #[test]
    fn claude_messages_response() {
        let resp = MessagesResponse {
            id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Here is the result.".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: 25,
                output_tokens: 150,
                cache_creation_input_tokens: Some(100),
                cache_read_input_tokens: Some(50),
            },
        };
        insta::assert_json_snapshot!("claude_response", &resp);
    }

    #[test]
    fn claude_stream_event_message_start() {
        let evt = StreamEvent::MessageStart {
            message: MessagesResponse {
                id: "msg_stream_01".into(),
                response_type: "message".into(),
                role: "assistant".into(),
                content: vec![],
                model: "claude-sonnet-4-20250514".into(),
                stop_reason: None,
                stop_sequence: None,
                usage: ClaudeUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
        };
        insta::assert_json_snapshot!("claude_stream_message_start", &evt);
    }

    #[test]
    fn claude_stream_event_content_block_start() {
        let evt = StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text { text: "".into() },
        };
        insta::assert_json_snapshot!("claude_stream_content_block_start", &evt);
    }

    #[test]
    fn claude_stream_event_text_delta() {
        let evt = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello, ".into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_text_delta", &evt);
    }

    #[test]
    fn claude_stream_event_input_json_delta() {
        let evt = StreamEvent::ContentBlockDelta {
            index: 1,
            delta: StreamDelta::InputJsonDelta {
                partial_json: r#"{"path": "/tmp"#.into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_input_json_delta", &evt);
    }

    #[test]
    fn claude_stream_event_thinking_delta() {
        let evt = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::ThinkingDelta {
                thinking: "Let me think about this...".into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_thinking_delta", &evt);
    }

    #[test]
    fn claude_stream_event_signature_delta() {
        let evt = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::SignatureDelta {
                signature: "abc123signature".into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_signature_delta", &evt);
    }

    #[test]
    fn claude_stream_event_content_block_stop() {
        let evt = StreamEvent::ContentBlockStop { index: 0 };
        insta::assert_json_snapshot!("claude_stream_content_block_stop", &evt);
    }

    #[test]
    fn claude_stream_event_message_delta() {
        let evt = StreamEvent::MessageDelta {
            delta: MessageDelta {
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
        insta::assert_json_snapshot!("claude_stream_message_delta", &evt);
    }

    #[test]
    fn claude_stream_event_message_stop() {
        let evt = StreamEvent::MessageStop {};
        insta::assert_json_snapshot!("claude_stream_message_stop", &evt);
    }

    #[test]
    fn claude_stream_event_ping() {
        let evt = StreamEvent::Ping {};
        insta::assert_json_snapshot!("claude_stream_ping", &evt);
    }

    #[test]
    fn claude_stream_event_error() {
        let evt = StreamEvent::Error {
            error: ClaudeApiError {
                error_type: "overloaded_error".into(),
                message: "Service temporarily overloaded".into(),
            },
        };
        insta::assert_json_snapshot!("claude_stream_error", &evt);
    }

    #[test]
    fn claude_content_block_text() {
        let block = ContentBlock::Text {
            text: "Hello, world!".into(),
        };
        insta::assert_json_snapshot!("claude_content_text", &block);
    }

    #[test]
    fn claude_content_block_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "toolu_01A".into(),
            name: "bash".into(),
            input: json!({"command": "ls -la"}),
        };
        insta::assert_json_snapshot!("claude_content_tool_use", &block);
    }

    #[test]
    fn claude_content_block_tool_result() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_01A".into(),
            content: Some("total 42\ndrwxr-xr-x 2 user user 4096".into()),
            is_error: Some(false),
        };
        insta::assert_json_snapshot!("claude_content_tool_result", &block);
    }

    #[test]
    fn claude_content_block_tool_result_error() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_01A".into(),
            content: Some("Permission denied".into()),
            is_error: Some(true),
        };
        insta::assert_json_snapshot!("claude_content_tool_result_error", &block);
    }

    #[test]
    fn claude_content_block_thinking() {
        let block = ContentBlock::Thinking {
            thinking: "I need to consider the edge cases here...".into(),
            signature: Some("sig_abc123".into()),
        };
        insta::assert_json_snapshot!("claude_content_thinking", &block);
    }

    #[test]
    fn claude_content_block_thinking_no_signature() {
        let block = ContentBlock::Thinking {
            thinking: "Reasoning step by step...".into(),
            signature: None,
        };
        insta::assert_json_snapshot!("claude_content_thinking_no_sig", &block);
    }

    #[test]
    fn claude_content_block_image_base64() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/jpeg".into(),
                data: "/9j/4AAQSkZJRg==".into(),
            },
        };
        insta::assert_json_snapshot!("claude_content_image_base64", &block);
    }

    #[test]
    fn claude_content_block_image_url() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/image.png".into(),
            },
        };
        insta::assert_json_snapshot!("claude_content_image_url", &block);
    }

    #[test]
    fn claude_usage_default() {
        let usage = ClaudeUsage::default();
        insta::assert_json_snapshot!("claude_usage_default", &usage);
    }

    #[test]
    fn claude_usage_full() {
        let usage = ClaudeUsage {
            input_tokens: 500,
            output_tokens: 1200,
            cache_creation_input_tokens: Some(300),
            cache_read_input_tokens: Some(200),
        };
        insta::assert_json_snapshot!("claude_usage_full", &usage);
    }

    #[test]
    fn claude_tool_choice_variants() {
        let auto = ToolChoice::Auto {};
        let any = ToolChoice::Any {};
        let specific = ToolChoice::Tool {
            name: "read_file".into(),
        };
        insta::assert_yaml_snapshot!("claude_tool_choice_auto", &auto);
        insta::assert_yaml_snapshot!("claude_tool_choice_any", &any);
        insta::assert_yaml_snapshot!("claude_tool_choice_specific", &specific);
    }

    #[test]
    fn claude_system_message_text() {
        let sys = SystemMessage::Text("You are a helpful assistant.".into());
        insta::assert_json_snapshot!("claude_system_text", &sys);
    }

    #[test]
    fn claude_system_message_blocks() {
        let sys = SystemMessage::Blocks(vec![
            SystemBlock::Text {
                text: "Primary instructions".into(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral".into(),
                }),
            },
            SystemBlock::Text {
                text: "Secondary context".into(),
                cache_control: None,
            },
        ]);
        insta::assert_json_snapshot!("claude_system_blocks", &sys);
    }

    #[test]
    fn claude_stop_reason_variants() {
        let variants = vec![
            StopReason::EndTurn,
            StopReason::MaxTokens,
            StopReason::StopSequence,
            StopReason::ToolUse,
        ];
        insta::assert_yaml_snapshot!("claude_stop_reason_all", &variants);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Gemini type snapshots
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;

    #[test]
    fn gemini_request_minimal() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        insta::assert_json_snapshot!("gemini_request_minimal", &req);
    }

    #[test]
    fn gemini_request_full() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![
                Part::text("Analyze this data"),
                Part::inline_data("image/png", "iVBORw0KGgo="),
            ]))
            .add_content(Content::model(vec![Part::function_call(
                "read_file",
                json!({"path": "/tmp/data.txt"}),
            )]))
            .add_content(Content::user(vec![Part::function_response(
                "read_file",
                json!({"content": "file data here"}),
            )]))
            .system_instruction(Content::system(vec![Part::text(
                "You are a helpful assistant.",
            )]))
            .generation_config(GenerationConfig {
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                max_output_tokens: Some(4096),
                candidate_count: Some(1),
                stop_sequences: Some(vec!["END".into(), "STOP".into()]),
                response_mime_type: Some("application/json".into()),
                response_schema: Some(json!({"type": "object"})),
            })
            .safety_settings(vec![
                SafetySetting {
                    category: HarmCategory::HarmCategoryHarassment,
                    threshold: HarmBlockThreshold::BlockMediumAndAbove,
                },
                SafetySetting {
                    category: HarmCategory::HarmCategoryHateSpeech,
                    threshold: HarmBlockThreshold::BlockOnlyHigh,
                },
                SafetySetting {
                    category: HarmCategory::HarmCategoryDangerousContent,
                    threshold: HarmBlockThreshold::BlockNone,
                },
            ])
            .tools(vec![GeminiTool {
                function_declarations: vec![FunctionDeclaration {
                    name: "read_file".into(),
                    description: "Read file contents".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        },
                        "required": ["path"]
                    }),
                }],
            }]);
        insta::assert_json_snapshot!("gemini_request_full", &req);
    }

    #[test]
    fn gemini_response_simple() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("Hello! How can I help?")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
            prompt_feedback: None,
        };
        insta::assert_json_snapshot!("gemini_response_simple", &resp);
    }

    #[test]
    fn gemini_candidate_with_safety_ratings() {
        let candidate = Candidate {
            content: Content::model(vec![Part::text("Response text")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![
                SafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Negligible,
                },
                SafetyRating {
                    category: HarmCategory::HarmCategoryHateSpeech,
                    probability: HarmProbability::Low,
                },
                SafetyRating {
                    category: HarmCategory::HarmCategorySexuallyExplicit,
                    probability: HarmProbability::Negligible,
                },
                SafetyRating {
                    category: HarmCategory::HarmCategoryDangerousContent,
                    probability: HarmProbability::Negligible,
                },
            ]),
            citation_metadata: Some(CitationMetadata {
                citation_sources: vec![CitationSource {
                    start_index: Some(0),
                    end_index: Some(50),
                    uri: Some("https://example.com/source".into()),
                    license: Some("MIT".into()),
                }],
            }),
        };
        insta::assert_json_snapshot!("gemini_candidate_full", &candidate);
    }

    #[test]
    fn gemini_content_all_part_variants() {
        let content = Content {
            role: Some("user".into()),
            parts: vec![
                Part::Text("Hello".into()),
                Part::InlineData(InlineData {
                    mime_type: "image/png".into(),
                    data: "base64data==".into(),
                }),
                Part::FunctionCall(GeminiFunctionCall {
                    name: "search".into(),
                    args: json!({"query": "test"}),
                }),
                Part::FunctionResponse(FunctionResponse {
                    name: "search".into(),
                    response: json!({"results": []}),
                }),
            ],
        };
        insta::assert_json_snapshot!("gemini_content_all_parts", &content);
    }

    #[test]
    fn gemini_function_call() {
        let fc = Part::function_call("get_weather", json!({"city": "London", "units": "celsius"}));
        insta::assert_json_snapshot!("gemini_function_call", &fc);
    }

    #[test]
    fn gemini_function_response() {
        let fr = Part::function_response(
            "get_weather",
            json!({"temperature": 15, "condition": "cloudy"}),
        );
        insta::assert_json_snapshot!("gemini_function_response", &fr);
    }

    #[test]
    fn gemini_generation_config_full() {
        let config = GenerationConfig {
            temperature: Some(0.8),
            top_p: Some(0.95),
            top_k: Some(50),
            max_output_tokens: Some(8192),
            candidate_count: Some(3),
            stop_sequences: Some(vec!["###".into()]),
            response_mime_type: Some("text/plain".into()),
            response_schema: None,
        };
        insta::assert_json_snapshot!("gemini_generation_config_full", &config);
    }

    #[test]
    fn gemini_generation_config_default() {
        let config = GenerationConfig::default();
        insta::assert_json_snapshot!("gemini_generation_config_default", &config);
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
            SafetySetting {
                category: HarmCategory::HarmCategoryCivicIntegrity,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            },
        ];
        insta::assert_yaml_snapshot!("gemini_safety_all_categories", &settings);
    }

    #[test]
    fn gemini_tool_config() {
        let tc = ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Any,
                allowed_function_names: Some(vec!["search".into(), "read_file".into()]),
            },
        };
        insta::assert_json_snapshot!("gemini_tool_config", &tc);
    }

    #[test]
    fn gemini_function_calling_modes() {
        let modes = vec![
            FunctionCallingMode::Auto,
            FunctionCallingMode::Any,
            FunctionCallingMode::None,
        ];
        insta::assert_yaml_snapshot!("gemini_function_calling_modes", &modes);
    }

    #[test]
    fn gemini_stream_response() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("partial ")]),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        insta::assert_json_snapshot!("gemini_stream_chunk", &chunk);
    }

    #[test]
    fn gemini_stream_response_final() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("done.")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: Some(vec![SafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::Negligible,
                }]),
                citation_metadata: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 5,
                candidates_token_count: 10,
                total_token_count: 15,
            }),
        };
        insta::assert_json_snapshot!("gemini_stream_final", &chunk);
    }

    #[test]
    fn gemini_prompt_feedback_blocked() {
        let feedback = PromptFeedback {
            block_reason: Some("SAFETY".into()),
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::High,
            }]),
        };
        insta::assert_json_snapshot!("gemini_prompt_feedback_blocked", &feedback);
    }

    #[test]
    fn gemini_error_response() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 400,
                message: "Invalid argument: contents is required".into(),
                status: Some("INVALID_ARGUMENT".into()),
            },
        };
        insta::assert_json_snapshot!("gemini_error_response", &err);
    }

    #[test]
    fn gemini_usage_metadata() {
        let usage = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 250,
            total_token_count: 350,
        };
        insta::assert_json_snapshot!("gemini_usage_metadata", &usage);
    }

    #[test]
    fn gemini_harm_probability_variants() {
        let variants = vec![
            HarmProbability::Negligible,
            HarmProbability::Low,
            HarmProbability::Medium,
            HarmProbability::High,
        ];
        insta::assert_yaml_snapshot!("gemini_harm_probability_all", &variants);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. OpenAI type snapshots
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;

    #[test]
    fn openai_request_minimal() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage {
                role: ChatMessageRole::User,
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!("openai_request_minimal", &req);
    }

    #[test]
    fn openai_request_full() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage {
                    role: ChatMessageRole::System,
                    content: Some("You are a helpful assistant.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::User,
                    content: Some("What files are in /tmp?".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_abc123".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "bash".into(),
                            arguments: r#"{"command":"ls /tmp"}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::Tool,
                    content: Some("file1.txt\nfile2.txt".into()),
                    tool_calls: None,
                    tool_call_id: Some("call_abc123".into()),
                },
            ],
            tools: Some(vec![OpenAIToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "bash".into(),
                    description: "Execute a shell command".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"}
                        },
                        "required": ["command"]
                    }),
                },
            }]),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            stream: Some(true),
            top_p: Some(0.9),
            frequency_penalty: Some(0.5),
            presence_penalty: Some(0.3),
            stop: Some(vec!["END".into()]),
            n: Some(1),
            tool_choice: Some(json!("auto")),
        };
        insta::assert_json_snapshot!("openai_request_full", &req);
    }

    #[test]
    fn openai_response() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-abc123".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o-2024-08-06".into(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: Some("Hello! How can I help you today?".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 15,
                completion_tokens: 20,
                total_tokens: 35,
            }),
        };
        insta::assert_json_snapshot!("openai_response", &resp);
    }

    #[test]
    fn openai_response_with_tool_calls() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-tool456".into(),
            object: "chat.completion".into(),
            created: 1700000100,
            model: "gpt-4o".into(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: None,
                    tool_calls: Some(vec![
                        ToolCall {
                            id: "call_001".into(),
                            call_type: "function".into(),
                            function: OpenAIFunctionCall {
                                name: "read_file".into(),
                                arguments: r#"{"path":"/tmp/a.txt"}"#.into(),
                            },
                        },
                        ToolCall {
                            id: "call_002".into(),
                            call_type: "function".into(),
                            function: OpenAIFunctionCall {
                                name: "read_file".into(),
                                arguments: r#"{"path":"/tmp/b.txt"}"#.into(),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 50,
                completion_tokens: 30,
                total_tokens: 80,
            }),
        };
        insta::assert_json_snapshot!("openai_response_tool_calls", &resp);
    }

    #[test]
    fn openai_chunk_text() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-chunk01".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: OaiDelta {
                    role: None,
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("openai_chunk_text", &chunk);
    }

    #[test]
    fn openai_chunk_role_first() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-chunk00".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: OaiDelta {
                    role: Some("assistant".into()),
                    content: None,
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("openai_chunk_role_first", &chunk);
    }

    #[test]
    fn openai_chunk_tool_call() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-chunk02".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: OaiDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: Some("call_stream_001".into()),
                        call_type: Some("function".into()),
                        function: Some(StreamFunctionCall {
                            name: Some("bash".into()),
                            arguments: Some(r#"{"com"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("openai_chunk_tool_call", &chunk);
    }

    #[test]
    fn openai_chunk_tool_call_continuation() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-chunk03".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: OaiDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: None,
                        call_type: None,
                        function: Some(StreamFunctionCall {
                            name: None,
                            arguments: Some(r#"mand":"ls"}"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
        };
        insta::assert_json_snapshot!("openai_chunk_tool_call_continuation", &chunk);
    }

    #[test]
    fn openai_chunk_finish() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-chunk99".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: OaiDelta::default(),
                finish_reason: Some("stop".into()),
            }],
        };
        insta::assert_json_snapshot!("openai_chunk_finish", &chunk);
    }

    #[test]
    fn openai_tool_call() {
        let tc = ToolCall {
            id: "call_xyz789".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"/tmp/out.txt","content":"hello"}"#.into(),
            },
        };
        insta::assert_json_snapshot!("openai_tool_call", &tc);
    }

    #[test]
    fn openai_message_system() {
        let msg = ChatMessage {
            role: ChatMessageRole::System,
            content: Some("You are a coding assistant.".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        insta::assert_json_snapshot!("openai_message_system", &msg);
    }

    #[test]
    fn openai_message_user() {
        let msg = ChatMessage {
            role: ChatMessageRole::User,
            content: Some("Write a function to sort an array".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        insta::assert_json_snapshot!("openai_message_user", &msg);
    }

    #[test]
    fn openai_message_assistant() {
        let msg = ChatMessage {
            role: ChatMessageRole::Assistant,
            content: Some("Here's a sorting function...".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        insta::assert_json_snapshot!("openai_message_assistant", &msg);
    }

    #[test]
    fn openai_message_tool() {
        let msg = ChatMessage {
            role: ChatMessageRole::Tool,
            content: Some("Command executed successfully".into()),
            tool_calls: None,
            tool_call_id: Some("call_abc".into()),
        };
        insta::assert_json_snapshot!("openai_message_tool", &msg);
    }

    #[test]
    fn openai_usage_default() {
        let usage = OpenAIUsage::default();
        insta::assert_json_snapshot!("openai_usage_default", &usage);
    }

    #[test]
    fn openai_usage_full() {
        let usage = OpenAIUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
        };
        insta::assert_json_snapshot!("openai_usage_full", &usage);
    }

    #[test]
    fn openai_error_response() {
        let err = OpenAIApiError {
            error: ApiErrorDetail {
                message: "Invalid model specified".into(),
                error_type: "invalid_request_error".into(),
                param: Some("model".into()),
                code: Some("model_not_found".into()),
            },
        };
        insta::assert_json_snapshot!("openai_error_response", &err);
    }

    #[test]
    fn openai_role_variants() {
        let variants = vec![
            ChatMessageRole::System,
            ChatMessageRole::User,
            ChatMessageRole::Assistant,
            ChatMessageRole::Tool,
        ];
        insta::assert_yaml_snapshot!("openai_role_variants", &variants);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Cross-bridge consistency
// ═══════════════════════════════════════════════════════════════════════════

mod cross_bridge {
    use super::*;

    /// Same logical "hello world" request expressed in all three bridge formats.
    #[test]
    fn cross_bridge_simple_request_claude() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("What is 2+2?".into()),
            }],
            max_tokens: 256,
            system: Some(SystemMessage::Text("You are a math tutor.".into())),
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: Some(0.0),
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        };
        insta::assert_json_snapshot!("cross_request_claude", &req);
    }

    #[test]
    fn cross_bridge_simple_request_gemini() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What is 2+2?")]))
            .system_instruction(Content::system(vec![Part::text("You are a math tutor.")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.0),
                max_output_tokens: Some(256),
                ..Default::default()
            });
        insta::assert_json_snapshot!("cross_request_gemini", &req);
    }

    #[test]
    fn cross_bridge_simple_request_openai() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage {
                    role: ChatMessageRole::System,
                    content: Some("You are a math tutor.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::User,
                    content: Some("What is 2+2?".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(256),
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: None,
        };
        insta::assert_json_snapshot!("cross_request_openai", &req);
    }

    /// Same logical response in all three formats.
    #[test]
    fn cross_bridge_simple_response_claude() {
        let resp = MessagesResponse {
            id: "msg_cross_01".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "2+2 equals 4.".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: 20,
                output_tokens: 10,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        insta::assert_json_snapshot!("cross_response_claude", &resp);
    }

    #[test]
    fn cross_bridge_simple_response_gemini() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content::model(vec![Part::text("2+2 equals 4.")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 20,
                candidates_token_count: 10,
                total_token_count: 30,
            }),
            prompt_feedback: None,
        };
        insta::assert_json_snapshot!("cross_response_gemini", &resp);
    }

    #[test]
    fn cross_bridge_simple_response_openai() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-cross01".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: Some("2+2 equals 4.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
            }),
        };
        insta::assert_json_snapshot!("cross_response_openai", &resp);
    }

    /// Verify JSON serialization roundtrips are stable for each bridge.
    #[test]
    fn cross_bridge_serialization_stability() {
        // Claude roundtrip
        let claude_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("test".into()),
            }],
            max_tokens: 100,
            system: None,
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        };
        let json1 = serde_json::to_string(&claude_req).unwrap();
        let roundtrip: MessagesRequest = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&roundtrip).unwrap();
        assert_eq!(json1, json2, "Claude serialization is not stable");

        // Gemini roundtrip
        let gemini_req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]));
        let json1 = serde_json::to_string(&gemini_req).unwrap();
        let roundtrip: GenerateContentRequest = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&roundtrip).unwrap();
        assert_eq!(json1, json2, "Gemini serialization is not stable");

        // OpenAI roundtrip
        let openai_req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage {
                role: ChatMessageRole::User,
                content: Some("test".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: None,
        };
        let json1 = serde_json::to_string(&openai_req).unwrap();
        let roundtrip: ChatCompletionRequest = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&roundtrip).unwrap();
        assert_eq!(json1, json2, "OpenAI serialization is not stable");
    }

    /// Tool call expressed in all three bridge formats.
    #[test]
    fn cross_bridge_tool_call_claude() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Read /tmp/data.txt".into()),
            }],
            max_tokens: 1024,
            system: None,
            tools: Some(vec![ClaudeToolDefinition {
                name: "read_file".into(),
                description: "Read a file".into(),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            }]),
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: Some(ToolChoice::Auto {}),
            thinking: None,
        };
        insta::assert_json_snapshot!("cross_tool_call_claude", &req);
    }

    #[test]
    fn cross_bridge_tool_call_gemini() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Read /tmp/data.txt")]))
            .tools(vec![GeminiTool {
                function_declarations: vec![FunctionDeclaration {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                }],
            }]);
        insta::assert_json_snapshot!("cross_tool_call_gemini", &req);
    }

    #[test]
    fn cross_bridge_tool_call_openai() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage {
                role: ChatMessageRole::User,
                content: Some("Read /tmp/data.txt".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Some(vec![OpenAIToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                },
            }]),
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: Some(json!("auto")),
        };
        insta::assert_json_snapshot!("cross_tool_call_openai", &req);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Edge case snapshots
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn edge_empty_content_claude() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("".into()),
        };
        insta::assert_json_snapshot!("edge_empty_content_claude", &msg);
    }

    #[test]
    fn edge_empty_content_gemini() {
        let content = Content::user(vec![Part::text("")]);
        insta::assert_json_snapshot!("edge_empty_content_gemini", &content);
    }

    #[test]
    fn edge_empty_content_openai() {
        let msg = ChatMessage {
            role: ChatMessageRole::User,
            content: Some("".into()),
            tool_calls: None,
            tool_call_id: None,
        };
        insta::assert_json_snapshot!("edge_empty_content_openai", &msg);
    }

    #[test]
    fn edge_empty_blocks_claude() {
        let msg = Message {
            role: Role::Assistant,
            content: MessageContent::Blocks(vec![]),
        };
        insta::assert_json_snapshot!("edge_empty_blocks_claude", &msg);
    }

    #[test]
    fn edge_empty_parts_gemini() {
        let content = Content {
            role: Some("model".into()),
            parts: vec![],
        };
        insta::assert_json_snapshot!("edge_empty_parts_gemini", &content);
    }

    #[test]
    fn edge_special_characters_claude() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text(
                "Special chars: <>&\"'\n\t\r\\/ \u{0000} \u{FFFF} 日本語 émojis 🎉🚀".into(),
            ),
        };
        insta::assert_json_snapshot!("edge_special_chars_claude", &msg);
    }

    #[test]
    fn edge_special_characters_gemini() {
        let content = Content::user(vec![Part::text(
            "Special chars: <>&\"'\n\t\r\\/ \u{0000} \u{FFFF} 日本語 émojis 🎉🚀",
        )]);
        insta::assert_json_snapshot!("edge_special_chars_gemini", &content);
    }

    #[test]
    fn edge_special_characters_openai() {
        let msg = ChatMessage {
            role: ChatMessageRole::User,
            content: Some(
                "Special chars: <>&\"'\n\t\r\\/ \u{0000} \u{FFFF} 日本語 émojis 🎉🚀".into(),
            ),
            tool_calls: None,
            tool_call_id: None,
        };
        insta::assert_json_snapshot!("edge_special_chars_openai", &msg);
    }

    #[test]
    fn edge_very_long_content() {
        let long_text = "A".repeat(10_000);
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text(long_text.clone()),
        };
        // Use YAML for readability with length assertion
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            json["content"].as_str().unwrap().len(),
            10_000,
            "Long content must preserve length"
        );
        // Snapshot just the structure (not the 10k text)
        let summary = json!({
            "role": "user",
            "content_length": 10_000,
            "content_starts_with": &long_text[..50],
            "content_ends_with": &long_text[9950..],
        });
        insta::assert_json_snapshot!("edge_long_content_summary", &summary);
    }

    #[test]
    fn edge_max_complexity_claude() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Text {
                            text: "Multi-block message".into(),
                        },
                        ContentBlock::Image {
                            source: ImageSource::Url {
                                url: "https://example.com/img.png".into(),
                            },
                        },
                    ]),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Thinking {
                            thinking: "Let me analyze this...".into(),
                            signature: Some("sig_think_01".into()),
                        },
                        ContentBlock::Text {
                            text: "I'll use a tool.".into(),
                        },
                        ContentBlock::ToolUse {
                            id: "tu_01".into(),
                            name: "bash".into(),
                            input: json!({"command": "echo hello"}),
                        },
                        ContentBlock::ToolUse {
                            id: "tu_02".into(),
                            name: "read_file".into(),
                            input: json!({"path": "/etc/hosts"}),
                        },
                    ]),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![
                        ContentBlock::ToolResult {
                            tool_use_id: "tu_01".into(),
                            content: Some("hello".into()),
                            is_error: Some(false),
                        },
                        ContentBlock::ToolResult {
                            tool_use_id: "tu_02".into(),
                            content: Some("Permission denied".into()),
                            is_error: Some(true),
                        },
                    ]),
                },
            ],
            max_tokens: 8192,
            system: Some(SystemMessage::Blocks(vec![
                SystemBlock::Text {
                    text: "System instruction 1".into(),
                    cache_control: Some(CacheControl {
                        cache_type: "ephemeral".into(),
                    }),
                },
                SystemBlock::Text {
                    text: "System instruction 2".into(),
                    cache_control: None,
                },
            ])),
            tools: Some(vec![
                ClaudeToolDefinition {
                    name: "bash".into(),
                    description: "Run shell commands".into(),
                    input_schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
                },
                ClaudeToolDefinition {
                    name: "read_file".into(),
                    description: "Read files".into(),
                    input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                },
            ]),
            metadata: Some(RequestMetadata {
                user_id: Some("power-user-42".into()),
            }),
            stream: Some(true),
            stop_sequences: Some(vec!["HALT".into(), "QUIT".into(), "EXIT".into()]),
            temperature: Some(0.5),
            top_p: Some(0.85),
            top_k: Some(30),
            tool_choice: Some(ToolChoice::Any {}),
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".into(),
                budget_tokens: 20000,
            }),
        };
        insta::assert_yaml_snapshot!("edge_max_complexity_claude", &req);
    }

    #[test]
    fn edge_max_complexity_gemini() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content {
                role: Some("user".into()),
                parts: vec![
                    Part::text("Analyze this data"),
                    Part::inline_data("image/png", "base64data=="),
                ],
            })
            .add_content(Content {
                role: Some("model".into()),
                parts: vec![Part::FunctionCall(GeminiFunctionCall {
                    name: "analyze".into(),
                    args: json!({"data": "sample", "mode": "deep"}),
                })],
            })
            .add_content(Content {
                role: Some("user".into()),
                parts: vec![Part::FunctionResponse(FunctionResponse {
                    name: "analyze".into(),
                    response: json!({"result": "complex output", "confidence": 0.95}),
                })],
            })
            .system_instruction(Content::system(vec![Part::text(
                "You are an advanced data analyst.",
            )]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                top_p: Some(0.85),
                top_k: Some(30),
                max_output_tokens: Some(8192),
                candidate_count: Some(1),
                stop_sequences: Some(vec!["HALT".into(), "QUIT".into()]),
                response_mime_type: Some("application/json".into()),
                response_schema: Some(json!({"type": "object", "properties": {"answer": {"type": "string"}}})),
            })
            .safety_settings(vec![
                SafetySetting {
                    category: HarmCategory::HarmCategoryHarassment,
                    threshold: HarmBlockThreshold::BlockNone,
                },
                SafetySetting {
                    category: HarmCategory::HarmCategoryHateSpeech,
                    threshold: HarmBlockThreshold::BlockNone,
                },
            ])
            .tools(vec![GeminiTool {
                function_declarations: vec![
                    FunctionDeclaration {
                        name: "analyze".into(),
                        description: "Analyze data deeply".into(),
                        parameters: json!({"type": "object", "properties": {"data": {"type": "string"}, "mode": {"type": "string"}}}),
                    },
                    FunctionDeclaration {
                        name: "summarize".into(),
                        description: "Summarize results".into(),
                        parameters: json!({"type": "object", "properties": {"text": {"type": "string"}}}),
                    },
                ],
            }]);
        insta::assert_yaml_snapshot!("edge_max_complexity_gemini", &req);
    }

    #[test]
    fn edge_max_complexity_openai() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage {
                    role: ChatMessageRole::System,
                    content: Some("You are an advanced data analyst.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::User,
                    content: Some("Analyze this dataset".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: None,
                    tool_calls: Some(vec![
                        ToolCall {
                            id: "call_a1".into(),
                            call_type: "function".into(),
                            function: OpenAIFunctionCall {
                                name: "analyze".into(),
                                arguments: r#"{"data":"sample","mode":"deep"}"#.into(),
                            },
                        },
                        ToolCall {
                            id: "call_a2".into(),
                            call_type: "function".into(),
                            function: OpenAIFunctionCall {
                                name: "summarize".into(),
                                arguments: r#"{"text":"results"}"#.into(),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::Tool,
                    content: Some(r#"{"result":"complex output","confidence":0.95}"#.into()),
                    tool_calls: None,
                    tool_call_id: Some("call_a1".into()),
                },
                ChatMessage {
                    role: ChatMessageRole::Tool,
                    content: Some("Summary: data looks good".into()),
                    tool_calls: None,
                    tool_call_id: Some("call_a2".into()),
                },
            ],
            tools: Some(vec![
                OpenAIToolDefinition {
                    tool_type: "function".into(),
                    function: FunctionDefinition {
                        name: "analyze".into(),
                        description: "Analyze data deeply".into(),
                        parameters: json!({"type": "object", "properties": {"data": {"type": "string"}, "mode": {"type": "string"}}}),
                    },
                },
                OpenAIToolDefinition {
                    tool_type: "function".into(),
                    function: FunctionDefinition {
                        name: "summarize".into(),
                        description: "Summarize results".into(),
                        parameters: json!({"type": "object", "properties": {"text": {"type": "string"}}}),
                    },
                },
            ]),
            temperature: Some(0.5),
            max_tokens: Some(8192),
            stream: Some(true),
            top_p: Some(0.85),
            frequency_penalty: Some(0.2),
            presence_penalty: Some(0.1),
            stop: Some(vec!["HALT".into(), "QUIT".into()]),
            n: Some(1),
            tool_choice: Some(json!("auto")),
        };
        insta::assert_yaml_snapshot!("edge_max_complexity_openai", &req);
    }

    #[test]
    fn edge_null_optional_fields_claude() {
        let resp = MessagesResponse {
            id: "msg_null_test".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        insta::assert_json_snapshot!("edge_null_fields_claude", &resp);
    }

    #[test]
    fn edge_null_optional_fields_openai() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-null".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
        };
        insta::assert_json_snapshot!("edge_null_fields_openai", &resp);
    }

    #[test]
    fn edge_null_optional_fields_gemini() {
        let resp = GenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
            prompt_feedback: None,
        };
        insta::assert_json_snapshot!("edge_null_fields_gemini", &resp);
    }

    #[test]
    fn edge_unicode_content_all_bridges() {
        let text = "こんにちは世界 🌍 Ñoño café résumé naïve";

        let claude = Message {
            role: Role::User,
            content: MessageContent::Text(text.into()),
        };
        let gemini = Content::user(vec![Part::text(text)]);
        let openai = ChatMessage {
            role: ChatMessageRole::User,
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
        };

        insta::assert_json_snapshot!("edge_unicode_claude", &claude);
        insta::assert_json_snapshot!("edge_unicode_gemini", &gemini);
        insta::assert_json_snapshot!("edge_unicode_openai", &openai);
    }

    #[test]
    fn edge_newlines_in_tool_args() {
        let claude_block = ContentBlock::ToolUse {
            id: "tu_newline".into(),
            name: "write_file".into(),
            input: json!({"path": "/tmp/test.py", "content": "def hello():\n    print('hello')\n\nhello()\n"}),
        };
        let openai_call = ToolCall {
            id: "call_newline".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"/tmp/test.py","content":"def hello():\n    print('hello')\n\nhello()\n"}"#.into(),
            },
        };
        let gemini_part = Part::function_call(
            "write_file",
            json!({"path": "/tmp/test.py", "content": "def hello():\n    print('hello')\n\nhello()\n"}),
        );

        insta::assert_json_snapshot!("edge_newlines_claude", &claude_block);
        insta::assert_json_snapshot!("edge_newlines_openai", &openai_call);
        insta::assert_json_snapshot!("edge_newlines_gemini", &gemini_part);
    }

    #[test]
    fn edge_empty_tool_args() {
        let claude_block = ContentBlock::ToolUse {
            id: "tu_empty".into(),
            name: "no_args_tool".into(),
            input: json!({}),
        };
        let openai_call = ToolCall {
            id: "call_empty".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "no_args_tool".into(),
                arguments: "{}".into(),
            },
        };
        let gemini_part = Part::function_call("no_args_tool", json!({}));

        insta::assert_json_snapshot!("edge_empty_tool_args_claude", &claude_block);
        insta::assert_json_snapshot!("edge_empty_tool_args_openai", &openai_call);
        insta::assert_json_snapshot!("edge_empty_tool_args_gemini", &gemini_part);
    }

    #[test]
    fn edge_deeply_nested_tool_args() {
        let nested = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "array": [1, 2, {"deep": true}],
                        "null_val": null,
                        "bool_val": false
                    }
                }
            }
        });

        let claude_block = ContentBlock::ToolUse {
            id: "tu_nested".into(),
            name: "complex_tool".into(),
            input: nested.clone(),
        };
        let gemini_part = Part::function_call("complex_tool", nested);

        insta::assert_json_snapshot!("edge_nested_args_claude", &claude_block);
        insta::assert_json_snapshot!("edge_nested_args_gemini", &gemini_part);
    }
}
