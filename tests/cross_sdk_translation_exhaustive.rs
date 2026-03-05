// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exhaustive cross-SDK translation integration tests.
//!
//! Verifies that requests can be correctly translated between all SDK pairs
//! at both the JSON level (via `Mapper` trait) and the IR level (via
//! `IrMapper` trait).

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeToOpenAiMapper, DialectRequest, GeminiToOpenAiMapper, IrMapper,
    MapError, Mapper, OpenAiClaudeIrMapper, OpenAiGeminiIrMapper, OpenAiToClaudeMapper,
    OpenAiToGeminiMapper, default_ir_mapper,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn openai_chat_request() -> serde_json::Value {
    json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "max_tokens": 1024
    })
}

fn openai_chat_with_system() -> serde_json::Value {
    json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hi"}
        ],
        "max_tokens": 1024
    })
}

fn claude_messages_request() -> serde_json::Value {
    json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    })
}

fn claude_with_system() -> serde_json::Value {
    json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 2048,
        "system": "You are a helpful assistant.",
        "messages": [
            {"role": "user", "content": "Hi"}
        ]
    })
}

fn gemini_request() -> serde_json::Value {
    json!({
        "model": "gemini-pro",
        "contents": [
            {"role": "user", "parts": [{"text": "Hello"}]}
        ]
    })
}

fn gemini_with_system() -> serde_json::Value {
    json!({
        "model": "gemini-pro",
        "system_instruction": {
            "parts": [{"text": "You are a helpful assistant."}]
        },
        "contents": [
            {"role": "user", "parts": [{"text": "Hi"}]}
        ]
    })
}

fn simple_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn tool_call_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "NYC"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F, sunny".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 72°F and sunny in NYC."),
    ])
}

fn thinking_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve this puzzle"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ),
    ])
}

fn image_ir() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is in this image?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgoAAAANS".into(),
            },
        ],
    )])
}

fn multi_turn_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are an expert coder."),
        IrMessage::text(IrRole::User, "Write a function to add two numbers."),
        IrMessage::text(IrRole::Assistant, "def add(a, b): return a + b"),
        IrMessage::text(IrRole::User, "Now make it handle floats."),
        IrMessage::text(
            IrRole::Assistant,
            "def add(a: float, b: float) -> float: return a + b",
        ),
    ])
}

fn multi_tool_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Search and read"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result1".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result2".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

fn tool_error_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "permission denied".into(),
                }],
                is_error: true,
            }],
        ),
    ])
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI → Claude (JSON-level: 5, IR-level: 5)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_json_basic_chat() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: openai_chat_request(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["max_tokens"], 1024);
}

#[test]
fn openai_to_claude_json_system_message_extraction() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: openai_chat_with_system(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "You are a helpful assistant.");
    // System message should not appear in messages array
    let messages = result["messages"].as_array().unwrap();
    assert!(messages.iter().all(|m| m["role"] != "system"));
}

#[test]
fn openai_to_claude_json_streaming() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": true,
            "max_tokens": 512
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["stream"], true);
}

#[test]
fn openai_to_claude_json_tool_definitions() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "What is the weather?"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get the weather for a city",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        }
                    }
                }
            }],
            "max_tokens": 1024
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "get_weather");
}

#[test]
fn openai_to_claude_json_temperature_and_top_p() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "temperature": 0.7,
            "top_p": 0.9,
            "max_tokens": 256
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["temperature"], 0.7);
    assert_eq!(result["top_p"], 0.9);
}

#[test]
fn openai_to_claude_json_stop_sequences() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": ["END", "STOP"],
            "max_tokens": 100
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let stop = result["stop_sequences"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0], "END");
}

#[test]
fn openai_to_claude_json_max_tokens_default() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    // Claude requires max_tokens; mapper should default to 4096
    assert_eq!(result["max_tokens"], 4096);
}

#[test]
fn openai_to_claude_ir_simple() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_ir();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn openai_to_claude_ir_tool_role_becomes_user() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &tool_call_ir())
        .unwrap();
    // Tool message (index 2) → User role in Claude
    assert_eq!(result.messages[2].role, IrRole::User);
    assert!(matches!(
        &result.messages[2].content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn openai_to_claude_ir_image_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &image_ir())
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 2);
    assert!(matches!(
        &result.messages[0].content[1],
        IrContentBlock::Image { media_type, .. } if media_type == "image/png"
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude → OpenAI (JSON-level: 5, IR-level: 5)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_openai_json_basic_message() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: claude_messages_request(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello");
}

#[test]
fn claude_to_openai_json_system_injection() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: claude_with_system(),
    };
    let result = mapper.map_request(&req).unwrap();
    let messages = result["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are a helpful assistant.");
}

#[test]
fn claude_to_openai_json_content_blocks_to_text() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"}
                ]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let messages = result["messages"].as_array().unwrap();
    assert!(!messages.is_empty());
}

#[test]
fn claude_to_openai_json_tool_use_mapping() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    }
                }
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
}

#[test]
fn claude_to_openai_json_max_tokens_preserved() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: claude_messages_request(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["max_tokens"], 1024);
}

#[test]
fn claude_to_openai_ir_thinking_dropped() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &thinking_ir())
        .unwrap();
    let asst = &result.messages[1];
    // Thinking block dropped, only text remains
    assert_eq!(asst.content.len(), 1);
    assert!(matches!(
        &asst.content[0],
        IrContentBlock::Text { text } if text == "The answer is 42."
    ));
}

#[test]
fn claude_to_openai_ir_user_tool_results_become_tool_role() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &multi_tool_ir())
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn claude_to_openai_ir_simple_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &simple_ir())
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[1].role, IrRole::User);
}

#[test]
fn claude_to_openai_ir_tool_use_in_assistant_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &tool_call_ir())
        .unwrap();
    let asst = &result.messages[1];
    assert_eq!(asst.role, IrRole::Assistant);
    assert!(
        asst.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "get_weather"))
    );
}

#[test]
fn claude_to_openai_ir_tool_error_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &tool_error_ir())
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 1);
    if let IrContentBlock::ToolResult { is_error, .. } = &tool_msgs[0].content[0] {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. OpenAI → Gemini (JSON-level: 5, IR-level: 5)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_gemini_json_basic_chat() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: openai_chat_request(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["contents"][0]["role"], "user");
    assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
}

#[test]
fn openai_to_gemini_json_system_instruction() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: openai_chat_with_system(),
    };
    let result = mapper.map_request(&req).unwrap();
    let si = &result["system_instruction"];
    assert!(si.is_object() || si.is_string());
    // System message should not appear in contents
    let contents = result["contents"].as_array().unwrap();
    assert!(contents.iter().all(|c| c["role"] != "system"));
}

#[test]
fn openai_to_gemini_json_role_mapping() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let contents = result["contents"].as_array().unwrap();
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model");
}

#[test]
fn openai_to_gemini_json_function_declarations() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Get weather"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}}
                    }
                }
            }],
            "max_tokens": 512
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tools = &result["tools"];
    assert!(tools.is_array());
}

#[test]
fn openai_to_gemini_json_generation_config() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "temperature": 0.5,
            "top_p": 0.8,
            "max_tokens": 200,
            "stop": ["END"]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let gen_cfg = &result["generationConfig"];
    assert_eq!(gen_cfg["temperature"], 0.5);
    assert_eq!(gen_cfg["topP"], 0.8);
    assert_eq!(gen_cfg["maxOutputTokens"], 200);
}

#[test]
fn openai_to_gemini_ir_simple() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &simple_ir())
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[2].text_content(), "Hi there!");
}

#[test]
fn openai_to_gemini_ir_tool_role_becomes_user() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &tool_call_ir())
        .unwrap();
    assert_eq!(result.messages[2].role, IrRole::User);
}

#[test]
fn openai_to_gemini_ir_thinking_dropped() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &thinking_ir())
        .unwrap();
    let asst = &result.messages[1];
    assert!(
        !asst
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    );
    assert_eq!(asst.text_content(), "The answer is 42.");
}

#[test]
fn openai_to_gemini_ir_multi_turn() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &multi_turn_ir())
        .unwrap();
    assert_eq!(result.len(), 5);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[4].role, IrRole::Assistant);
}

#[test]
fn openai_to_gemini_ir_tool_use_preserved() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &tool_call_ir())
        .unwrap();
    let tools = result.tool_calls();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tools[0] {
        assert_eq!(name, "get_weather");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Gemini → OpenAI (JSON-level: 5, IR-level: 5)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_openai_json_basic_content() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: gemini_request(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello");
}

#[test]
fn gemini_to_openai_json_system_instruction_to_message() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: gemini_with_system(),
    };
    let result = mapper.map_request(&req).unwrap();
    let messages = result["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "system");
}

#[test]
fn gemini_to_openai_json_model_role_to_assistant() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello!"}]}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let messages = result["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
}

#[test]
fn gemini_to_openai_json_generation_config_flattening() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "generationConfig": {
                "temperature": 0.6,
                "topP": 0.85,
                "maxOutputTokens": 300,
                "stopSequences": ["END"]
            }
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["temperature"], 0.6);
    assert_eq!(result["top_p"], 0.85);
    assert_eq!(result["max_tokens"], 300);
}

#[test]
fn gemini_to_openai_json_model_passthrough() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: gemini_request(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["model"], "gemini-pro");
}

#[test]
fn gemini_to_openai_ir_simple() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &simple_ir())
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
}

#[test]
fn gemini_to_openai_ir_user_tool_results_split() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &multi_tool_ir())
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn gemini_to_openai_ir_thinking_dropped() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &thinking_ir())
        .unwrap();
    assert!(!result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn gemini_to_openai_ir_text_preserved() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &simple_ir())
        .unwrap();
    assert_eq!(result.messages[1].text_content(), "Hello");
    assert_eq!(result.messages[2].text_content(), "Hi there!");
}

#[test]
fn gemini_to_openai_ir_empty_conversation() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
        .unwrap();
    assert!(result.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Claude → Gemini (IR-level: 10)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_gemini_ir_simple() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &simple_ir())
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[1].text_content(), "Hello");
}

#[test]
fn claude_to_gemini_ir_thinking_dropped() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &thinking_ir())
        .unwrap();
    let asst = &result.messages[1];
    assert!(
        !asst
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    );
    assert_eq!(asst.text_content(), "The answer is 42.");
}

#[test]
fn claude_to_gemini_ir_tool_role_becomes_user() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "file.txt".into(),
                }],
                is_error: false,
            }],
        ),
    ]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    // Tool role → User role
    assert_eq!(result.messages[2].role, IrRole::User);
}

#[test]
fn claude_to_gemini_ir_image_preserved() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &image_ir())
        .unwrap();
    assert!(matches!(
        &result.messages[0].content[1],
        IrContentBlock::Image { media_type, .. } if media_type == "image/png"
    ));
}

#[test]
fn claude_to_gemini_ir_system_image_rejected() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "You are helpful.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64".into(),
            },
        ],
    )]);
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableContent { .. }));
}

#[test]
fn gemini_to_claude_ir_simple() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &simple_ir())
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
}

#[test]
fn gemini_to_claude_ir_tool_role_becomes_user() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &tool_call_ir())
        .unwrap();
    // Tool-role messages → User role for Claude
    assert_eq!(result.messages[2].role, IrRole::User);
}

#[test]
fn gemini_to_claude_ir_image_preserved() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &image_ir())
        .unwrap();
    assert!(matches!(
        &result.messages[0].content[1],
        IrContentBlock::Image { media_type, .. } if media_type == "image/png"
    ));
}

#[test]
fn claude_to_gemini_ir_multi_turn() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &multi_turn_ir())
        .unwrap();
    assert_eq!(result.len(), 5);
    assert_eq!(result.messages[4].role, IrRole::Assistant);
}

#[test]
fn claude_gemini_ir_unsupported_pair() {
    let mapper = ClaudeGeminiIrMapper;
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Roundtrip tests (10): A→B→A for each pair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_claude_simple_text() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = simple_ir();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_openai_claude_tool_calls() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = tool_call_ir();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();
    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    assert_eq!(orig_tools.len(), back_tools.len());
    for (ot, bt) in orig_tools.iter().zip(back_tools.iter()) {
        if let (
            IrContentBlock::ToolUse {
                name: on,
                input: oi,
                ..
            },
            IrContentBlock::ToolUse {
                name: bn,
                input: bi,
                ..
            },
        ) = (ot, bt)
        {
            assert_eq!(on, bn);
            assert_eq!(oi, bi);
        }
    }
}

#[test]
fn roundtrip_openai_gemini_simple_text() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = simple_ir();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_openai_gemini_tool_calls() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = tool_call_ir();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    assert_eq!(orig_tools.len(), back_tools.len());
}

#[test]
fn roundtrip_claude_gemini_simple_text() {
    let mapper = ClaudeGeminiIrMapper;
    let orig = simple_ir();
    let gemini = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_claude_gemini_image() {
    let mapper = ClaudeGeminiIrMapper;
    let orig = image_ir();
    let gemini = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    assert_eq!(
        orig.messages[0].content.len(),
        back.messages[0].content.len()
    );
    assert!(matches!(
        &back.messages[0].content[1],
        IrContentBlock::Image { media_type, data } if media_type == "image/png" && data == "iVBORw0KGgoAAAANS"
    ));
}

#[test]
fn roundtrip_thinking_lost_via_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = thinking_ir();
    let openai = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    // Text survives
    assert_eq!(
        back.messages[1].text_content(),
        orig.messages[1].text_content()
    );
    // Thinking block lost
    assert!(!back.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn roundtrip_thinking_lost_via_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let orig = thinking_ir();
    let gemini = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    // Text survives
    assert_eq!(
        back.messages[1].text_content(),
        orig.messages[1].text_content()
    );
    // Thinking block lost
    assert!(!back.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn roundtrip_openai_claude_multi_turn() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = multi_turn_ir();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_openai_gemini_multi_turn() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = multi_turn_ir();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.text_content(), b.text_content());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. JSON-level roundtrip via paired mappers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_roundtrip_openai_claude_basic() {
    let fwd = OpenAiToClaudeMapper;
    let rev = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: openai_chat_with_system(),
    };
    let claude_body = fwd.map_request(&req).unwrap();
    let back_req = DialectRequest {
        dialect: Dialect::Claude,
        body: claude_body,
    };
    let openai_body = rev.map_request(&back_req).unwrap();
    // Core fields survive
    let msgs = openai_body["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m["role"] == "system"));
    assert!(msgs.iter().any(|m| m["role"] == "user"));
}

#[test]
fn json_roundtrip_openai_gemini_basic() {
    let fwd = OpenAiToGeminiMapper;
    let rev = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: openai_chat_with_system(),
    };
    let gemini_body = fwd.map_request(&req).unwrap();
    let back_req = DialectRequest {
        dialect: Dialect::Gemini,
        body: gemini_body,
    };
    let openai_body = rev.map_request(&back_req).unwrap();
    let msgs = openai_body["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m["role"] == "system"));
    assert!(msgs.iter().any(|m| m["role"] == "user"));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Factory and cross-cutting tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_resolves_all_major_pairs() {
    let pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::Claude, Dialect::OpenAi),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Gemini, Dialect::OpenAi),
        (Dialect::Claude, Dialect::Gemini),
        (Dialect::Gemini, Dialect::Claude),
    ];
    for (from, to) in &pairs {
        assert!(
            default_ir_mapper(*from, *to).is_some(),
            "no mapper for {from} -> {to}"
        );
    }
}

#[test]
fn factory_mapper_simple_conv() {
    for (from, to) in &[
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Claude, Dialect::Gemini),
    ] {
        let mapper = default_ir_mapper(*from, *to).unwrap();
        let result = mapper.map_request(*from, *to, &simple_ir()).unwrap();
        assert_eq!(result.len(), 3, "failed for {from} -> {to}");
    }
}

#[test]
fn wrong_dialect_json_mapper_rejected() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({"model": "claude"}),
    };
    assert!(mapper.map_request(&req).is_err());
}

#[test]
fn wrong_dialect_json_gemini_mapper_rejected() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"model": "gemini"}),
    };
    assert!(mapper.map_request(&req).is_err());
}

#[test]
fn metadata_preserved_through_ir_mapping() {
    let mapper = OpenAiClaudeIrMapper;
    let mut msg = IrMessage::text(IrRole::User, "hello");
    msg.metadata.insert("request_id".into(), json!("abc-123"));
    let conv = IrConversation::from_messages(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("request_id"),
        Some(&json!("abc-123"))
    );
}

#[test]
fn openai_to_claude_ir_empty() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn claude_to_gemini_ir_empty() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn openai_to_claude_ir_system_with_image_rejected() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "You are helpful.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "base64".into(),
            },
        ],
    )]);
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableContent { .. }));
}

#[test]
fn roundtrip_tool_error_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = tool_error_ir();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();
    // Find the tool result and verify is_error survived
    let tool_msgs: Vec<_> = back
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 1);
    if let IrContentBlock::ToolResult { is_error, .. } = &tool_msgs[0].content[0] {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn roundtrip_tool_error_openai_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = tool_error_ir();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    let tool_msgs: Vec<_> = back
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 1);
    if let IrContentBlock::ToolResult { is_error, .. } = &tool_msgs[0].content[0] {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn claude_to_openai_json_stream_passthrough() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": true
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["stream"], true);
}

#[test]
fn claude_to_openai_json_stop_sequences_mapping() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hi"}],
            "stop_sequences": ["END", "STOP"]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let stop = &result["stop"];
    assert!(stop.is_array());
}

#[test]
fn openai_to_gemini_json_stop_string() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": ["DONE"],
            "max_tokens": 100
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let gen_cfg = &result["generationConfig"];
    assert!(gen_cfg["stopSequences"].is_array());
}

#[test]
fn gemini_to_openai_ir_preserves_multi_turn() {
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &multi_turn_ir())
        .unwrap();
    assert_eq!(result.len(), 5);
    assert_eq!(
        result.messages[2].text_content(),
        "def add(a, b): return a + b"
    );
}

#[test]
fn claude_to_gemini_ir_tool_use_preserved() {
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &tool_call_ir())
        .unwrap();
    let tools = result.tool_calls();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, input, .. } = tools[0] {
        assert_eq!(name, "get_weather");
        assert_eq!(*input, json!({"city": "NYC"}));
    }
}

#[test]
fn roundtrip_claude_gemini_multi_tool() {
    let mapper = ClaudeGeminiIrMapper;
    let orig = multi_tool_ir();
    let gemini = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    // Tool calls should survive the roundtrip
    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    assert_eq!(orig_tools.len(), back_tools.len());
}

#[test]
fn all_three_dialects_transitive_simple() {
    // OpenAI → Claude → Gemini: text content should survive the full chain
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let orig = simple_ir();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    assert_eq!(gemini.len(), 3);
    assert_eq!(gemini.messages[1].text_content(), "Hello");
    assert_eq!(gemini.messages[2].text_content(), "Hi there!");
}

#[test]
fn all_three_dialects_reverse_transitive() {
    // Gemini → OpenAI → Claude: text content should survive the full chain
    let og = OpenAiGeminiIrMapper;
    let oc = OpenAiClaudeIrMapper;
    let orig = simple_ir();
    let openai = og
        .map_request(Dialect::Gemini, Dialect::OpenAi, &orig)
        .unwrap();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    assert_eq!(claude.len(), 3);
    assert_eq!(claude.messages[1].text_content(), "Hello");
}
