#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive dialect parsing tests for all 6 SDK dialects.
//!
//! Tests cover: request parsing, response parsing, tool calls, tool results,
//! streaming formats, error handling, edge cases, and IR normalization.

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use abp_dialect::registry::{parse_response, DialectError, DialectRegistry};
use abp_dialect::Dialect;
use serde_json::{json, Value};

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn registry() -> DialectRegistry {
    DialectRegistry::with_builtins()
}

fn parse(dialect: Dialect, v: &Value) -> Result<IrRequest, DialectError> {
    registry().parse(dialect, v)
}

fn roundtrip(dialect: Dialect, v: &Value) -> Value {
    let reg = registry();
    let ir = reg.parse(dialect, v).expect("parse");
    reg.serialize(dialect, &ir).expect("serialize")
}

fn text_of(blocks: &[IrContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| b.as_text())
        .collect::<Vec<_>>()
        .join("")
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI — request parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_basic_chat_request() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hello"}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4o"));
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful."));
    assert_eq!(ir.messages.len(), 2);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[1].role, IrRole::User);
    assert_eq!(ir.messages[1].text_content(), "Hello");
}

#[test]
fn openai_multi_turn_conversation() {
    let v = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "What is 2+2?"},
            {"role": "assistant", "content": "4"},
            {"role": "user", "content": "And 3+3?"}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages.len(), 3);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[1].text_content(), "4");
    assert_eq!(ir.messages[2].role, IrRole::User);
}

#[test]
fn openai_tool_definitions() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }
            }
        }]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "get_weather");
    assert_eq!(ir.tools[0].description, "Get the weather");
    assert!(ir.tools[0].parameters.get("properties").is_some());
}

#[test]
fn openai_multiple_tools() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [
            {"type": "function", "function": {"name": "tool_a", "description": "A", "parameters": {}}},
            {"type": "function", "function": {"name": "tool_b", "description": "B", "parameters": {}}}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.tools.len(), 2);
    assert_eq!(ir.tools[0].name, "tool_a");
    assert_eq!(ir.tools[1].name, "tool_b");
}

#[test]
fn openai_tool_call_in_assistant_message() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "user", "content": "What's the weather?"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"NYC\"}"
                    }
                }]
            }
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages.len(), 2);
    let assistant_msg = &ir.messages[1];
    assert_eq!(assistant_msg.role, IrRole::Assistant);
    let tool_calls = assistant_msg.tool_calls();
    assert_eq!(tool_calls.len(), 1);
    match &tool_calls[0] {
        IrContentBlock::ToolCall { id, name, input } => {
            assert_eq!(id, "call_123");
            assert_eq!(name, "get_weather");
            assert_eq!(input["location"], "NYC");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn openai_tool_result_message() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "tool", "tool_call_id": "call_123", "content": "72°F and sunny"}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Tool);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_call_id, "call_123");
            assert!(!is_error);
            assert_eq!(text_of(content), "72°F and sunny");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn openai_generation_config_temperature() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7,
        "max_tokens": 1024,
        "top_p": 0.9,
        "stop": ["END"]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.temperature, Some(0.7));
    assert_eq!(ir.config.max_tokens, Some(1024));
    assert_eq!(ir.config.top_p, Some(0.9));
    assert_eq!(ir.config.stop_sequences, vec!["END"]);
}

#[test]
fn openai_stop_as_string() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "stop": "DONE"
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.stop_sequences, vec!["DONE"]);
}

#[test]
fn openai_max_completion_tokens() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "max_completion_tokens": 2048
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(2048));
}

#[test]
fn openai_empty_content_not_added() {
    let v = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "assistant", "content": ""}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.messages[0].content.is_empty());
}

#[test]
fn openai_null_content_assistant() {
    let v = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "assistant", "content": null, "tool_calls": []}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.messages[0].content.is_empty());
}

#[test]
fn openai_no_model_field() {
    let v = json!({
        "messages": [{"role": "user", "content": "hi"}]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.model.is_none());
}

#[test]
fn openai_multiple_tool_calls_in_one_message() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "fn_a", "arguments": "{}"}},
                {"id": "c2", "type": "function", "function": {"name": "fn_b", "arguments": "{\"x\":1}"}}
            ]
        }]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    let calls = ir.messages[0].tool_calls();
    assert_eq!(calls.len(), 2);
}

#[test]
fn openai_tool_call_with_malformed_json_arguments() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "fn_a", "arguments": "not valid json"}
            }]
        }]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    let calls = ir.messages[0].tool_calls();
    assert_eq!(calls.len(), 1);
    // Should fallback to string value
    match &calls[0] {
        IrContentBlock::ToolCall { input, .. } => {
            assert!(input.is_string());
        }
        _ => panic!("expected ToolCall"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude — request parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_basic_messages_request() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "system": "You are helpful.",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful."));
    assert_eq!(ir.config.max_tokens, Some(1024));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn claude_content_blocks_text() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "Hello "},
                {"type": "text", "text": "world"}
            ]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].content.len(), 2);
    assert_eq!(ir.messages[0].text_content(), "Hello world");
}

#[test]
fn claude_tool_use_content_block() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "toolu_01",
                "name": "get_weather",
                "input": {"location": "SF"}
            }]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    let blocks = &ir.messages[0].content;
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        IrContentBlock::ToolCall { id, name, input } => {
            assert_eq!(id, "toolu_01");
            assert_eq!(name, "get_weather");
            assert_eq!(input["location"], "SF");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn claude_tool_result_content_block() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_01",
                "content": "72°F"
            }]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_call_id, "toolu_01");
            assert!(!is_error);
            assert_eq!(text_of(content), "72°F");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn claude_tool_result_with_error() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_02",
                "content": "API rate limit exceeded",
                "is_error": true
            }]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn claude_tool_result_nested_blocks() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_03",
                "content": [
                    {"type": "text", "text": "Result line 1"},
                    {"type": "text", "text": "Result line 2"}
                ]
            }]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            assert_eq!(content.len(), 2);
            assert_eq!(text_of(content), "Result line 1Result line 2");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn claude_thinking_block() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me think about this..."},
                {"type": "text", "text": "The answer is 42."}
            ]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].content.len(), 2);
    match &ir.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "Let me think about this..."),
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn claude_thinking_block_text_field() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "text": "Thinking via text field"}
            ]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "Thinking via text field"),
        _ => panic!("expected Thinking"),
    }
}

#[test]
fn claude_image_content_block() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "iVBOR..."
                }
            }]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBOR...");
        }
        _ => panic!("expected Image"),
    }
}

#[test]
fn claude_tool_definitions() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "name": "search",
            "description": "Search the web",
            "input_schema": {
                "type": "object",
                "properties": {"q": {"type": "string"}}
            }
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "search");
    assert_eq!(ir.tools[0].description, "Search the web");
    assert!(ir.tools[0].parameters.get("properties").is_some());
}

#[test]
fn claude_generation_config() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 4096,
        "temperature": 0.5,
        "top_p": 0.8,
        "top_k": 40,
        "stop_sequences": ["<end>", "STOP"],
        "messages": [{"role": "user", "content": "hi"}]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(4096));
    assert_eq!(ir.config.temperature, Some(0.5));
    assert_eq!(ir.config.top_p, Some(0.8));
    assert_eq!(ir.config.top_k, Some(40));
    assert_eq!(ir.config.stop_sequences, vec!["<end>", "STOP"]);
}

#[test]
fn claude_no_system_prompt() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert!(ir.system_prompt.is_none());
}

#[test]
fn claude_mixed_content_blocks() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "text", "text": "I'll look that up."},
                {"type": "tool_use", "id": "tu_1", "name": "search", "input": {"q": "rust"}}
            ]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].content.len(), 2);
    assert!(ir.messages[0].content[0].as_text().is_some());
    assert!(ir.messages[0].content[1].is_tool_call());
}

#[test]
fn claude_string_content_shorthand() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "simple string"}]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].text_content(), "simple string");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Gemini — request parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_basic_generate_content() {
    let v = json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": "Hello Gemini"}]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Hello Gemini");
}

#[test]
fn gemini_system_instruction() {
    let v = json!({
        "system_instruction": {
            "parts": [{"text": "You are a pirate."}]
        },
        "contents": [{"role": "user", "parts": [{"text": "Ahoy!"}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("You are a pirate."));
}

#[test]
fn gemini_model_role() {
    let v = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "What is AI?"}]},
            {"role": "model", "parts": [{"text": "AI is..."}]}
        ]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[1].text_content(), "AI is...");
}

#[test]
fn gemini_function_call_part() {
    let v = json!({
        "contents": [{
            "role": "model",
            "parts": [{
                "functionCall": {
                    "name": "get_weather",
                    "args": {"location": "Tokyo"}
                }
            }]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolCall { name, input, .. } => {
            assert_eq!(name, "get_weather");
            assert_eq!(input["location"], "Tokyo");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn gemini_function_call_empty_id() {
    let v = json!({
        "contents": [{
            "role": "model",
            "parts": [{"functionCall": {"name": "fn_a", "args": {}}}]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolCall { id, .. } => assert!(id.is_empty()),
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn gemini_function_response_part() {
    let v = json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": "get_weather",
                    "response": {"temp": 72, "unit": "F"}
                }
            }]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_call_id, "get_weather");
            assert!(!is_error);
            let text = text_of(content);
            let parsed: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(parsed["temp"], 72);
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn gemini_function_declarations() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "tools": [{
            "functionDeclarations": [{
                "name": "search",
                "description": "Web search",
                "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
            }]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "search");
    assert_eq!(ir.tools[0].description, "Web search");
}

#[test]
fn gemini_multiple_function_declarations_in_one_tool() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "tools": [{
            "functionDeclarations": [
                {"name": "fn_a", "description": "A", "parameters": {}},
                {"name": "fn_b", "description": "B", "parameters": {}}
            ]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.tools.len(), 2);
}

#[test]
fn gemini_generation_config_camel_case() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "generationConfig": {
            "maxOutputTokens": 1000,
            "temperature": 0.9,
            "topP": 0.95,
            "topK": 50,
            "stopSequences": ["END"]
        }
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(1000));
    assert_eq!(ir.config.temperature, Some(0.9));
    assert_eq!(ir.config.top_p, Some(0.95));
    assert_eq!(ir.config.top_k, Some(50));
    assert_eq!(ir.config.stop_sequences, vec!["END"]);
}

#[test]
fn gemini_generation_config_snake_case() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "generation_config": {
            "max_output_tokens": 500,
            "temperature": 0.3,
            "top_p": 0.8,
            "top_k": 20,
            "stop_sequences": ["STOP"]
        }
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(500));
    assert_eq!(ir.config.top_p, Some(0.8));
    assert_eq!(ir.config.top_k, Some(20));
    assert_eq!(ir.config.stop_sequences, vec!["STOP"]);
}

#[test]
fn gemini_inline_data_image() {
    let v = json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "inlineData": {
                    "mimeType": "image/jpeg",
                    "data": "base64data=="
                }
            }]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64data==");
        }
        _ => panic!("expected Image"),
    }
}

#[test]
fn gemini_no_model_field() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert!(ir.model.is_none());
}

#[test]
fn gemini_with_model_field() {
    let v = json!({
        "model": "gemini-1.5-pro",
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gemini-1.5-pro"));
}

#[test]
fn gemini_mixed_parts() {
    let v = json!({
        "contents": [{
            "role": "user",
            "parts": [
                {"text": "Describe this image:"},
                {"inlineData": {"mimeType": "image/png", "data": "abc123"}}
            ]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[0].content.len(), 2);
    assert!(ir.messages[0].content[0].as_text().is_some());
    matches!(&ir.messages[0].content[1], IrContentBlock::Image { .. });
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Kimi — request parsing (OpenAI-compatible + extensions)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_basic_chat_request() {
    let v = json!({
        "model": "moonshot-v1-8k",
        "messages": [
            {"role": "system", "content": "You are Kimi."},
            {"role": "user", "content": "Hello Kimi"}
        ]
    });
    let ir = parse(Dialect::Kimi, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("moonshot-v1-8k"));
    assert_eq!(ir.system_prompt.as_deref(), Some("You are Kimi."));
    assert_eq!(ir.messages.len(), 2);
}

#[test]
fn kimi_preserves_refs_metadata() {
    let v = json!({
        "model": "moonshot-v1-8k",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": [{"url": "https://example.com", "title": "Example"}]
    });
    let ir = parse(Dialect::Kimi, &v).unwrap();
    assert!(ir.metadata.contains_key("kimi_refs"));
}

#[test]
fn kimi_preserves_search_plus_metadata() {
    let v = json!({
        "model": "moonshot-v1-8k",
        "messages": [{"role": "user", "content": "hi"}],
        "search_plus": true
    });
    let ir = parse(Dialect::Kimi, &v).unwrap();
    assert!(ir.metadata.contains_key("kimi_search_plus"));
}

#[test]
fn kimi_tool_call_openai_compatible() {
    let v = json!({
        "model": "moonshot-v1-8k",
        "messages": [{
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "kimi_call_1",
                "type": "function",
                "function": {"name": "search", "arguments": "{\"q\":\"weather\"}"}
            }]
        }]
    });
    let ir = parse(Dialect::Kimi, &v).unwrap();
    let calls = ir.messages[0].tool_calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        IrContentBlock::ToolCall { id, name, .. } => {
            assert_eq!(id, "kimi_call_1");
            assert_eq!(name, "search");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn kimi_tool_result_openai_compatible() {
    let v = json!({
        "model": "moonshot-v1-8k",
        "messages": [
            {"role": "tool", "tool_call_id": "kimi_call_1", "content": "sunny, 25C"}
        ]
    });
    let ir = parse(Dialect::Kimi, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Tool);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { tool_call_id, .. } => {
            assert_eq!(tool_call_id, "kimi_call_1");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn kimi_generation_config() {
    let v = json!({
        "model": "moonshot-v1-128k",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.3,
        "max_tokens": 4096,
        "top_p": 0.7
    });
    let ir = parse(Dialect::Kimi, &v).unwrap();
    assert_eq!(ir.config.temperature, Some(0.3));
    assert_eq!(ir.config.max_tokens, Some(4096));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Codex — request parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_basic_request_with_input() {
    let v = json!({
        "model": "codex-mini",
        "input": "Fix the bug in main.rs"
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("codex-mini"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Fix the bug in main.rs");
}

#[test]
fn codex_instructions_as_system_prompt() {
    let v = json!({
        "model": "codex-mini",
        "instructions": "You are a code assistant.",
        "input": "Write hello world"
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert_eq!(
        ir.system_prompt.as_deref(),
        Some("You are a code assistant.")
    );
}

#[test]
fn codex_items_with_message() {
    let v = json!({
        "model": "codex-mini",
        "items": [
            {"type": "message", "role": "user", "content": "explain this"},
            {"type": "message", "role": "assistant", "content": "Sure, it does..."}
        ]
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.messages.len(), 2);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[1].text_content(), "Sure, it does...");
}

#[test]
fn codex_items_function_call() {
    let v = json!({
        "model": "codex-mini",
        "items": [{
            "type": "function_call",
            "call_id": "fc_1",
            "name": "read_file",
            "arguments": "{\"path\":\"src/main.rs\"}"
        }]
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.messages.len(), 1);
    let msg = &ir.messages[0];
    assert_eq!(msg.role, IrRole::Assistant);
    match &msg.content[0] {
        IrContentBlock::ToolCall { id, name, input } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "src/main.rs");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn codex_items_function_call_malformed_args() {
    let v = json!({
        "model": "codex-mini",
        "items": [{
            "type": "function_call",
            "call_id": "fc_2",
            "name": "run_cmd",
            "arguments": "not-json"
        }]
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolCall { input, .. } => {
            // Should produce null since from_str fails
            assert!(input.is_null());
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn codex_tool_definitions_openai_style() {
    let v = json!({
        "model": "codex-mini",
        "input": "hi",
        "tools": [{
            "type": "function",
            "function": {
                "name": "shell",
                "description": "Run shell command",
                "parameters": {"type": "object"}
            }
        }]
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "shell");
}

#[test]
fn codex_no_input_no_items() {
    let v = json!({"model": "codex-mini"});
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn codex_both_input_and_items() {
    let v = json!({
        "model": "codex-mini",
        "input": "Do something",
        "items": [{"type": "message", "role": "assistant", "content": "OK"}]
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    // input comes first, then items
    assert_eq!(ir.messages.len(), 2);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Do something");
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Copilot — request parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_basic_chat_request() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You are a Copilot extension."},
            {"role": "user", "content": "Explain this code"}
        ]
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4o"));
    assert_eq!(ir.messages.len(), 2);
}

#[test]
fn copilot_preserves_references() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "explain"}],
        "references": [
            {"type": "file", "id": "main.rs", "content": "fn main() {}"}
        ]
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_references"));
    let refs = ir.metadata.get("copilot_references").unwrap();
    assert!(refs.is_array());
}

#[test]
fn copilot_preserves_confirmations() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "do it"}],
        "confirmations": [{"id": "conf_1", "accepted": true}]
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_confirmations"));
}

#[test]
fn copilot_preserves_agent_mode() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "agent_mode": "code-review"
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_agent_mode"));
    assert_eq!(
        ir.metadata["copilot_agent_mode"].as_str(),
        Some("code-review")
    );
}

#[test]
fn copilot_tool_call_openai_compatible() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "cp_call_1",
                "type": "function",
                "function": {"name": "read_file", "arguments": "{\"path\":\"lib.rs\"}"}
            }]
        }]
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    let calls = ir.messages[0].tool_calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        IrContentBlock::ToolCall { id, name, .. } => {
            assert_eq!(id, "cp_call_1");
            assert_eq!(name, "read_file");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn copilot_tool_result_openai_compatible() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "tool", "tool_call_id": "cp_call_1", "content": "fn main() {}"}
        ]
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { tool_call_id, .. } => {
            assert_eq!(tool_call_id, "cp_call_1");
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn copilot_tool_definitions_openai_style() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "run_tests",
                "description": "Run test suite",
                "parameters": {"type": "object"}
            }
        }]
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "run_tests");
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Response parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_response_basic() {
    let v = json!({
        "id": "chatcmpl-abc",
        "model": "gpt-4o",
        "choices": [{
            "message": {"role": "assistant", "content": "Hello!"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.id.as_deref(), Some("chatcmpl-abc"));
    assert_eq!(ir.model.as_deref(), Some("gpt-4o"));
    assert_eq!(ir.text_content(), "Hello!");
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
    let usage = ir.usage.unwrap();
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.total_tokens, 15);
}

#[test]
fn openai_response_tool_calls() {
    let v = json!({
        "id": "chatcmpl-def",
        "model": "gpt-4o",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_789",
                    "type": "function",
                    "function": {"name": "calc", "arguments": "{\"expr\":\"2+2\"}"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::ToolUse));
    assert!(ir.has_tool_calls());
    let calls = ir.tool_calls();
    assert_eq!(calls.len(), 1);
}

#[test]
fn openai_response_max_tokens() {
    let v = json!({
        "id": "chatcmpl-ghi",
        "choices": [{"message": {"role": "assistant", "content": "partial..."}, "finish_reason": "length"}]
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::MaxTokens));
}

#[test]
fn openai_response_content_filter() {
    let v = json!({
        "id": "chatcmpl-jkl",
        "choices": [{"message": {"role": "assistant", "content": ""}, "finish_reason": "content_filter"}]
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::ContentFilter));
}

#[test]
fn openai_response_unknown_finish_reason() {
    let v = json!({
        "id": "chatcmpl-mno",
        "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "custom_reason"}]
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(
        ir.stop_reason,
        Some(IrStopReason::Other("custom_reason".into()))
    );
}

#[test]
fn claude_response_basic() {
    let v = json!({
        "id": "msg_abc",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "Hello from Claude"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 10}
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.id.as_deref(), Some("msg_abc"));
    assert_eq!(ir.text_content(), "Hello from Claude");
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
    let usage = ir.usage.unwrap();
    assert_eq!(usage.input_tokens, 20);
    assert_eq!(usage.output_tokens, 10);
    assert_eq!(usage.total_tokens, 30);
}

#[test]
fn claude_response_tool_use() {
    let v = json!({
        "id": "msg_def",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {"type": "text", "text": "Let me check."},
            {"type": "tool_use", "id": "tu_1", "name": "search", "input": {"q": "test"}}
        ],
        "stop_reason": "tool_use"
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::ToolUse));
    assert!(ir.has_tool_calls());
    assert_eq!(ir.content.len(), 2);
}

#[test]
fn claude_response_stop_sequence() {
    let v = json!({
        "id": "msg_ghi",
        "content": [{"type": "text", "text": "done"}],
        "stop_reason": "stop_sequence"
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::StopSequence));
}

#[test]
fn claude_response_max_tokens() {
    let v = json!({
        "id": "msg_jkl",
        "content": [{"type": "text", "text": "incomplete..."}],
        "stop_reason": "max_tokens"
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::MaxTokens));
}

#[test]
fn claude_response_cache_tokens() {
    let v = json!({
        "id": "msg_cache",
        "content": [{"type": "text", "text": "ok"}],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 80,
            "cache_creation_input_tokens": 20
        }
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    let u = ir.usage.unwrap();
    assert_eq!(u.cache_read_tokens, 80);
    assert_eq!(u.cache_write_tokens, 20);
}

#[test]
fn gemini_response_basic() {
    let v = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "Hello from Gemini"}]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 5,
            "candidatesTokenCount": 10,
            "totalTokenCount": 15
        }
    });
    let ir = parse_response(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.text_content(), "Hello from Gemini");
    let u = ir.usage.unwrap();
    assert_eq!(u.input_tokens, 5);
    assert_eq!(u.output_tokens, 10);
    assert_eq!(u.total_tokens, 15);
}

#[test]
fn gemini_response_function_call() {
    let v = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"functionCall": {"name": "search", "args": {"q": "rust"}}}]
            }
        }]
    });
    let ir = parse_response(Dialect::Gemini, &v).unwrap();
    assert!(ir.has_tool_calls());
}

#[test]
fn gemini_response_cached_content_tokens() {
    let v = json!({
        "candidates": [{"content": {"parts": [{"text": "ok"}]}}],
        "usageMetadata": {
            "promptTokenCount": 100,
            "candidatesTokenCount": 20,
            "totalTokenCount": 120,
            "cachedContentTokenCount": 80
        }
    });
    let ir = parse_response(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.usage.unwrap().cache_read_tokens, 80);
}

#[test]
fn kimi_response_fallback_openai() {
    let v = json!({
        "id": "kimi_resp_1",
        "model": "moonshot-v1-8k",
        "choices": [{
            "message": {"role": "assistant", "content": "From Kimi"},
            "finish_reason": "stop"
        }]
    });
    let ir = parse_response(Dialect::Kimi, &v).unwrap();
    assert_eq!(ir.text_content(), "From Kimi");
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
}

#[test]
fn codex_response_fallback_openai() {
    let v = json!({
        "id": "codex_resp_1",
        "choices": [{
            "message": {"role": "assistant", "content": "From Codex"},
            "finish_reason": "stop"
        }]
    });
    let ir = parse_response(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.text_content(), "From Codex");
}

#[test]
fn copilot_response_fallback_openai() {
    let v = json!({
        "id": "cp_resp_1",
        "choices": [{
            "message": {"role": "assistant", "content": "From Copilot"},
            "finish_reason": "stop"
        }]
    });
    let ir = parse_response(Dialect::Copilot, &v).unwrap();
    assert_eq!(ir.text_content(), "From Copilot");
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Streaming response format tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_streaming_chunk_response() {
    // OpenAI streaming uses delta instead of message
    let chunk = json!({
        "id": "chatcmpl-stream",
        "model": "gpt-4o",
        "choices": [{
            "message": {"role": "assistant", "content": "stream chunk"},
            "finish_reason": null
        }]
    });
    let ir = parse_response(Dialect::OpenAi, &chunk).unwrap();
    assert_eq!(ir.text_content(), "stream chunk");
    assert!(ir.stop_reason.is_none());
}

#[test]
fn openai_streaming_final_chunk() {
    let chunk = json!({
        "id": "chatcmpl-stream",
        "choices": [{
            "message": {"role": "assistant", "content": "done"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 50, "total_tokens": 60}
    });
    let ir = parse_response(Dialect::OpenAi, &chunk).unwrap();
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
    assert!(ir.usage.is_some());
}

#[test]
fn claude_streaming_content_block_delta() {
    // Claude streaming events can be parsed as response when aggregated
    let aggregated = json!({
        "id": "msg_stream",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "streamed text"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 5, "output_tokens": 8}
    });
    let ir = parse_response(Dialect::Claude, &aggregated).unwrap();
    assert_eq!(ir.text_content(), "streamed text");
}

#[test]
fn gemini_streaming_chunk() {
    let chunk = json!({
        "candidates": [{
            "content": {"parts": [{"text": "Gemini stream piece"}]}
        }]
    });
    let ir = parse_response(Dialect::Gemini, &chunk).unwrap();
    assert_eq!(ir.text_content(), "Gemini stream piece");
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Error handling for malformed requests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_not_json_object() {
    let v = json!("just a string");
    let err = parse(Dialect::OpenAi, &v).unwrap_err();
    assert_eq!(err.dialect, Dialect::OpenAi);
    assert!(err.message.contains("expected JSON object"));
}

#[test]
fn claude_not_json_object() {
    let v = json!(42);
    let err = parse(Dialect::Claude, &v).unwrap_err();
    assert_eq!(err.dialect, Dialect::Claude);
    assert!(err.message.contains("expected JSON object"));
}

#[test]
fn gemini_not_json_object() {
    let v = json!([1, 2, 3]);
    let err = parse(Dialect::Gemini, &v).unwrap_err();
    assert_eq!(err.dialect, Dialect::Gemini);
}

#[test]
fn codex_not_json_object() {
    let v = json!(null);
    let err = parse(Dialect::Codex, &v).unwrap_err();
    assert_eq!(err.dialect, Dialect::Codex);
}

#[test]
fn kimi_not_json_object() {
    let v = json!(true);
    let err = parse(Dialect::Kimi, &v).unwrap_err();
    assert_eq!(err.dialect, Dialect::Kimi);
}

#[test]
fn copilot_not_json_object() {
    let v = json!(false);
    let err = parse(Dialect::Copilot, &v).unwrap_err();
    assert_eq!(err.dialect, Dialect::Copilot);
}

#[test]
fn openai_empty_object_parses_ok() {
    let v = json!({});
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.model.is_none());
    assert!(ir.messages.is_empty());
}

#[test]
fn claude_empty_object_parses_ok() {
    let v = json!({});
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn gemini_empty_object_parses_ok() {
    let v = json!({});
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn openai_messages_not_array() {
    let v = json!({"model": "gpt-4", "messages": "not an array"});
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn gemini_contents_not_array() {
    let v = json!({"contents": "not an array"});
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn openai_tools_not_array() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": "not an array"
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.tools.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Edge cases and ambiguous formats
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_unknown_role_defaults_to_user() {
    let v = json!({
        "messages": [{"role": "unknown_role", "content": "hi"}]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn openai_missing_role_defaults_to_user() {
    let v = json!({
        "messages": [{"content": "no role"}]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn claude_unknown_role_defaults_to_user() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "custom", "content": "hi"}]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn gemini_unknown_role_defaults_to_user() {
    let v = json!({
        "contents": [{"role": "something", "parts": [{"text": "hi"}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn gemini_missing_role_defaults_to_user() {
    let v = json!({
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn claude_unknown_content_block_type_skipped() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "video", "url": "https://example.com/video.mp4"},
                {"type": "text", "text": "Describe this"}
            ]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    // Unknown type "video" should be skipped, only "text" remains
    assert_eq!(ir.messages[0].content.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "Describe this");
}

#[test]
fn gemini_unknown_part_type_skipped() {
    let v = json!({
        "contents": [{
            "role": "user",
            "parts": [
                {"videoMetadata": {"uri": "gs://bucket/video.mp4"}},
                {"text": "Describe this"}
            ]
        }]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[0].content.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "Describe this");
}

#[test]
fn openai_system_prompt_extracted() {
    let v = json!({
        "messages": [
            {"role": "system", "content": "system prompt here"},
            {"role": "user", "content": "hi"}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("system prompt here"));
}

#[test]
fn openai_second_system_not_overwritten() {
    let v = json!({
        "messages": [
            {"role": "system", "content": "first system"},
            {"role": "system", "content": "second system"}
        ]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    // The first system message sets system_prompt
    assert_eq!(ir.system_prompt.as_deref(), Some("first system"));
}

#[test]
fn openai_tool_call_empty_arguments() {
    let v = json!({
        "messages": [{
            "role": "assistant",
            "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "no_args"}
            }]
        }]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    let calls = ir.messages[0].tool_calls();
    match &calls[0] {
        IrContentBlock::ToolCall { input, .. } => {
            // Arguments default to "{}" string parsed as empty object
            assert!(input.is_object());
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn claude_content_block_missing_text_field() {
    // A text block without a "text" field should be skipped
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [{"type": "text"}]
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    // filter_map returns None for text block without text field
    assert!(ir.messages[0].content.is_empty());
}

#[test]
fn openai_tool_with_no_function_key() {
    let v = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function"}]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.tools.is_empty());
}

#[test]
fn gemini_tools_empty_function_declarations() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "tools": [{"functionDeclarations": []}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert!(ir.tools.is_empty());
}

#[test]
fn openai_response_empty_choices() {
    let v = json!({
        "id": "chatcmpl-empty",
        "choices": []
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert!(ir.content.is_empty());
}

#[test]
fn gemini_response_empty_candidates() {
    let v = json!({
        "candidates": []
    });
    let ir = parse_response(Dialect::Gemini, &v).unwrap();
    assert!(ir.content.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Roundtrip: parse → serialize → parse consistency
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_roundtrip_basic() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "Be helpful"},
            {"role": "user", "content": "Hello"}
        ],
        "temperature": 0.7
    });
    let reg = registry();
    let ir1 = reg.parse(Dialect::OpenAi, &v).unwrap();
    let json2 = reg.serialize(Dialect::OpenAi, &ir1).unwrap();
    let ir2 = reg.parse(Dialect::OpenAi, &json2).unwrap();
    assert_eq!(ir1.model, ir2.model);
    assert_eq!(ir1.messages.len(), ir2.messages.len());
    assert_eq!(ir1.system_prompt, ir2.system_prompt);
}

#[test]
fn claude_roundtrip_basic() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "system": "Be concise",
        "messages": [{"role": "user", "content": "Hi"}]
    });
    let reg = registry();
    let ir1 = reg.parse(Dialect::Claude, &v).unwrap();
    let json2 = reg.serialize(Dialect::Claude, &ir1).unwrap();
    let ir2 = reg.parse(Dialect::Claude, &json2).unwrap();
    assert_eq!(ir1.model, ir2.model);
    assert_eq!(ir1.system_prompt, ir2.system_prompt);
    assert_eq!(ir1.messages.len(), ir2.messages.len());
}

#[test]
fn gemini_roundtrip_basic() {
    let v = json!({
        "model": "gemini-1.5-pro",
        "system_instruction": {"parts": [{"text": "You are a pirate"}]},
        "contents": [{"role": "user", "parts": [{"text": "Ahoy!"}]}],
        "generationConfig": {"temperature": 0.5, "maxOutputTokens": 100}
    });
    let reg = registry();
    let ir1 = reg.parse(Dialect::Gemini, &v).unwrap();
    let json2 = reg.serialize(Dialect::Gemini, &ir1).unwrap();
    let ir2 = reg.parse(Dialect::Gemini, &json2).unwrap();
    assert_eq!(ir1.model, ir2.model);
    assert_eq!(ir1.system_prompt, ir2.system_prompt);
}

#[test]
fn openai_roundtrip_with_tools() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [{
            "type": "function",
            "function": {"name": "get_weather", "description": "Get weather", "parameters": {"type": "object"}}
        }]
    });
    let reg = registry();
    let ir1 = reg.parse(Dialect::OpenAi, &v).unwrap();
    let json2 = reg.serialize(Dialect::OpenAi, &ir1).unwrap();
    let ir2 = reg.parse(Dialect::OpenAi, &json2).unwrap();
    assert_eq!(ir1.tools.len(), ir2.tools.len());
    assert_eq!(ir1.tools[0].name, ir2.tools[0].name);
}

#[test]
fn kimi_roundtrip_preserves_metadata() {
    let v = json!({
        "model": "moonshot-v1-8k",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": [{"url": "https://example.com"}]
    });
    let reg = registry();
    let ir1 = reg.parse(Dialect::Kimi, &v).unwrap();
    let json2 = reg.serialize(Dialect::Kimi, &ir1).unwrap();
    assert!(json2.get("refs").is_some());
}

#[test]
fn copilot_roundtrip_preserves_metadata() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "references": [{"type": "file", "id": "main.rs"}],
        "agent_mode": "code"
    });
    let reg = registry();
    let ir1 = reg.parse(Dialect::Copilot, &v).unwrap();
    let json2 = reg.serialize(Dialect::Copilot, &ir1).unwrap();
    assert!(json2.get("references").is_some());
    assert!(json2.get("agent_mode").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Cross-dialect normalization to IR
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_simple_text_normalizes() {
    let openai = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let claude = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let gemini = json!({
        "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
    });

    let ir_o = parse(Dialect::OpenAi, &openai).unwrap();
    let ir_c = parse(Dialect::Claude, &claude).unwrap();
    let ir_g = parse(Dialect::Gemini, &gemini).unwrap();

    assert_eq!(ir_o.messages[0].role, ir_c.messages[0].role);
    assert_eq!(ir_o.messages[0].role, ir_g.messages[0].role);
    assert_eq!(ir_o.messages[0].text_content(), "Hello");
    assert_eq!(ir_c.messages[0].text_content(), "Hello");
    assert_eq!(ir_g.messages[0].text_content(), "Hello");
}

#[test]
fn cross_dialect_system_prompt_normalizes() {
    let openai = json!({
        "messages": [
            {"role": "system", "content": "Be helpful"},
            {"role": "user", "content": "hi"}
        ]
    });
    let claude = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "system": "Be helpful",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let gemini = json!({
        "system_instruction": {"parts": [{"text": "Be helpful"}]},
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
    });

    let ir_o = parse(Dialect::OpenAi, &openai).unwrap();
    let ir_c = parse(Dialect::Claude, &claude).unwrap();
    let ir_g = parse(Dialect::Gemini, &gemini).unwrap();

    assert_eq!(ir_o.system_prompt.as_deref(), Some("Be helpful"));
    assert_eq!(ir_c.system_prompt.as_deref(), Some("Be helpful"));
    assert_eq!(ir_g.system_prompt.as_deref(), Some("Be helpful"));
}

#[test]
fn cross_dialect_tool_definition_normalizes() {
    let openai = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "calc", "description": "Calculate", "parameters": {"type": "object"}}}]
    });
    let claude = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "calc", "description": "Calculate", "input_schema": {"type": "object"}}]
    });
    let gemini = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "tools": [{"functionDeclarations": [{"name": "calc", "description": "Calculate", "parameters": {"type": "object"}}]}]
    });

    let ir_o = parse(Dialect::OpenAi, &openai).unwrap();
    let ir_c = parse(Dialect::Claude, &claude).unwrap();
    let ir_g = parse(Dialect::Gemini, &gemini).unwrap();

    assert_eq!(ir_o.tools[0].name, "calc");
    assert_eq!(ir_c.tools[0].name, "calc");
    assert_eq!(ir_g.tools[0].name, "calc");
    assert_eq!(ir_o.tools[0].description, ir_c.tools[0].description);
    assert_eq!(ir_c.tools[0].description, ir_g.tools[0].description);
}

#[test]
fn all_openai_compatible_dialects_parse_same_basic_input() {
    let v = json!({
        "model": "some-model",
        "messages": [
            {"role": "system", "content": "hello"},
            {"role": "user", "content": "world"}
        ],
        "temperature": 0.5,
        "max_tokens": 100
    });

    for dialect in [Dialect::OpenAi, Dialect::Kimi, Dialect::Copilot] {
        let ir = parse(dialect, &v).unwrap();
        assert_eq!(ir.model.as_deref(), Some("some-model"));
        assert_eq!(ir.system_prompt.as_deref(), Some("hello"));
        assert_eq!(ir.messages.len(), 2);
        assert_eq!(ir.config.temperature, Some(0.5));
        assert_eq!(ir.config.max_tokens, Some(100));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Registry meta tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_has_all_six_dialects() {
    let reg = registry();
    assert_eq!(reg.len(), 6);
    for d in Dialect::all() {
        assert!(reg.get(*d).is_some(), "missing dialect: {:?}", d);
    }
}

#[test]
fn registry_list_dialects_deterministic() {
    let reg = registry();
    let dialects = reg.list_dialects();
    assert_eq!(dialects.len(), 6);
    // BTreeMap gives deterministic order
    let d2 = reg.list_dialects();
    assert_eq!(dialects, d2);
}

#[test]
fn registry_supports_all_pairs() {
    let reg = registry();
    for from in Dialect::all() {
        for to in Dialect::all() {
            assert!(reg.supports_pair(*from, *to));
        }
    }
}

#[test]
fn registry_parse_unregistered_dialect_errors() {
    let reg = DialectRegistry::new(); // empty
    let v = json!({"messages": []});
    let err = reg.parse(Dialect::OpenAi, &v).unwrap_err();
    assert!(err.message.contains("not registered"));
}

#[test]
fn dialect_display_labels() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. IR type unit tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_text_helper() {
    let msg = IrMessage::text(IrRole::User, "Hello world");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.text_content(), "Hello world");
}

#[test]
fn ir_message_tool_calls_helper() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "before".into(),
            },
            IrContentBlock::ToolCall {
                id: "t1".into(),
                name: "fn".into(),
                input: json!({}),
            },
        ],
    );
    assert_eq!(msg.tool_calls().len(), 1);
}

#[test]
fn ir_request_builder_chain() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("gpt-4")
        .with_system_prompt("Be helpful")
        .with_tool(IrToolDefinition {
            name: "search".into(),
            description: "search".into(),
            parameters: json!({}),
        });
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.system_prompt.as_deref(), Some("Be helpful"));
    assert_eq!(ir.tools.len(), 1);
}

#[test]
fn ir_response_builder_chain() {
    let ir = IrResponse::text("Hello")
        .with_id("resp_1")
        .with_model("gpt-4")
        .with_stop_reason(IrStopReason::EndTurn)
        .with_usage(IrUsage::from_io(10, 5));
    assert_eq!(ir.id.as_deref(), Some("resp_1"));
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
    assert_eq!(ir.usage.unwrap().total_tokens, 15);
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(10, 5);
    let b = IrUsage::from_io(20, 15);
    let c = a.merge(b);
    assert_eq!(c.input_tokens, 30);
    assert_eq!(c.output_tokens, 20);
    assert_eq!(c.total_tokens, 50);
}

#[test]
fn ir_content_block_as_text() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    assert_eq!(block.as_text(), Some("hello"));
    let tool = IrContentBlock::ToolCall {
        id: "".into(),
        name: "".into(),
        input: json!(null),
    };
    assert_eq!(tool.as_text(), None);
}

#[test]
fn ir_content_block_is_tool_call() {
    let tc = IrContentBlock::ToolCall {
        id: "x".into(),
        name: "y".into(),
        input: json!({}),
    };
    assert!(tc.is_tool_call());
    assert!(!tc.is_tool_result());
}

#[test]
fn ir_content_block_is_tool_result() {
    let tr = IrContentBlock::ToolResult {
        tool_call_id: "x".into(),
        content: vec![],
        is_error: false,
    };
    assert!(tr.is_tool_result());
    assert!(!tr.is_tool_call());
}

#[test]
fn ir_request_system_message_accessor() {
    let ir = IrRequest::new(vec![
        IrMessage::text(IrRole::System, "system msg"),
        IrMessage::text(IrRole::User, "user msg"),
    ]);
    let sys = ir.system_message().unwrap();
    assert_eq!(sys.role, IrRole::System);
    assert_eq!(sys.text_content(), "system msg");
}

#[test]
fn ir_request_all_tool_calls_across_messages() {
    let ir = IrRequest::new(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "t1".into(),
                name: "a".into(),
                input: json!({}),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "t2".into(),
                name: "b".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(ir.all_tool_calls().len(), 2);
}

#[test]
fn ir_response_has_tool_calls() {
    let ir = IrResponse::new(vec![IrContentBlock::ToolCall {
        id: "x".into(),
        name: "y".into(),
        input: json!({}),
    }]);
    assert!(ir.has_tool_calls());

    let ir2 = IrResponse::text("just text");
    assert!(!ir2.has_tool_calls());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Serde roundtrip tests for IR types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_serde_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn ir_content_block_text_serde() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn ir_content_block_tool_call_serde() {
    let block = IrContentBlock::ToolCall {
        id: "tc1".into(),
        name: "search".into(),
        input: json!({"q": "test"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_call");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn ir_content_block_tool_result_serde() {
    let block = IrContentBlock::ToolResult {
        tool_call_id: "tc1".into(),
        content: vec![IrContentBlock::Text {
            text: "result".into(),
        }],
        is_error: true,
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["is_error"], true);
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn ir_stop_reason_serde_roundtrip() {
    let reasons = vec![
        IrStopReason::EndTurn,
        IrStopReason::StopSequence,
        IrStopReason::MaxTokens,
        IrStopReason::ToolUse,
        IrStopReason::ContentFilter,
        IrStopReason::Other("custom".into()),
    ];
    for r in reasons {
        let json = serde_json::to_string(&r).unwrap();
        let back: IrStopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}

#[test]
fn ir_generation_config_default() {
    let cfg = IrGenerationConfig::default();
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.stop_sequences.is_empty());
    assert!(cfg.extra.is_empty());
}

#[test]
fn ir_message_metadata_serde() {
    let mut msg = IrMessage::text(IrRole::User, "hi");
    msg.metadata.insert("key".into(), json!("value"));
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["metadata"]["key"], "value");
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back.metadata["key"], "value");
}

#[test]
fn ir_message_empty_metadata_not_serialized() {
    let msg = IrMessage::text(IrRole::User, "hi");
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json.get("metadata").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Dialect error type tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_error_display() {
    let err = DialectError {
        dialect: Dialect::OpenAi,
        message: "bad input".into(),
    };
    let s = format!("{}", err);
    assert!(s.contains("OpenAI"));
    assert!(s.contains("bad input"));
}

#[test]
fn dialect_error_is_std_error() {
    let err = DialectError {
        dialect: Dialect::Claude,
        message: "test".into(),
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn dialect_error_equality() {
    let e1 = DialectError {
        dialect: Dialect::Gemini,
        message: "error".into(),
    };
    let e2 = DialectError {
        dialect: Dialect::Gemini,
        message: "error".into(),
    };
    assert_eq!(e1, e2);
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Complex real-world scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_full_tool_use_conversation() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You have access to tools."},
            {"role": "user", "content": "What's the weather in Paris?"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}
                }]
            },
            {"role": "tool", "tool_call_id": "call_abc", "content": "15°C, partly cloudy"},
            {"role": "assistant", "content": "The weather in Paris is 15°C, partly cloudy."}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }
        }],
        "temperature": 0.0
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages.len(), 5);
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.config.temperature, Some(0.0));
    // Verify tool call
    let tc_msg = &ir.messages[2];
    assert_eq!(tc_msg.role, IrRole::Assistant);
    assert_eq!(tc_msg.tool_calls().len(), 1);
    // Verify tool result
    let tr_msg = &ir.messages[3];
    assert_eq!(tr_msg.role, IrRole::Tool);
    // Final response
    assert_eq!(ir.messages[4].role, IrRole::Assistant);
    assert!(ir.messages[4].text_content().contains("partly cloudy"));
}

#[test]
fn claude_full_tool_use_conversation() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 4096,
        "system": "You can use tools.",
        "messages": [
            {"role": "user", "content": "Search for Rust docs"},
            {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "I'll search for that."},
                    {"type": "tool_use", "id": "tu_1", "name": "web_search", "input": {"query": "Rust docs"}}
                ]
            },
            {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "tu_1",
                    "content": "Found: doc.rust-lang.org"
                }]
            },
            {"role": "assistant", "content": "I found the Rust docs at doc.rust-lang.org"}
        ],
        "tools": [{
            "name": "web_search",
            "description": "Search the web",
            "input_schema": {"type": "object", "properties": {"query": {"type": "string"}}}
        }]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages.len(), 4);
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.system_prompt.as_deref(), Some("You can use tools."));

    // Verify mixed content assistant message
    let asst = &ir.messages[1];
    assert_eq!(asst.content.len(), 2);
    assert!(asst.content[0].as_text().is_some());
    assert!(asst.content[1].is_tool_call());

    // Verify tool result
    assert!(ir.messages[2].content[0].is_tool_result());
}

#[test]
fn gemini_full_function_calling_conversation() {
    let v = json!({
        "model": "gemini-1.5-pro",
        "system_instruction": {"parts": [{"text": "You can call functions."}]},
        "contents": [
            {"role": "user", "parts": [{"text": "Calculate 10 factorial"}]},
            {"role": "model", "parts": [{"functionCall": {"name": "calc", "args": {"expr": "10!"}}}]},
            {"role": "user", "parts": [{"functionResponse": {"name": "calc", "response": {"result": 3628800}}}]},
            {"role": "model", "parts": [{"text": "10! = 3,628,800"}]}
        ],
        "tools": [{"functionDeclarations": [{"name": "calc", "description": "Calculate expression", "parameters": {"type": "object"}}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages.len(), 4);
    assert!(ir.messages[1].tool_calls().len() == 1);
    assert!(ir.messages[2].content[0].is_tool_result());
    assert!(ir.messages[3].text_content().contains("3,628,800"));
}

#[test]
fn codex_instructions_plus_items_complex() {
    let v = json!({
        "model": "codex-mini",
        "instructions": "You are a coding assistant",
        "input": "Read the Cargo.toml file",
        "items": [
            {"type": "function_call", "call_id": "fc_1", "name": "read_file", "arguments": "{\"path\":\"Cargo.toml\"}"},
            {"type": "message", "role": "assistant", "content": "Here is the contents of Cargo.toml..."}
        ],
        "tools": [{
            "type": "function",
            "function": {"name": "read_file", "description": "Read a file", "parameters": {"type": "object"}}
        }]
    });
    let ir = parse(Dialect::Codex, &v).unwrap();
    assert_eq!(
        ir.system_prompt.as_deref(),
        Some("You are a coding assistant")
    );
    assert_eq!(ir.messages.len(), 3); // input + function_call + message
    assert_eq!(ir.tools.len(), 1);
}

#[test]
fn copilot_complex_with_references_and_tools() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You are a Copilot extension for code review."},
            {"role": "user", "content": "Review this code"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "cp_1",
                    "type": "function",
                    "function": {"name": "get_diff", "arguments": "{\"pr\":42}"}
                }]
            },
            {"role": "tool", "tool_call_id": "cp_1", "content": "diff --git a/main.rs ..."}
        ],
        "tools": [{
            "type": "function",
            "function": {"name": "get_diff", "description": "Get PR diff", "parameters": {"type": "object"}}
        }],
        "references": [{"type": "git_diff", "id": "pr_42"}],
        "agent_mode": "code-review"
    });
    let ir = parse(Dialect::Copilot, &v).unwrap();
    assert_eq!(ir.messages.len(), 4);
    assert_eq!(ir.tools.len(), 1);
    assert!(ir.metadata.contains_key("copilot_references"));
    assert!(ir.metadata.contains_key("copilot_agent_mode"));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Additional edge cases for full coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_tool_definition_missing_description() {
    let v = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "fn_a", "parameters": {}}}]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].description, "");
}

#[test]
fn claude_tool_definition_missing_description() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "fn_a", "input_schema": {}}]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.tools[0].description, "");
}

#[test]
fn gemini_function_declaration_missing_description() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "tools": [{"functionDeclarations": [{"name": "fn_a", "parameters": {}}]}]
    });
    let ir = parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.tools[0].description, "");
}

#[test]
fn openai_tool_definition_missing_parameters() {
    let v = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "fn_a", "description": "A"}}]
    });
    let ir = parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.tools[0].parameters.is_object());
}

#[test]
fn claude_tool_definition_missing_input_schema() {
    let v = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "fn_a", "description": "A"}]
    });
    let ir = parse(Dialect::Claude, &v).unwrap();
    assert!(ir.tools[0].parameters.is_object());
}

#[test]
fn openai_response_no_usage() {
    let v = json!({
        "id": "resp",
        "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert!(ir.usage.is_none());
}

#[test]
fn claude_response_no_usage() {
    let v = json!({
        "id": "resp",
        "content": [{"type": "text", "text": "ok"}],
        "stop_reason": "end_turn"
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    assert!(ir.usage.is_none());
}

#[test]
fn gemini_response_no_usage() {
    let v = json!({
        "candidates": [{"content": {"parts": [{"text": "ok"}]}}]
    });
    let ir = parse_response(Dialect::Gemini, &v).unwrap();
    assert!(ir.usage.is_none());
}

#[test]
fn openai_response_multiple_tool_calls() {
    let v = json!({
        "id": "resp",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "a", "arguments": "{}"}},
                    {"id": "c2", "type": "function", "function": {"name": "b", "arguments": "{}"}}
                ]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let ir = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.tool_calls().len(), 2);
}

#[test]
fn claude_response_no_content() {
    let v = json!({
        "id": "resp",
        "model": "claude-sonnet-4-20250514",
        "stop_reason": "end_turn"
    });
    let ir = parse_response(Dialect::Claude, &v).unwrap();
    assert!(ir.content.is_empty());
}

#[test]
fn openai_response_not_json_object() {
    let v = json!("string");
    assert!(parse_response(Dialect::OpenAi, &v).is_none());
}

#[test]
fn claude_response_not_json_object() {
    let v = json!(42);
    assert!(parse_response(Dialect::Claude, &v).is_none());
}

#[test]
fn gemini_response_not_json_object() {
    let v = json!([]);
    assert!(parse_response(Dialect::Gemini, &v).is_none());
}
