// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for improved cross-dialect mapping: role mapping, tool format mapping,
//! content block mapping, and early failure for unmappable requests.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeToOpenAiMapper, CodexClaudeIrMapper, DialectRequest,
    GeminiToOpenAiMapper, IrMapper, MapError, Mapper, OpenAiToClaudeMapper, OpenAiToGeminiMapper,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// 1. Role mapping tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_system_to_top_level() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a coding assistant."},
                {"role": "user", "content": "Help me"}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "You are a coding assistant.");
    let messages = result["messages"].as_array().unwrap();
    assert!(messages.iter().all(|m| m["role"] != "system"));
}

#[test]
fn openai_to_claude_tool_role_becomes_user_tool_result() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "result data"
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let msg = &result["messages"][0];
    assert_eq!(msg["role"], "user");
    assert_eq!(msg["content"][0]["type"], "tool_result");
    assert_eq!(msg["content"][0]["tool_use_id"], "call_1");
}

#[test]
fn claude_to_openai_user_tool_result_becomes_tool_role() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "tu_1",
                    "content": "72°F"
                }]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let msg = &result["messages"][0];
    assert_eq!(msg["role"], "tool");
    assert_eq!(msg["tool_call_id"], "tu_1");
    assert_eq!(msg["content"], "72°F");
}

#[test]
fn claude_to_openai_system_becomes_system_message() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3",
            "max_tokens": 1024,
            "system": "Act as a coder.",
            "messages": [{"role": "user", "content": "Go"}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Act as a coder.");
}

#[test]
fn openai_to_gemini_system_to_system_instruction() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a bot."},
                {"role": "user", "content": "Hi"}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(
        result["system_instruction"]["parts"][0]["text"],
        "You are a bot."
    );
    // No system in contents
    let contents = result["contents"].as_array().unwrap();
    assert!(contents.iter().all(|c| c["role"] != "system"));
}

#[test]
fn openai_to_gemini_assistant_becomes_model() {
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
    assert_eq!(result["contents"][1]["role"], "model");
}

#[test]
fn gemini_to_openai_model_becomes_assistant() {
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
    assert_eq!(result["messages"][1]["role"], "assistant");
    assert_eq!(result["messages"][1]["content"], "Hello!");
}

#[test]
fn gemini_to_openai_system_instruction_becomes_system() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "system_instruction": {
                "parts": [{"text": "You are smart."}]
            },
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "system");
    assert_eq!(result["messages"][0]["content"], "You are smart.");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Tool format mapping tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_function_call_to_claude_tool_use() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"rust\"}"
                    }
                }]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let asst = &result["messages"][0];
    let blocks = asst["content"].as_array().unwrap();
    let tool_block = blocks.iter().find(|b| b["type"] == "tool_use").unwrap();
    assert_eq!(tool_block["id"], "call_abc");
    assert_eq!(tool_block["name"], "search");
    assert_eq!(tool_block["input"]["query"], "rust");
}

#[test]
fn claude_tool_use_to_openai_function_call() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "tu_xyz",
                    "name": "read_file",
                    "input": {"path": "main.rs"}
                }]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let asst = &result["messages"][0];
    let tc = &asst["tool_calls"][0];
    assert_eq!(tc["id"], "tu_xyz");
    assert_eq!(tc["type"], "function");
    assert_eq!(tc["function"]["name"], "read_file");
    // arguments should be a JSON string
    let args: serde_json::Value =
        serde_json::from_str(tc["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(args["path"], "main.rs");
}

#[test]
fn openai_function_to_gemini_declarations() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "calc",
                    "description": "Calculate",
                    "parameters": {
                        "type": "object",
                        "properties": {"expr": {"type": "string"}}
                    }
                }
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let decl = &result["tools"][0]["function_declarations"][0];
    assert_eq!(decl["name"], "calc");
    assert_eq!(decl["description"], "Calculate");
    assert!(decl["parameters"]["properties"]["expr"].is_object());
}

#[test]
fn gemini_function_to_openai_function() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "tools": [{
                "function_declarations": [{
                    "name": "calc",
                    "description": "Calculate",
                    "parameters": {
                        "type": "object",
                        "properties": {"expr": {"type": "string"}}
                    }
                }]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tool = &result["tools"][0];
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["function"]["name"], "calc");
    assert_eq!(tool["function"]["description"], "Calculate");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Content block mapping tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_text_string_to_claude_text() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "plain text"}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    // User content stays as string in Claude format
    assert_eq!(result["messages"][0]["content"], "plain text");
}

#[test]
fn claude_image_to_openai_image_url() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe this image"},
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/jpeg",
                            "data": "AAAA"
                        }
                    }
                ]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let msg = &result["messages"][0];
    assert_eq!(msg["role"], "user");
    let parts = msg["content"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[0]["text"], "Describe this image");
    assert_eq!(parts[1]["type"], "image_url");
    let url = parts[1]["image_url"]["url"].as_str().unwrap();
    assert!(url.starts_with("data:image/jpeg;base64,"));
    assert!(url.ends_with("AAAA"));
}

#[test]
fn gemini_inline_data_to_openai_image_url() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{
                "role": "user",
                "parts": [
                    {"text": "What is this?"},
                    {"inlineData": {"mimeType": "image/png", "data": "BBBB"}}
                ]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let msg = &result["messages"][0];
    let parts = msg["content"].as_array().unwrap();
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "image_url");
    let url = parts[1]["image_url"]["url"].as_str().unwrap();
    assert_eq!(url, "data:image/png;base64,BBBB");
}

#[test]
fn openai_image_url_to_gemini_inline_data() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,CCCC"}}
                ]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let parts = &result["contents"][0]["parts"];
    assert_eq!(parts[0]["text"], "Describe");
    assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
    assert_eq!(parts[1]["inlineData"]["data"], "CCCC");
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Early failure for unmappable requests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn system_prompt_with_images_fails_claude_to_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![
                IrContentBlock::Text {
                    text: "Analyze this logo".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "base64data".into(),
                },
            ],
        ),
        IrMessage::text(IrRole::User, "What colors are in it?"),
    ]);
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnmappableContent { ref field, .. } if field == "system"),
        "expected UnmappableContent for system, got: {err}"
    );
}

#[test]
fn system_prompt_text_only_ok_claude_to_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
    assert!(result.is_ok());
}

#[test]
fn codex_apply_patch_fails_to_claude() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Fix the bug"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "apply_patch".into(),
                input: json!({"patch": "--- a/file\n+++ b/file"}),
            }],
        ),
    ]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnmappableTool { ref name, .. } if name == "apply_patch"),
        "expected UnmappableTool for apply_patch, got: {err}"
    );
}

#[test]
fn codex_apply_diff_fails_to_claude() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t2".into(),
            name: "apply_diff".into(),
            input: json!({"diff": "..."}),
        }],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { ref name, .. } if name == "apply_diff"));
}

#[test]
fn codex_regular_text_maps_ok_to_claude() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ]);
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Cross-dialect roundtrip tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_gemini_openai() {
    let o2g = OpenAiToGeminiMapper;
    let g2o = GeminiToOpenAiMapper;

    let original = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "Be brief"},
            {"role": "user", "content": "What is Rust?"},
            {"role": "assistant", "content": "A systems language."}
        ],
        "max_tokens": 512,
        "temperature": 0.5
    });

    let gemini = o2g
        .map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: original.clone(),
        })
        .unwrap();

    // Verify Gemini structure
    assert!(gemini.get("system_instruction").is_some());
    assert_eq!(gemini["contents"][1]["role"], "model");

    let back = g2o
        .map_request(&DialectRequest {
            dialect: Dialect::Gemini,
            body: gemini,
        })
        .unwrap();

    let msgs = back["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Be brief");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"], "What is Rust?");
    assert_eq!(msgs[2]["role"], "assistant");
    assert_eq!(msgs[2]["content"], "A systems language.");
    assert_eq!(back["max_tokens"], 512);
    assert_eq!(back["temperature"], 0.5);
}

#[test]
fn roundtrip_openai_gemini_tool_definitions() {
    let o2g = OpenAiToGeminiMapper;
    let g2o = GeminiToOpenAiMapper;

    let original = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "search",
                "description": "Search",
                "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
            }
        }]
    });

    let gemini = o2g
        .map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: original,
        })
        .unwrap();

    let back = g2o
        .map_request(&DialectRequest {
            dialect: Dialect::Gemini,
            body: gemini,
        })
        .unwrap();

    assert_eq!(back["tools"][0]["function"]["name"], "search");
    assert_eq!(back["tools"][0]["function"]["description"], "Search");
}

#[test]
fn roundtrip_gemini_role_mapping() {
    // model→assistant→model roundtrip
    let g2o = GeminiToOpenAiMapper;
    let o2g = OpenAiToGeminiMapper;

    let gemini_req = json!({
        "model": "gemini-pro",
        "system_instruction": {"parts": [{"text": "Be helpful"}]},
        "contents": [
            {"role": "user", "parts": [{"text": "Hi"}]},
            {"role": "model", "parts": [{"text": "Hello!"}]}
        ]
    });

    let openai = g2o
        .map_request(&DialectRequest {
            dialect: Dialect::Gemini,
            body: gemini_req,
        })
        .unwrap();

    assert_eq!(openai["messages"][1]["role"], "user");
    assert_eq!(openai["messages"][2]["role"], "assistant");

    let back = o2g
        .map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai,
        })
        .unwrap();

    assert_eq!(back["contents"][0]["role"], "user");
    assert_eq!(back["contents"][1]["role"], "model");
}
