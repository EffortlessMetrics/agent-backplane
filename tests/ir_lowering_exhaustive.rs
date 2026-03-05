#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive tests for lowering from IR to each SDK dialect.

use abp_ir::lower::*;
use abp_ir::*;
use abp_sdk_types::Dialect;
use serde_json::{json, Value};

// ── Helpers ────────────────────────────────────────────────────────────

fn empty_conv() -> IrConversation {
    IrConversation::new()
}

fn single_user(text: &str) -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, text))
}

fn single_assistant(text: &str) -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::Assistant, text))
}

fn single_system(text: &str) -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::System, text))
}

fn system_user_assistant() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"))
}

fn multi_turn() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be concise."))
        .push(IrMessage::text(IrRole::User, "What is 2+2?"))
        .push(IrMessage::text(IrRole::Assistant, "4"))
        .push(IrMessage::text(IrRole::User, "And 3+3?"))
        .push(IrMessage::text(IrRole::Assistant, "6"))
}

fn tool_use_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Read file.txt"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me read that.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "read_file".into(),
                    input: json!({"path": "file.txt"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_abc".into(),
                content: vec![IrContentBlock::Text {
                    text: "file contents here".into(),
                }],
                is_error: false,
            }],
        ))
}

fn tool_error_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_err".into(),
            content: vec![IrContentBlock::Text {
                text: "Not found".into(),
            }],
            is_error: true,
        }],
    ))
}

fn thinking_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Let me think...".into(),
            },
            IrContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ],
    ))
}

fn image_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is in this image?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    ))
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![IrToolDefinition {
        name: "calculator".into(),
        description: "Evaluate math expressions".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "expr": {"type": "string"}
            },
            "required": ["expr"]
        }),
    }]
}

fn two_tools() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
        IrToolDefinition {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
        },
    ]
}

fn all_openai_like() -> Vec<Dialect> {
    vec![
        Dialect::OpenAi,
        Dialect::Codex,
        Dialect::Copilot,
        Dialect::Kimi,
    ]
}

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn only_tool_use_assistant() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"q": "hello"}),
        }],
    ))
}

fn multiple_tool_results() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![
            IrContentBlock::ToolResult {
                tool_use_id: "call_a".into(),
                content: vec![IrContentBlock::Text {
                    text: "result A".into(),
                }],
                is_error: false,
            },
            IrContentBlock::ToolResult {
                tool_use_id: "call_b".into(),
                content: vec![IrContentBlock::Text {
                    text: "result B".into(),
                }],
                is_error: false,
            },
        ],
    ))
}

// ── 1. Empty conversation ──────────────────────────────────────────────

#[test]
fn empty_conv_openai() {
    let v = lower_to_openai(&empty_conv(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
    assert!(v.get("tools").is_none());
}

#[test]
fn empty_conv_claude() {
    let v = lower_to_claude(&empty_conv(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
    assert!(v.get("system").is_none());
}

#[test]
fn empty_conv_gemini() {
    let v = lower_to_gemini(&empty_conv(), &[]);
    assert_eq!(v["contents"].as_array().unwrap().len(), 0);
    assert!(v.get("system_instruction").is_none());
}

#[test]
fn empty_conv_kimi() {
    let v = lower_to_kimi(&empty_conv(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn empty_conv_codex() {
    let v = lower_to_codex(&empty_conv(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn empty_conv_copilot() {
    let v = lower_to_copilot(&empty_conv(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn empty_conv_lower_for_dialect_all() {
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &empty_conv(), &[]);
        assert!(v.is_object(), "{d}: should produce object");
    }
}

// ── 2. Single-message conversations ────────────────────────────────────

#[test]
fn single_user_openai() {
    let v = lower_to_openai(&single_user("hi"), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "hi");
}

#[test]
fn single_user_claude() {
    let v = lower_to_claude(&single_user("hi"), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "user");
    let content = msgs[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "hi");
}

#[test]
fn single_user_gemini() {
    let v = lower_to_gemini(&single_user("hi"), &[]);
    let contents = v["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[0]["parts"][0]["text"], "hi");
}

#[test]
fn single_assistant_openai() {
    let v = lower_to_openai(&single_assistant("bye"), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "assistant");
    assert_eq!(msgs[0]["content"], "bye");
}

#[test]
fn single_assistant_gemini_uses_model_role() {
    let v = lower_to_gemini(&single_assistant("bye"), &[]);
    assert_eq!(v["contents"][0]["role"], "model");
}

#[test]
fn single_system_openai_inline() {
    let v = lower_to_openai(&single_system("system prompt"), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "system prompt");
}

#[test]
fn single_system_claude_extracted() {
    let v = lower_to_claude(&single_system("system prompt"), &[]);
    assert_eq!(v["system"], "system prompt");
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn single_system_gemini_extracted() {
    let v = lower_to_gemini(&single_system("system prompt"), &[]);
    assert_eq!(v["system_instruction"]["parts"][0]["text"], "system prompt");
    assert_eq!(v["contents"].as_array().unwrap().len(), 0);
}

// ── 3. System prompt handling per dialect ──────────────────────────────

#[test]
fn system_prompt_openai_inline_role() {
    let v = lower_to_openai(&system_user_assistant(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "You are helpful.");
}

#[test]
fn system_prompt_kimi_inline_role() {
    let v = lower_to_kimi(&system_user_assistant(), &[]);
    assert_eq!(v["messages"][0]["role"], "system");
}

#[test]
fn system_prompt_codex_inline_role() {
    let v = lower_to_codex(&system_user_assistant(), &[]);
    assert_eq!(v["messages"][0]["role"], "system");
}

#[test]
fn system_prompt_copilot_inline_role() {
    let v = lower_to_copilot(&system_user_assistant(), &[]);
    assert_eq!(v["messages"][0]["role"], "system");
}

#[test]
fn system_prompt_claude_top_level() {
    let v = lower_to_claude(&system_user_assistant(), &[]);
    assert_eq!(v["system"], "You are helpful.");
    let msgs = v["messages"].as_array().unwrap();
    assert!(msgs.iter().all(|m| m["role"] != "system"));
}

#[test]
fn system_prompt_gemini_system_instruction() {
    let v = lower_to_gemini(&system_user_assistant(), &[]);
    assert_eq!(
        v["system_instruction"]["parts"][0]["text"],
        "You are helpful."
    );
    let contents = v["contents"].as_array().unwrap();
    assert!(contents.iter().all(|c| c["role"] != "system"));
}

#[test]
fn no_system_prompt_claude_omits_field() {
    let v = lower_to_claude(&single_user("hi"), &[]);
    assert!(v.get("system").is_none());
}

#[test]
fn no_system_prompt_gemini_omits_field() {
    let v = lower_to_gemini(&single_user("hi"), &[]);
    assert!(v.get("system_instruction").is_none());
}

// ── 4. Content preservation ────────────────────────────────────────────

#[test]
fn content_preserved_openai_all_like() {
    for d in all_openai_like() {
        let conv = single_user("exact content 🎉");
        let v = lower_for_dialect(d, &conv, &[]);
        assert_eq!(v["messages"][0]["content"], "exact content 🎉");
    }
}

#[test]
fn content_preserved_claude() {
    let v = lower_to_claude(&single_user("exact content 🎉"), &[]);
    assert_eq!(v["messages"][0]["content"][0]["text"], "exact content 🎉");
}

#[test]
fn content_preserved_gemini() {
    let v = lower_to_gemini(&single_user("exact content 🎉"), &[]);
    assert_eq!(v["contents"][0]["parts"][0]["text"], "exact content 🎉");
}

#[test]
fn empty_string_content_preserved_openai() {
    let v = lower_to_openai(&single_user(""), &[]);
    assert_eq!(v["messages"][0]["content"], "");
}

#[test]
fn multiline_content_preserved() {
    let text = "line1\nline2\nline3";
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &single_user(text), &[]);
        assert!(v.is_object());
    }
}

#[test]
fn unicode_content_preserved_all_dialects() {
    let text = "日本語テスト 中文测试 한국어";
    let conv = single_user(text);
    let v_openai = lower_to_openai(&conv, &[]);
    assert_eq!(v_openai["messages"][0]["content"], text);
    let v_claude = lower_to_claude(&conv, &[]);
    assert_eq!(v_claude["messages"][0]["content"][0]["text"], text);
    let v_gemini = lower_to_gemini(&conv, &[]);
    assert_eq!(v_gemini["contents"][0]["parts"][0]["text"], text);
}

// ── 5. Multi-turn conversations ────────────────────────────────────────

#[test]
fn multi_turn_openai_message_count() {
    let v = lower_to_openai(&multi_turn(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 5);
}

#[test]
fn multi_turn_claude_filters_system() {
    let v = lower_to_claude(&multi_turn(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 4);
    assert_eq!(v["system"], "Be concise.");
}

#[test]
fn multi_turn_gemini_filters_system() {
    let v = lower_to_gemini(&multi_turn(), &[]);
    let contents = v["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 4);
}

#[test]
fn multi_turn_role_alternation_openai() {
    let v = lower_to_openai(&multi_turn(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
    assert_eq!(msgs[3]["role"], "user");
    assert_eq!(msgs[4]["role"], "assistant");
}

#[test]
fn multi_turn_gemini_role_mapping() {
    let v = lower_to_gemini(&multi_turn(), &[]);
    let c = v["contents"].as_array().unwrap();
    assert_eq!(c[0]["role"], "user");
    assert_eq!(c[1]["role"], "model");
    assert_eq!(c[2]["role"], "user");
    assert_eq!(c[3]["role"], "model");
}

#[test]
fn multi_turn_content_values_openai() {
    let v = lower_to_openai(&multi_turn(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs[1]["content"], "What is 2+2?");
    assert_eq!(msgs[2]["content"], "4");
    assert_eq!(msgs[3]["content"], "And 3+3?");
    assert_eq!(msgs[4]["content"], "6");
}

// ── 6. Tool definition lowering ────────────────────────────────────────

#[test]
fn tool_def_openai_structure() {
    let v = lower_to_openai(&single_user("hi"), &sample_tools());
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "calculator");
    assert_eq!(
        tools[0]["function"]["description"],
        "Evaluate math expressions"
    );
    assert!(tools[0]["function"]["parameters"].is_object());
}

#[test]
fn tool_def_kimi_matches_openai() {
    let conv = single_user("hi");
    let tools = sample_tools();
    assert_eq!(
        lower_to_openai(&conv, &tools)["tools"],
        lower_to_kimi(&conv, &tools)["tools"]
    );
}

#[test]
fn tool_def_codex_matches_openai() {
    let conv = single_user("hi");
    let tools = sample_tools();
    assert_eq!(
        lower_to_openai(&conv, &tools)["tools"],
        lower_to_codex(&conv, &tools)["tools"]
    );
}

#[test]
fn tool_def_copilot_matches_openai() {
    let conv = single_user("hi");
    let tools = sample_tools();
    assert_eq!(
        lower_to_openai(&conv, &tools)["tools"],
        lower_to_copilot(&conv, &tools)["tools"]
    );
}

#[test]
fn tool_def_claude_uses_input_schema() {
    let v = lower_to_claude(&single_user("hi"), &sample_tools());
    let tool = &v["tools"][0];
    assert_eq!(tool["name"], "calculator");
    assert_eq!(tool["description"], "Evaluate math expressions");
    assert!(tool.get("input_schema").is_some());
    assert!(tool.get("parameters").is_none());
    assert!(tool.get("function").is_none());
}

#[test]
fn tool_def_gemini_function_declarations() {
    let v = lower_to_gemini(&single_user("hi"), &sample_tools());
    let decls = v["tools"][0]["function_declarations"].as_array().unwrap();
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0]["name"], "calculator");
    assert_eq!(decls[0]["description"], "Evaluate math expressions");
    assert!(decls[0]["parameters"].is_object());
}

#[test]
fn multiple_tools_openai() {
    let v = lower_to_openai(&single_user("hi"), &two_tools());
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["function"]["name"], "read_file");
    assert_eq!(tools[1]["function"]["name"], "write_file");
}

#[test]
fn multiple_tools_claude() {
    let v = lower_to_claude(&single_user("hi"), &two_tools());
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "read_file");
    assert_eq!(tools[1]["name"], "write_file");
}

#[test]
fn multiple_tools_gemini() {
    let v = lower_to_gemini(&single_user("hi"), &two_tools());
    let decls = v["tools"][0]["function_declarations"].as_array().unwrap();
    assert_eq!(decls.len(), 2);
}

#[test]
fn no_tools_omits_tools_field_all_dialects() {
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &single_user("hi"), &[]);
        assert!(v.get("tools").is_none(), "{d}: tools should be absent");
    }
}

// ── 7. Tool use lowering (assistant calling tools) ─────────────────────

#[test]
fn tool_use_openai_tool_calls_field() {
    let v = lower_to_openai(&tool_use_conv(), &[]);
    let assistant_msg = &v["messages"][1];
    assert_eq!(assistant_msg["role"], "assistant");
    assert_eq!(assistant_msg["content"], "Let me read that.");
    let tc = assistant_msg["tool_calls"].as_array().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0]["id"], "call_abc");
    assert_eq!(tc[0]["type"], "function");
    assert_eq!(tc[0]["function"]["name"], "read_file");
}

#[test]
fn tool_use_openai_arguments_stringified() {
    let v = lower_to_openai(&tool_use_conv(), &[]);
    let args = v["messages"][1]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    let parsed: Value = serde_json::from_str(args).unwrap();
    assert_eq!(parsed["path"], "file.txt");
}

#[test]
fn tool_use_claude_content_blocks() {
    let v = lower_to_claude(&tool_use_conv(), &[]);
    let assistant_msg = &v["messages"][1];
    let content = assistant_msg["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "tool_use");
    assert_eq!(content[1]["id"], "call_abc");
    assert_eq!(content[1]["name"], "read_file");
    assert_eq!(content[1]["input"]["path"], "file.txt");
}

#[test]
fn tool_use_gemini_function_call() {
    let v = lower_to_gemini(&tool_use_conv(), &[]);
    let assistant_parts = v["contents"][1]["parts"].as_array().unwrap();
    let fc_part = assistant_parts
        .iter()
        .find(|p| p.get("functionCall").is_some())
        .unwrap();
    assert_eq!(fc_part["functionCall"]["name"], "read_file");
    assert_eq!(fc_part["functionCall"]["args"]["path"], "file.txt");
}

#[test]
fn only_tool_use_no_text_openai() {
    let v = lower_to_openai(&only_tool_use_assistant(), &[]);
    let msg = &v["messages"][0];
    assert!(msg.get("content").is_none() || msg["content"].is_null());
    assert_eq!(msg["tool_calls"].as_array().unwrap().len(), 1);
}

#[test]
fn only_tool_use_claude_no_text_block() {
    let v = lower_to_claude(&only_tool_use_assistant(), &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "tool_use");
}

#[test]
fn only_tool_use_gemini_function_call_only() {
    let v = lower_to_gemini(&only_tool_use_assistant(), &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert!(parts[0].get("functionCall").is_some());
}

// ── 8. Tool result lowering ────────────────────────────────────────────

#[test]
fn tool_result_openai_role_tool() {
    let v = lower_to_openai(&tool_use_conv(), &[]);
    let tool_msg = &v["messages"][2];
    assert_eq!(tool_msg["role"], "tool");
    assert_eq!(tool_msg["tool_call_id"], "call_abc");
    assert_eq!(tool_msg["content"], "file contents here");
}

#[test]
fn tool_result_claude_content_block() {
    let v = lower_to_claude(&tool_use_conv(), &[]);
    let tool_msg = &v["messages"][2];
    assert_eq!(tool_msg["role"], "user");
    let content = tool_msg["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "tool_result");
    assert_eq!(content[0]["tool_use_id"], "call_abc");
    assert_eq!(content[0]["is_error"], false);
    let inner = content[0]["content"].as_array().unwrap();
    assert_eq!(inner[0]["type"], "text");
    assert_eq!(inner[0]["text"], "file contents here");
}

#[test]
fn tool_result_gemini_function_response() {
    let v = lower_to_gemini(&tool_use_conv(), &[]);
    let tool_parts = v["contents"][2]["parts"].as_array().unwrap();
    let fr = &tool_parts[0]["functionResponse"];
    assert_eq!(fr["name"], "call_abc");
    assert_eq!(fr["response"]["result"], "file contents here");
}

#[test]
fn tool_error_claude_is_error_true() {
    let v = lower_to_claude(&tool_error_conv(), &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["is_error"], true);
}

#[test]
fn tool_error_openai_still_produces_content() {
    let v = lower_to_openai(&tool_error_conv(), &[]);
    let msg = &v["messages"][0];
    assert_eq!(msg["content"], "Not found");
    assert_eq!(msg["tool_call_id"], "call_err");
}

#[test]
fn multiple_tool_results_openai_expand_to_messages() {
    let v = lower_to_openai(&multiple_tool_results(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["tool_call_id"], "call_a");
    assert_eq!(msgs[0]["content"], "result A");
    assert_eq!(msgs[1]["tool_call_id"], "call_b");
    assert_eq!(msgs[1]["content"], "result B");
}

#[test]
fn multiple_tool_results_claude_single_message() {
    let v = lower_to_claude(&multiple_tool_results(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    let content = msgs[0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "tool_result");
    assert_eq!(content[1]["type"], "tool_result");
}

#[test]
fn tool_result_gemini_role_user() {
    let v = lower_to_gemini(&tool_use_conv(), &[]);
    assert_eq!(v["contents"][2]["role"], "user");
}

// ── 9. Thinking/reasoning block handling ───────────────────────────────

#[test]
fn thinking_claude_preserved() {
    let v = lower_to_claude(&thinking_conv(), &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "thinking");
    assert_eq!(content[0]["thinking"], "Let me think...");
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[1]["text"], "The answer is 42.");
}

#[test]
fn thinking_gemini_skipped() {
    let v = lower_to_gemini(&thinking_conv(), &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["text"], "The answer is 42.");
}

#[test]
fn thinking_openai_included_as_text() {
    // OpenAI lowering uses text_content() which only concatenates Text blocks
    let v = lower_to_openai(&thinking_conv(), &[]);
    let msg = &v["messages"][0];
    assert_eq!(msg["content"], "The answer is 42.");
}

#[test]
fn thinking_only_message_openai() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "pondering".into(),
        }],
    ));
    let v = lower_to_openai(&conv, &[]);
    // text_content() returns "" for thinking-only
    assert_eq!(v["messages"][0]["content"], "");
}

#[test]
fn thinking_only_message_gemini_empty_parts() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "pondering".into(),
        }],
    ));
    let v = lower_to_gemini(&conv, &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 0);
}

#[test]
fn thinking_only_message_claude_has_thinking_block() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "deep thought".into(),
        }],
    ));
    let v = lower_to_claude(&conv, &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "thinking");
}

// ── 10. Image/vision content lowering ──────────────────────────────────

#[test]
fn image_claude_source_block() {
    let v = lower_to_claude(&image_conv(), &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["type"], "base64");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(content[1]["source"]["data"], "iVBORw0KGgo=");
}

#[test]
fn image_gemini_inline_data() {
    let v = lower_to_gemini(&image_conv(), &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["text"], "What is in this image?");
    assert_eq!(parts[1]["inline_data"]["mime_type"], "image/png");
    assert_eq!(parts[1]["inline_data"]["data"], "iVBORw0KGgo=");
}

#[test]
fn image_openai_text_only_fallback() {
    // OpenAI lowering via text_content() only extracts text blocks
    let v = lower_to_openai(&image_conv(), &[]);
    let msg = &v["messages"][0];
    assert_eq!(msg["content"], "What is in this image?");
}

#[test]
fn image_only_message_claude() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        }],
    ));
    let v = lower_to_claude(&conv, &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "image");
}

#[test]
fn image_only_message_gemini() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        }],
    ));
    let v = lower_to_gemini(&conv, &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert!(parts[0].get("inline_data").is_some());
}

#[test]
fn image_media_type_preserved_claude() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/webp".into(),
            data: "webpdata".into(),
        }],
    ));
    let v = lower_to_claude(&conv, &[]);
    assert_eq!(
        v["messages"][0]["content"][0]["source"]["media_type"],
        "image/webp"
    );
}

#[test]
fn image_media_type_preserved_gemini() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/gif".into(),
            data: "gifdata".into(),
        }],
    ));
    let v = lower_to_gemini(&conv, &[]);
    assert_eq!(
        v["contents"][0]["parts"][0]["inline_data"]["mime_type"],
        "image/gif"
    );
}

// ── 11. Role mapping per dialect ───────────────────────────────────────

#[test]
fn role_system_all_dialects() {
    for d in all_dialects() {
        assert_eq!(ir_role_to_dialect(IrRole::System, *d), "system");
    }
}

#[test]
fn role_user_all_dialects() {
    for d in all_dialects() {
        assert_eq!(ir_role_to_dialect(IrRole::User, *d), "user");
    }
}

#[test]
fn role_assistant_openai_like() {
    for d in all_openai_like() {
        assert_eq!(ir_role_to_dialect(IrRole::Assistant, d), "assistant");
    }
}

#[test]
fn role_assistant_claude() {
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::Claude),
        "assistant"
    );
}

#[test]
fn role_assistant_gemini_model() {
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
        "model"
    );
}

#[test]
fn role_tool_openai_like() {
    for d in all_openai_like() {
        assert_eq!(ir_role_to_dialect(IrRole::Tool, d), "tool");
    }
}

#[test]
fn role_tool_claude_becomes_user() {
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
}

#[test]
fn role_tool_gemini_becomes_user() {
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Gemini), "user");
}

// ── 12. Dialect-specific field names ───────────────────────────────────

#[test]
fn openai_uses_messages_key() {
    let v = lower_to_openai(&single_user("hi"), &[]);
    assert!(v.get("messages").is_some());
    assert!(v.get("contents").is_none());
}

#[test]
fn gemini_uses_contents_key() {
    let v = lower_to_gemini(&single_user("hi"), &[]);
    assert!(v.get("contents").is_some());
    assert!(v.get("messages").is_none());
}

#[test]
fn claude_uses_messages_key() {
    let v = lower_to_claude(&single_user("hi"), &[]);
    assert!(v.get("messages").is_some());
    assert!(v.get("contents").is_none());
}

#[test]
fn openai_tool_uses_function_wrapper() {
    let v = lower_to_openai(&single_user("hi"), &sample_tools());
    let tool = &v["tools"][0];
    assert_eq!(tool["type"], "function");
    assert!(tool.get("function").is_some());
}

#[test]
fn claude_tool_no_function_wrapper() {
    let v = lower_to_claude(&single_user("hi"), &sample_tools());
    let tool = &v["tools"][0];
    assert!(tool.get("type").is_none());
    assert!(tool.get("function").is_none());
    assert!(tool.get("name").is_some());
}

#[test]
fn gemini_tool_wrapped_in_function_declarations() {
    let v = lower_to_gemini(&single_user("hi"), &sample_tools());
    assert!(v["tools"][0].get("function_declarations").is_some());
}

// ── 13. lower_for_dialect dispatch ─────────────────────────────────────

#[test]
fn lower_for_dialect_openai_matches_direct() {
    let conv = system_user_assistant();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::OpenAi, &conv, &tools),
        lower_to_openai(&conv, &tools)
    );
}

#[test]
fn lower_for_dialect_claude_matches_direct() {
    let conv = system_user_assistant();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::Claude, &conv, &tools),
        lower_to_claude(&conv, &tools)
    );
}

#[test]
fn lower_for_dialect_gemini_matches_direct() {
    let conv = system_user_assistant();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::Gemini, &conv, &tools),
        lower_to_gemini(&conv, &tools)
    );
}

#[test]
fn lower_for_dialect_kimi_matches_direct() {
    let conv = system_user_assistant();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::Kimi, &conv, &tools),
        lower_to_kimi(&conv, &tools)
    );
}

#[test]
fn lower_for_dialect_codex_matches_direct() {
    let conv = system_user_assistant();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::Codex, &conv, &tools),
        lower_to_codex(&conv, &tools)
    );
}

#[test]
fn lower_for_dialect_copilot_matches_direct() {
    let conv = system_user_assistant();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::Copilot, &conv, &tools),
        lower_to_copilot(&conv, &tools)
    );
}

// ── 14. OpenAI-like dialects produce identical output ──────────────────

#[test]
fn kimi_equals_openai_simple() {
    let conv = system_user_assistant();
    assert_eq!(lower_to_openai(&conv, &[]), lower_to_kimi(&conv, &[]));
}

#[test]
fn codex_equals_openai_simple() {
    let conv = system_user_assistant();
    assert_eq!(lower_to_openai(&conv, &[]), lower_to_codex(&conv, &[]));
}

#[test]
fn copilot_equals_openai_simple() {
    let conv = system_user_assistant();
    assert_eq!(lower_to_openai(&conv, &[]), lower_to_copilot(&conv, &[]));
}

#[test]
fn kimi_equals_openai_with_tools() {
    let conv = tool_use_conv();
    let tools = sample_tools();
    assert_eq!(lower_to_openai(&conv, &tools), lower_to_kimi(&conv, &tools));
}

#[test]
fn codex_equals_openai_with_tools() {
    let conv = tool_use_conv();
    let tools = sample_tools();
    assert_eq!(
        lower_to_openai(&conv, &tools),
        lower_to_codex(&conv, &tools)
    );
}

#[test]
fn copilot_equals_openai_with_tools() {
    let conv = tool_use_conv();
    let tools = sample_tools();
    assert_eq!(
        lower_to_openai(&conv, &tools),
        lower_to_copilot(&conv, &tools)
    );
}

// ── 15. Mixed content blocks ───────────────────────────────────────────

#[test]
fn mixed_text_and_image_claude() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "first text".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
            IrContentBlock::Text {
                text: "second text".into(),
            },
        ],
    ));
    let v = lower_to_claude(&conv, &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 3);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[2]["type"], "text");
}

#[test]
fn mixed_text_and_image_gemini() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "describe".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
        ],
    ));
    let v = lower_to_gemini(&conv, &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["text"], "describe");
    assert!(parts[1].get("inline_data").is_some());
}

#[test]
fn text_tool_use_thinking_claude() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "I will search".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            },
        ],
    ));
    let v = lower_to_claude(&conv, &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 3);
    assert_eq!(content[0]["type"], "thinking");
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[2]["type"], "tool_use");
}

#[test]
fn text_tool_use_thinking_gemini_skips_thinking() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "I will search".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            },
        ],
    ));
    let v = lower_to_gemini(&conv, &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["text"], "I will search");
    assert!(parts[1].get("functionCall").is_some());
}

// ── 16. Edge cases ─────────────────────────────────────────────────────

#[test]
fn empty_tool_input_openai() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "c1".into(),
            name: "get_time".into(),
            input: json!({}),
        }],
    ));
    let v = lower_to_openai(&conv, &[]);
    let args_str = v["messages"][0]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    assert_eq!(args_str, "{}");
}

#[test]
fn empty_tool_result_content_openai() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    ));
    let v = lower_to_openai(&conv, &[]);
    assert_eq!(v["messages"][0]["content"], "");
}

#[test]
fn empty_tool_result_content_claude() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    ));
    let v = lower_to_claude(&conv, &[]);
    let inner = v["messages"][0]["content"][0]["content"]
        .as_array()
        .unwrap();
    assert_eq!(inner.len(), 0);
}

#[test]
fn large_conversation_all_dialects() {
    let mut conv = IrConversation::new();
    conv = conv.push(IrMessage::text(IrRole::System, "System"));
    for i in 0..50 {
        conv = conv.push(IrMessage::text(IrRole::User, &format!("Q{i}")));
        conv = conv.push(IrMessage::text(IrRole::Assistant, &format!("A{i}")));
    }
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &conv, &[]);
        assert!(v.is_object());
    }
}

#[test]
fn special_characters_in_tool_name() {
    let tools = vec![IrToolDefinition {
        name: "my-tool_v2.0".into(),
        description: "A tool with special chars".into(),
        parameters: json!({"type": "object"}),
    }];
    let v = lower_to_openai(&single_user("hi"), &tools);
    assert_eq!(v["tools"][0]["function"]["name"], "my-tool_v2.0");
    let v = lower_to_claude(&single_user("hi"), &tools);
    assert_eq!(v["tools"][0]["name"], "my-tool_v2.0");
}

#[test]
fn tool_parameters_complex_schema() {
    let tools = vec![IrToolDefinition {
        name: "complex".into(),
        description: "Complex tool".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "properties": {
                        "deep": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        }),
    }];
    let v_openai = lower_to_openai(&single_user("hi"), &tools);
    assert_eq!(
        v_openai["tools"][0]["function"]["parameters"]["properties"]["nested"]["type"],
        "object"
    );
    let v_claude = lower_to_claude(&single_user("hi"), &tools);
    assert_eq!(
        v_claude["tools"][0]["input_schema"]["properties"]["nested"]["type"],
        "object"
    );
}

// ── 17. All dialects produce valid JSON objects ────────────────────────

#[test]
fn all_dialects_valid_json_empty() {
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &empty_conv(), &[]);
        assert!(v.is_object());
    }
}

#[test]
fn all_dialects_valid_json_full() {
    let conv = tool_use_conv();
    let tools = two_tools();
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &conv, &tools);
        assert!(v.is_object());
        // Verify it serializes cleanly
        let s = serde_json::to_string(&v).unwrap();
        assert!(!s.is_empty());
    }
}

#[test]
fn all_dialects_valid_json_thinking() {
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &thinking_conv(), &[]);
        assert!(v.is_object());
    }
}

#[test]
fn all_dialects_valid_json_image() {
    for d in all_dialects() {
        let v = lower_for_dialect(*d, &image_conv(), &[]);
        assert!(v.is_object());
    }
}

// ── 18. Full round-trip conversation ───────────────────────────────────

#[test]
fn full_tool_flow_openai() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a file assistant."))
        .push(IrMessage::text(IrRole::User, "Show me file.txt"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Reading file.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "file.txt"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Hello, world!".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "The file contains: Hello, world!",
        ));

    let v = lower_to_openai(&conv, &sample_tools());
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 5);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
    assert_eq!(msgs[3]["role"], "tool");
    assert_eq!(msgs[4]["role"], "assistant");
    assert!(v.get("tools").is_some());
}

#[test]
fn full_tool_flow_claude() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a file assistant."))
        .push(IrMessage::text(IrRole::User, "Show me file.txt"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Reading file.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "file.txt"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Hello, world!".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "The file contains: Hello, world!",
        ));

    let v = lower_to_claude(&conv, &sample_tools());
    assert_eq!(v["system"], "You are a file assistant.");
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 4); // system extracted
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[2]["role"], "user"); // tool result → user role
    assert_eq!(msgs[3]["role"], "assistant");
}

#[test]
fn full_tool_flow_gemini() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a file assistant."))
        .push(IrMessage::text(IrRole::User, "Show me file.txt"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Reading file.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "file.txt"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Hello, world!".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "The file contains: Hello, world!",
        ));

    let v = lower_to_gemini(&conv, &sample_tools());
    assert_eq!(
        v["system_instruction"]["parts"][0]["text"],
        "You are a file assistant."
    );
    let contents = v["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 4);
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[2]["role"], "user"); // tool result → user
    assert_eq!(contents[3]["role"], "model");
}

// ── 19. JSON serialization sanity ──────────────────────────────────────

#[test]
fn serialized_output_is_valid_json_all() {
    let convs = vec![
        empty_conv(),
        single_user("test"),
        system_user_assistant(),
        multi_turn(),
        tool_use_conv(),
        thinking_conv(),
        image_conv(),
    ];
    for conv in &convs {
        for d in all_dialects() {
            let v = lower_for_dialect(*d, conv, &sample_tools());
            let s = serde_json::to_string_pretty(&v).unwrap();
            let _: Value = serde_json::from_str(&s).unwrap();
        }
    }
}

// ── 20. Deterministic output ───────────────────────────────────────────

#[test]
fn deterministic_output_all_dialects() {
    let conv = tool_use_conv();
    let tools = sample_tools();
    for d in all_dialects() {
        let a = lower_for_dialect(*d, &conv, &tools);
        let b = lower_for_dialect(*d, &conv, &tools);
        assert_eq!(a, b, "{d}: lowering should be deterministic");
    }
}

// ── 21. Tool result with nested non-text blocks ────────────────────────

#[test]
fn tool_result_nested_non_text_claude_unknown() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data".into(),
            }],
            is_error: false,
        }],
    ));
    let v = lower_to_claude(&conv, &[]);
    let inner = v["messages"][0]["content"][0]["content"]
        .as_array()
        .unwrap();
    assert_eq!(inner[0]["type"], "unknown");
}

#[test]
fn tool_result_nested_non_text_gemini_skips() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data".into(),
            }],
            is_error: false,
        }],
    ));
    let v = lower_to_gemini(&conv, &[]);
    // functionResponse text is empty since no text blocks in content
    assert_eq!(
        v["contents"][0]["parts"][0]["functionResponse"]["response"]["result"],
        ""
    );
}

// ── 22. System-only conversation ───────────────────────────────────────

#[test]
fn system_only_claude_extracts_and_empty_messages() {
    let conv = single_system("Be helpful.");
    let v = lower_to_claude(&conv, &[]);
    assert_eq!(v["system"], "Be helpful.");
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn system_only_gemini_extracts_and_empty_contents() {
    let conv = single_system("Be helpful.");
    let v = lower_to_gemini(&conv, &[]);
    assert_eq!(v["system_instruction"]["parts"][0]["text"], "Be helpful.");
    assert_eq!(v["contents"].as_array().unwrap().len(), 0);
}

#[test]
fn system_only_openai_keeps_inline() {
    let conv = single_system("Be helpful.");
    let v = lower_to_openai(&conv, &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["role"], "system");
}
