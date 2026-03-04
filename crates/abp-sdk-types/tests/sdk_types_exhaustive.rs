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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Exhaustive integration tests for all SDK dialect types in `abp-sdk-types`.

use std::collections::BTreeMap;

use abp_sdk_types::claude::*;
use abp_sdk_types::codex::*;
use abp_sdk_types::common::*;
use abp_sdk_types::convert::*;
use abp_sdk_types::copilot::*;
use abp_sdk_types::gemini::*;
use abp_sdk_types::kimi::*;
use abp_sdk_types::openai::*;
use abp_sdk_types::*;

// ════════════════════════════════════════════════════════════════════════
// §1  OpenAI types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn openai_request_minimal() {
    let req = OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: OpenAiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn openai_request_full_fields() {
    let req = OpenAiRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![
            OpenAiMessage {
                role: "system".into(),
                content: Some("system prompt".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAiMessage {
                role: "user".into(),
                content: Some("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        tools: Some(vec![OpenAiToolDef {
            tool_type: "function".into(),
            function: OpenAiFunctionDef {
                name: "calc".into(),
                description: "Calculator".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Required)),
        temperature: Some(1.0),
        max_tokens: Some(8192),
        response_format: Some(ResponseFormat::JsonObject {}),
        stream: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: OpenAiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn openai_message_with_tool_calls() {
    let msg = OpenAiMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAiFunctionCall {
                name: "read".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("call_1"));
    let back: OpenAiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn openai_message_tool_response() {
    let msg = OpenAiMessage {
        role: "tool".into(),
        content: Some("file contents".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: OpenAiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn openai_tool_choice_mode_none() {
    let tc = ToolChoice::Mode(ToolChoiceMode::None);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn openai_tool_choice_mode_auto() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn openai_tool_choice_function() {
    let tc = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "read_file".into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn openai_response_format_text() {
    let rf = ResponseFormat::Text {};
    let json = serde_json::to_string(&rf).unwrap();
    let back: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(rf, back);
}

#[test]
fn openai_response_format_json_schema() {
    let rf = ResponseFormat::JsonSchema {
        json_schema: serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}}),
    };
    let json = serde_json::to_string(&rf).unwrap();
    let back: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(rf, back);
}

#[test]
fn openai_response_with_usage() {
    let resp = OpenAiResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAiUsage {
            prompt_tokens: 10,
            completion_tokens: 3,
            total_tokens: 13,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: OpenAiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn openai_response_no_usage() {
    let resp = OpenAiResponse {
        id: "chatcmpl-x".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("usage"));
    let back: OpenAiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn openai_stream_chunk_initial_delta() {
    let chunk = OpenAiStreamChunk {
        id: "chatcmpl-s1".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAiStreamChoice {
            index: 0,
            delta: OpenAiStreamDelta {
                role: Some("assistant".into()),
                content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: OpenAiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn openai_stream_delta_default() {
    let delta = OpenAiStreamDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn openai_config_default() {
    let cfg = OpenAiConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.base_url.starts_with("https://"));
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

#[test]
fn openai_config_roundtrip() {
    let cfg = OpenAiConfig {
        base_url: "http://localhost:8080".into(),
        model: "local-model".into(),
        max_tokens: Some(512),
        temperature: Some(0.3),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: OpenAiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

// ════════════════════════════════════════════════════════════════════════
// §2  Claude types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn claude_request_minimal() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        system: None,
        messages: vec![],
        tools: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn claude_request_with_thinking() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 16384,
        system: Some("Be concise".into()),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "Explain monads".into(),
        }],
        tools: None,
        thinking: Some(ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 8192,
        }),
        stream: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("budget_tokens"));
    let back: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn claude_content_block_text() {
    let block = ClaudeContentBlock::Text {
        text: "Hello!".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"text""#));
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn claude_content_block_tool_use() {
    let block = ClaudeContentBlock::ToolUse {
        id: "toolu_01".into(),
        name: "search".into(),
        input: serde_json::json!({"query": "rust"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn claude_content_block_tool_result() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "toolu_01".into(),
        content: Some("42 results found".into()),
        is_error: Some(false),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn claude_content_block_tool_result_error() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "toolu_01".into(),
        content: Some("not found".into()),
        is_error: Some(true),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("is_error"));
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn claude_content_block_thinking_no_signature() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "step 1: ...".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("signature"));
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn claude_response_multiple_blocks() {
    let resp = ClaudeResponse {
        id: "msg_abc".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Thinking {
                thinking: "Let me think...".into(),
                signature: Some("sig_abc".into()),
            },
            ClaudeContentBlock::Text {
                text: "Here's my answer.".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "grep".into(),
                input: serde_json::json!({"pattern": "TODO"}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 500,
            output_tokens: 200,
            cache_creation_input_tokens: Some(100),
            cache_read_input_tokens: Some(50),
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
    assert_eq!(resp.content.len(), 3);
}

#[test]
fn claude_usage_minimal() {
    let u = ClaudeUsage {
        input_tokens: 10,
        output_tokens: 5,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&u).unwrap();
    assert!(!json.contains("cache"));
    let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn claude_stream_message_start() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_start".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_content_block_start() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_delta_input_json() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"pa"#.into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_delta_thinking() {
    let delta = ClaudeStreamDelta::ThinkingDelta {
        thinking: "step 2...".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(delta, back);
}

#[test]
fn claude_stream_delta_signature() {
    let delta = ClaudeStreamDelta::SignatureDelta {
        signature: "abc123".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(delta, back);
}

#[test]
fn claude_stream_content_block_stop() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 0 };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_message_delta() {
    let event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_message_stop() {
    let event = ClaudeStreamEvent::MessageStop {};
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_ping() {
    let event = ClaudeStreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_stream_error() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "Service overloaded".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_api_error_roundtrip() {
    let err = ClaudeApiError {
        error_type: "invalid_request_error".into(),
        message: "max_tokens too large".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn claude_config_default() {
    let cfg = ClaudeConfig::default();
    assert!(cfg.base_url.contains("anthropic.com"));
    assert!(cfg.model.starts_with("claude"));
    assert_eq!(cfg.max_tokens, 4096);
    assert!(cfg.system_prompt.is_none());
    assert!(cfg.thinking.is_none());
}

#[test]
fn claude_tool_def_roundtrip() {
    let tool = ClaudeToolDef {
        name: "bash".into(),
        description: "Run a bash command".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string"}},
            "required": ["command"]
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ClaudeToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// ════════════════════════════════════════════════════════════════════════
// §3  Gemini types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_request_minimal() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn gemini_request_with_system_instruction() {
    let req = GeminiRequest {
        model: "gemini-2.5-pro".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be helpful".into())],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(2048),
            temperature: Some(0.9),
            top_p: Some(0.95),
            top_k: Some(40),
            stop_sequences: Some(vec!["END".into()]),
        }),
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }]),
        tools: None,
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn gemini_part_text_roundtrip() {
    let part = GeminiPart::Text("Hello world".into());
    let json = serde_json::to_string(&part).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn gemini_part_inline_data() {
    let part = GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    });
    let json = serde_json::to_string(&part).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn gemini_part_function_call() {
    let part = GeminiPart::FunctionCall {
        name: "get_weather".into(),
        args: serde_json::json!({"city": "London"}),
    };
    let json = serde_json::to_string(&part).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn gemini_part_function_response() {
    let part = GeminiPart::FunctionResponse {
        name: "get_weather".into(),
        response: serde_json::json!({"temp": 15, "unit": "C"}),
    };
    let json = serde_json::to_string(&part).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn gemini_function_calling_mode_all_variants() {
    for mode in [
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

#[test]
fn gemini_tool_config_with_allowed_names() {
    let tc = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "calc".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn gemini_harm_category_all_variants() {
    for cat in [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
    ] {
        let json = serde_json::to_string(&cat).unwrap();
        let back: HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, back);
    }
}

#[test]
fn gemini_harm_block_threshold_all_variants() {
    for threshold in [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ] {
        let json = serde_json::to_string(&threshold).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(threshold, back);
    }
}

#[test]
fn gemini_response_with_usage_metadata() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Answer".into())],
            },
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 20,
            candidates_token_count: 10,
            total_token_count: 30,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GeminiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn gemini_stream_chunk_empty_candidates() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 0,
            total_token_count: 5,
        }),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn gemini_generation_config_default() {
    let cfg = GeminiGenerationConfig::default();
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.stop_sequences.is_none());
}

#[test]
fn gemini_config_default() {
    let cfg = GeminiConfig::default();
    assert!(cfg.base_url.contains("googleapis.com"));
    assert!(cfg.model.contains("gemini"));
    assert_eq!(cfg.max_output_tokens, Some(4096));
}

// ════════════════════════════════════════════════════════════════════════
// §4  Codex types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn codex_input_item_message() {
    let item = CodexInputItem::Message {
        role: "user".into(),
        content: "Write a test".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexInputItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn codex_tool_function() {
    let tool = CodexTool::Function {
        function: CodexFunctionDef {
            name: "execute".into(),
            description: "Execute code".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn codex_tool_code_interpreter() {
    let tool = CodexTool::CodeInterpreter {};
    let json = serde_json::to_string(&tool).unwrap();
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn codex_tool_file_search_with_limit() {
    let tool = CodexTool::FileSearch {
        max_num_results: Some(10),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn codex_tool_file_search_no_limit() {
    let tool = CodexTool::FileSearch {
        max_num_results: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn codex_text_format_text() {
    let fmt = CodexTextFormat::Text {};
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(fmt, back);
}

#[test]
fn codex_text_format_json_schema() {
    let fmt = CodexTextFormat::JsonSchema {
        name: "output".into(),
        schema: serde_json::json!({"type": "object"}),
        strict: true,
    };
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(fmt, back);
}

#[test]
fn codex_sandbox_config_default() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.networking, NetworkAccess::None);
    assert_eq!(cfg.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(cfg.timeout_seconds, Some(300));
    assert!(cfg.container_image.is_none());
    assert!(cfg.env.is_empty());
}

#[test]
fn codex_sandbox_config_full() {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin".into());
    let cfg = SandboxConfig {
        container_image: Some("ubuntu:22.04".into()),
        networking: NetworkAccess::Full,
        file_access: FileAccess::Full,
        timeout_seconds: Some(600),
        env,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn codex_network_access_default() {
    assert_eq!(NetworkAccess::default(), NetworkAccess::None);
}

#[test]
fn codex_file_access_default() {
    assert_eq!(FileAccess::default(), FileAccess::WorkspaceOnly);
}

#[test]
fn codex_response_item_message() {
    let item = CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Done!".into(),
        }],
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn codex_response_item_function_call() {
    let item = CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: Some("call_abc".into()),
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn codex_response_item_function_call_output() {
    let item = CodexResponseItem::FunctionCallOutput {
        call_id: "call_abc".into(),
        output: "file1.rs\nfile2.rs".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn codex_response_item_reasoning() {
    let item = CodexResponseItem::Reasoning {
        summary: vec![
            ReasoningSummary {
                text: "First I'll analyze...".into(),
            },
            ReasoningSummary {
                text: "Then I'll implement...".into(),
            },
        ],
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn codex_stream_event_response_created() {
    let event = CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: "resp_new".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn codex_stream_event_output_item_added() {
    let event = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![],
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn codex_stream_delta_function_args() {
    let delta = CodexStreamDelta::FunctionCallArgumentsDelta {
        delta: r#"{"pa"#.into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(delta, back);
}

#[test]
fn codex_stream_event_error() {
    let event = CodexStreamEvent::Error {
        message: "Rate limited".into(),
        code: Some("rate_limit".into()),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn codex_stream_event_response_completed() {
    let event = CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: "resp_done".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Done".into(),
                }],
            }],
            usage: Some(CodexUsage {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
            }),
            status: Some("completed".into()),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn codex_config_default() {
    let cfg = CodexConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
    assert!(cfg.model.contains("codex"));
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert_eq!(cfg.sandbox.networking, NetworkAccess::None);
}

// ════════════════════════════════════════════════════════════════════════
// §5  Copilot types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_reference_type_all_variants() {
    for rt in [
        CopilotReferenceType::File,
        CopilotReferenceType::Snippet,
        CopilotReferenceType::Repository,
        CopilotReferenceType::WebSearchResult,
    ] {
        let json = serde_json::to_string(&rt).unwrap();
        let back: CopilotReferenceType = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, back);
    }
}

#[test]
fn copilot_reference_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("uri".into(), serde_json::json!("file:///src/main.rs"));
    meta.insert("language".into(), serde_json::json!("rust"));
    let reference = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-1".into(),
        data: serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"}),
        metadata: Some(meta),
    };
    let json = serde_json::to_string(&reference).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(reference, back);
}

#[test]
fn copilot_reference_no_metadata() {
    let reference = CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: "repo-1".into(),
        data: serde_json::json!({"owner": "rust-lang", "name": "rust"}),
        metadata: None,
    };
    let json = serde_json::to_string(&reference).unwrap();
    assert!(!json.contains("metadata"));
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(reference, back);
}

#[test]
fn copilot_message_with_references() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "Explain this code".into(),
        name: Some("developer".into()),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: "snip-0".into(),
            data: serde_json::json!({"content": "fn foo() {}"}),
            metadata: None,
        }],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: CopilotMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn copilot_message_minimal() {
    let msg = CopilotMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
        name: None,
        copilot_references: vec![],
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("name"));
    assert!(!json.contains("copilot_references"));
    let back: CopilotMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn copilot_tool_function_type() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "search".into(),
            description: "Search code".into(),
            parameters: serde_json::json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn copilot_tool_confirmation_type() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "confirm-1".into(),
            title: "Delete file?".into(),
            message: "Are you sure you want to delete main.rs?".into(),
            accepted: None,
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn copilot_confirmation_accepted() {
    let conf = CopilotConfirmation {
        id: "confirm-2".into(),
        title: "Deploy?".into(),
        message: "Deploy to production?".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_string(&conf).unwrap();
    let back: CopilotConfirmation = serde_json::from_str(&json).unwrap();
    assert_eq!(conf, back);
}

#[test]
fn copilot_function_call_roundtrip() {
    let fc = CopilotFunctionCall {
        name: "read_file".into(),
        arguments: r#"{"path":"src/lib.rs"}"#.into(),
        id: Some("fc_1".into()),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let back: CopilotFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(fc, back);
}

#[test]
fn copilot_error_roundtrip() {
    let err = CopilotError {
        error_type: "agent_error".into(),
        message: "Something went wrong".into(),
        code: Some("500".into()),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: CopilotError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn copilot_turn_entry_roundtrip() {
    let entry = CopilotTurnEntry {
        request: "What is Rust?".into(),
        response: "Rust is a systems programming language.".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn copilot_request_with_history_and_refs() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "Refactor this".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: None,
        turn_history: vec![CopilotTurnEntry {
            request: "What does this do?".into(),
            response: "It processes data.".into(),
        }],
        references: vec![CopilotReference {
            ref_type: CopilotReferenceType::WebSearchResult,
            id: "web-0".into(),
            data: serde_json::json!({"url": "https://example.com"}),
            metadata: None,
        }],
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn copilot_response_full() {
    let resp = CopilotResponse {
        message: "Here you go".into(),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f-1".into(),
            data: serde_json::json!({}),
            metadata: None,
        }],
        copilot_errors: vec![CopilotError {
            error_type: "warning".into(),
            message: "Large file".into(),
            code: None,
        }],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c-1".into(),
            title: "Apply changes?".into(),
            message: "Shall I apply?".into(),
            accepted: Some(false),
        }),
        function_call: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn copilot_stream_event_references() {
    let event = CopilotStreamEvent::CopilotReferences {
        references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f-0".into(),
            data: serde_json::json!({}),
            metadata: None,
        }],
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn copilot_stream_event_errors() {
    let event = CopilotStreamEvent::CopilotErrors {
        errors: vec![CopilotError {
            error_type: "timeout".into(),
            message: "Request timed out".into(),
            code: Some("408".into()),
        }],
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn copilot_stream_event_function_call() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "edit_file".into(),
            arguments: "{}".into(),
            id: None,
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn copilot_stream_event_confirmation() {
    let event = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c-1".into(),
            title: "Proceed?".into(),
            message: "Run tests?".into(),
            accepted: None,
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn copilot_config_default() {
    let cfg = CopilotConfig::default();
    assert!(cfg.base_url.contains("githubcopilot"));
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.system_prompt.is_none());
}

// ════════════════════════════════════════════════════════════════════════
// §6  Kimi types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_request_with_search() {
    let req = KimiRequest {
        model: "moonshot-v1-128k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("Search for Rust tutorials".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: Some(4096),
        temperature: Some(0.5),
        stream: Some(true),
        tools: Some(vec![KimiTool::BuiltinFunction {
            function: KimiBuiltinFunction {
                name: "$web_search".into(),
            },
        }]),
        use_search: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn kimi_tool_function() {
    let tool = KimiTool::Function {
        function: KimiFunctionDef {
            name: "calc".into(),
            description: "Calculate".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn kimi_tool_builtin() {
    let tool = KimiTool::BuiltinFunction {
        function: KimiBuiltinFunction {
            name: "$browser".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn kimi_response_with_refs() {
    let resp = KimiResponse {
        id: "cmpl_abc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("According to [1]...".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
        refs: Some(vec![
            KimiRef {
                index: 1,
                url: "https://example.com/rust".into(),
                title: Some("Rust Guide".into()),
            },
            KimiRef {
                index: 2,
                url: "https://example.com/async".into(),
                title: None,
            },
        ]),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn kimi_message_with_tool_calls() {
    let msg = KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "call_k1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn kimi_message_tool_response() {
    let msg = KimiMessage {
        role: "tool".into(),
        content: Some("search results here".into()),
        tool_call_id: Some("call_k1".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn kimi_stream_chunk_with_refs() {
    let chunk = KimiStreamChunk {
        id: "cmpl_s1".into(),
        object: "chat.completion.chunk".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("partial".into()),
            },
            finish_reason: None,
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        }]),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: KimiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn kimi_chunk_delta_default() {
    let delta = KimiChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
}

#[test]
fn kimi_config_default() {
    let cfg = KimiConfig::default();
    assert!(cfg.base_url.contains("moonshot.cn"));
    assert!(cfg.model.contains("moonshot"));
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.use_k1_reasoning.is_none());
}

#[test]
fn kimi_config_roundtrip() {
    let cfg = KimiConfig {
        base_url: "https://custom.api/v1".into(),
        model: "moonshot-v1-32k".into(),
        max_tokens: Some(8192),
        temperature: Some(0.8),
        use_k1_reasoning: Some(true),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: KimiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

// ════════════════════════════════════════════════════════════════════════
// §7  Common types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn common_role_all_variants_serde() {
    let roles = [Role::System, Role::User, Role::Assistant, Role::Tool];
    for role in roles {
        let json = serde_json::to_string(&role).unwrap();
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn common_role_display() {
    assert_eq!(Role::System.to_string(), "system");
    assert_eq!(Role::User.to_string(), "user");
    assert_eq!(Role::Assistant.to_string(), "assistant");
    assert_eq!(Role::Tool.to_string(), "tool");
}

#[test]
fn common_token_usage_default() {
    let u = TokenUsage::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.total_tokens.is_none());
}

#[test]
fn common_token_usage_partial() {
    let u = TokenUsage {
        input_tokens: Some(100),
        output_tokens: None,
        total_tokens: None,
    };
    let json = serde_json::to_string(&u).unwrap();
    assert!(!json.contains("output_tokens"));
    let back: TokenUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn common_finish_reason_all_variants() {
    let reasons = [
        FinishReason::Stop,
        FinishReason::ToolUse,
        FinishReason::MaxTokens,
        FinishReason::StopSequence,
        FinishReason::ContentFilter,
    ];
    for reason in reasons {
        let json = serde_json::to_string(&reason).unwrap();
        let back: FinishReason = serde_json::from_str(&json).unwrap();
        assert_eq!(reason, back);
    }
}

// ════════════════════════════════════════════════════════════════════════
// §8  Top-level types (Dialect, DialectRequest, etc.)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_labels_unique() {
    let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
    let mut deduped = labels.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(labels.len(), deduped.len());
}

#[test]
fn dialect_display_matches_label() {
    for d in Dialect::all() {
        assert_eq!(d.to_string(), d.label());
    }
}

#[test]
fn model_config_default() {
    let cfg = ModelConfig::default();
    assert!(cfg.model.is_empty());
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.stop_sequences.is_none());
    assert!(cfg.extra.is_empty());
}

#[test]
fn model_config_with_extra() {
    let mut extra = BTreeMap::new();
    extra.insert("seed".into(), serde_json::json!(42));
    let cfg = ModelConfig {
        model: "gpt-4o".into(),
        max_tokens: Some(1024),
        temperature: Some(0.5),
        top_p: Some(0.9),
        stop_sequences: Some(vec!["DONE".into(), "END".into()]),
        extra,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("seed"));
    let back: ModelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn model_config_skip_empty_extra() {
    let cfg = ModelConfig {
        model: "test".into(),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("extra"));
}

#[test]
fn canonical_tool_def_roundtrip() {
    let def = CanonicalToolDef {
        name: "write_file".into(),
        description: "Write content to a file".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        }),
    };
    let json = serde_json::to_string(&def).unwrap();
    let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(def, back);
}

#[test]
fn dialect_request_model_extraction() {
    let models = vec![
        (
            DialectRequest::OpenAi(OpenAiRequest {
                model: "gpt-4o".into(),
                messages: vec![],
                tools: None,
                tool_choice: None,
                temperature: None,
                max_tokens: None,
                response_format: None,
                stream: None,
            }),
            "gpt-4o",
        ),
        (
            DialectRequest::Claude(ClaudeRequest {
                model: "claude-sonnet-4-20250514".into(),
                max_tokens: 1024,
                system: None,
                messages: vec![],
                tools: None,
                thinking: None,
                stream: None,
            }),
            "claude-sonnet-4-20250514",
        ),
        (
            DialectRequest::Gemini(GeminiRequest {
                model: "gemini-2.5-flash".into(),
                contents: vec![],
                system_instruction: None,
                generation_config: None,
                safety_settings: None,
                tools: None,
                tool_config: None,
            }),
            "gemini-2.5-flash",
        ),
    ];
    for (req, expected_model) in models {
        assert_eq!(req.model(), expected_model);
    }
}

#[test]
fn dialect_request_dialect_detection() {
    let kimi_req = DialectRequest::Kimi(KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    });
    assert_eq!(kimi_req.dialect(), Dialect::Kimi);

    let codex_req = DialectRequest::Codex(CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
    });
    assert_eq!(codex_req.dialect(), Dialect::Codex);

    let copilot_req = DialectRequest::Copilot(CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        turn_history: vec![],
        references: vec![],
    });
    assert_eq!(copilot_req.dialect(), Dialect::Copilot);
}

#[test]
fn dialect_response_dialect_detection() {
    let openai_resp = DialectResponse::OpenAi(OpenAiResponse {
        id: "resp-1".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    });
    assert_eq!(openai_resp.dialect(), Dialect::OpenAi);

    let kimi_resp = DialectResponse::Kimi(KimiResponse {
        id: "resp-k".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    });
    assert_eq!(kimi_resp.dialect(), Dialect::Kimi);

    let codex_resp = DialectResponse::Codex(CodexResponse {
        id: "resp-c".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: None,
        status: None,
    });
    assert_eq!(codex_resp.dialect(), Dialect::Codex);

    let copilot_resp = DialectResponse::Copilot(CopilotResponse {
        message: "hi".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    });
    assert_eq!(copilot_resp.dialect(), Dialect::Copilot);
}

#[test]
fn dialect_stream_chunk_dialect_detection() {
    let openai = DialectStreamChunk::OpenAi(OpenAiStreamChunk {
        id: "s1".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    });
    assert_eq!(openai.dialect(), Dialect::OpenAi);

    let claude = DialectStreamChunk::Claude(ClaudeStreamEvent::Ping {});
    assert_eq!(claude.dialect(), Dialect::Claude);

    let kimi = DialectStreamChunk::Kimi(KimiStreamChunk {
        id: "s2".into(),
        object: "chat.completion.chunk".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    });
    assert_eq!(kimi.dialect(), Dialect::Kimi);

    let codex = DialectStreamChunk::Codex(CodexStreamEvent::Error {
        message: "err".into(),
        code: None,
    });
    assert_eq!(codex.dialect(), Dialect::Codex);

    let copilot = DialectStreamChunk::Copilot(CopilotStreamEvent::Done {});
    assert_eq!(copilot.dialect(), Dialect::Copilot);
}

// ════════════════════════════════════════════════════════════════════════
// §9  Cross-SDK conversion
// ════════════════════════════════════════════════════════════════════════

#[test]
fn convert_message_roundtrip() {
    let msg = Message {
        role: "user".into(),
        content: Some("test".into()),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn convert_tool_definition_roundtrip() {
    let tool = ToolDefinition {
        name: "grep".into(),
        description: "Search files".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"pattern": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn conversion_error_all_variants_display() {
    let errors: Vec<ConversionError> = vec![
        ConversionError::UnsupportedField {
            field: "system".into(),
            dialect: Dialect::Claude,
        },
        ConversionError::IncompatibleType {
            source_type: "array".into(),
            target_type: "string".into(),
        },
        ConversionError::MissingRequiredField {
            field: "content".into(),
        },
        ConversionError::ContentTooLong {
            max: 100,
            actual: 200,
        },
    ];
    for err in &errors {
        let display = err.to_string();
        assert!(!display.is_empty());
    }
}

#[test]
fn conversion_error_is_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(ConversionError::MissingRequiredField {
        field: "model".into(),
    });
    assert!(err.to_string().contains("model"));
}

#[test]
fn conversion_report_new_clean() {
    let r = ConversionReport::new(Dialect::OpenAi, Dialect::Gemini);
    assert!(r.is_ok());
    assert!(r.is_lossless);
    assert_eq!(r.conversions, 0);
}

#[test]
fn conversion_report_with_warnings_still_ok() {
    let mut r = ConversionReport::new(Dialect::Claude, Dialect::Kimi);
    r.warnings.push("Thinking blocks stripped".into());
    r.conversions = 3;
    assert!(r.is_ok());
}

#[test]
fn conversion_report_serde_roundtrip() {
    let r = ConversionReport {
        source: Dialect::Codex,
        target: Dialect::Copilot,
        conversions: 10,
        warnings: vec!["sandbox ignored".into()],
        errors: vec![ConversionError::UnsupportedField {
            field: "sandbox".into(),
            dialect: Dialect::Copilot,
        }],
        is_lossless: false,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: ConversionReport = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
    assert!(!back.is_ok());
}

#[test]
fn role_mapper_all_openai_roles_identity() {
    for role in ["system", "user", "assistant", "tool"] {
        let mapped = RoleMapper::map_role(role, Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(mapped, role);
    }
}

#[test]
fn role_mapper_openai_to_gemini_user() {
    assert_eq!(
        RoleMapper::map_role("user", Dialect::OpenAi, Dialect::Gemini).unwrap(),
        "user"
    );
}

#[test]
fn role_mapper_openai_assistant_to_gemini_model() {
    assert_eq!(
        RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::Gemini).unwrap(),
        "model"
    );
}

#[test]
fn role_mapper_gemini_model_to_claude_assistant() {
    assert_eq!(
        RoleMapper::map_role("model", Dialect::Gemini, Dialect::Claude).unwrap(),
        "assistant"
    );
}

#[test]
fn role_mapper_tool_to_claude_fails() {
    let err = RoleMapper::map_role("tool", Dialect::OpenAi, Dialect::Claude).unwrap_err();
    assert!(matches!(err, ConversionError::UnsupportedField { .. }));
}

#[test]
fn role_mapper_system_to_gemini_fails() {
    let err = RoleMapper::map_role("system", Dialect::Kimi, Dialect::Gemini).unwrap_err();
    assert!(matches!(err, ConversionError::UnsupportedField { .. }));
}

#[test]
fn role_mapper_unknown_role_fails() {
    let err = RoleMapper::map_role("narrator", Dialect::Codex, Dialect::OpenAi).unwrap_err();
    assert!(matches!(err, ConversionError::IncompatibleType { .. }));
}

// ════════════════════════════════════════════════════════════════════════
// §10  Default values validation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn all_configs_default_serde_roundtrip() {
    // OpenAI
    let oai = OpenAiConfig::default();
    let json = serde_json::to_string(&oai).unwrap();
    let back: OpenAiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(oai, back);

    // Claude
    let cl = ClaudeConfig::default();
    let json = serde_json::to_string(&cl).unwrap();
    let back: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cl, back);

    // Gemini
    let gm = GeminiConfig::default();
    let json = serde_json::to_string(&gm).unwrap();
    let back: GeminiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(gm, back);

    // Codex
    let cx = CodexConfig::default();
    let json = serde_json::to_string(&cx).unwrap();
    let back: CodexConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cx, back);

    // Copilot
    let cp = CopilotConfig::default();
    let json = serde_json::to_string(&cp).unwrap();
    let back: CopilotConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cp, back);

    // Kimi
    let km = KimiConfig::default();
    let json = serde_json::to_string(&km).unwrap();
    let back: KimiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(km, back);

    // ModelConfig
    let mc = ModelConfig::default();
    let json = serde_json::to_string(&mc).unwrap();
    let back: ModelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(mc, back);
}

// ════════════════════════════════════════════════════════════════════════
// §11  JSON shape / field name validation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn openai_tool_call_uses_type_rename() {
    let tc = OpenAiToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: OpenAiFunctionCall {
            name: "test".into(),
            arguments: "{}".into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    // serde renames call_type to "type" in JSON
    assert!(json.contains(r#""type":"function""#));
}

#[test]
fn claude_content_block_uses_type_tag() {
    let block = ClaudeContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"text""#));
}

#[test]
fn gemini_uses_camel_case() {
    let cfg = GeminiGenerationConfig {
        max_output_tokens: Some(1024),
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("maxOutputTokens"));
}

#[test]
fn codex_input_item_uses_type_tag() {
    let item = CodexInputItem::Message {
        role: "user".into(),
        content: "hi".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"message""#));
}

#[test]
fn copilot_reference_type_uses_snake_case() {
    let rt = CopilotReferenceType::WebSearchResult;
    let json = serde_json::to_string(&rt).unwrap();
    assert!(json.contains("web_search_result"));
}

#[test]
fn dialect_request_uses_dialect_tag() {
    let req = DialectRequest::OpenAi(OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    });
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains(r#""dialect":"open_ai""#));
}

#[test]
fn dialect_serde_snake_case() {
    let json = serde_json::to_string(&Dialect::OpenAi).unwrap();
    assert_eq!(json, r#""open_ai""#);
    let json = serde_json::to_string(&Dialect::Claude).unwrap();
    assert_eq!(json, r#""claude""#);
    let json = serde_json::to_string(&Dialect::Gemini).unwrap();
    assert_eq!(json, r#""gemini""#);
    let json = serde_json::to_string(&Dialect::Kimi).unwrap();
    assert_eq!(json, r#""kimi""#);
    let json = serde_json::to_string(&Dialect::Codex).unwrap();
    assert_eq!(json, r#""codex""#);
    let json = serde_json::to_string(&Dialect::Copilot).unwrap();
    assert_eq!(json, r#""copilot""#);
}
