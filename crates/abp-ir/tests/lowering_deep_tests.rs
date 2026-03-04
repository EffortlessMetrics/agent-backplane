// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for SDK lowering functions in `abp_ir::lower`.
//!
//! Covers OpenAI, Claude, Gemini, Codex/Kimi/Copilot lowering, cross-dialect
//! roundtrips, edge cases, normalization-before-lowering, and error/boundary
//! conditions.

use abp_ir::lower::*;
use abp_ir::normalize;
use abp_ir::*;
use abp_sdk_types::Dialect;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn tool_def(name: &str, desc: &str) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: json!({"type": "object", "properties": {}}),
    }
}

fn tool_def_with_params(name: &str, desc: &str, params: serde_json::Value) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: params,
    }
}

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hi"))
        .push(IrMessage::text(IrRole::Assistant, "Hello!"))
}

fn tool_use_block(id: &str, name: &str, input: serde_json::Value) -> IrContentBlock {
    IrContentBlock::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn tool_result_block(tool_use_id: &str, text: &str, is_error: bool) -> IrContentBlock {
    IrContentBlock::ToolResult {
        tool_use_id: tool_use_id.into(),
        content: vec![IrContentBlock::Text { text: text.into() }],
        is_error,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: OpenAI lowering
// ═══════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
        assert!(lowered.get("tools").is_none());
    }

    #[test]
    fn single_user_message() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "ping"));
        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "ping");
    }

    #[test]
    fn multi_turn_conversation() {
        let lowered = lower_to_openai(&simple_conv(), &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
    }

    #[test]
    fn system_message_inline() {
        let lowered = lower_to_openai(&simple_conv(), &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
    }

    #[test]
    fn tool_calls_in_assistant_message() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Checking.".into(),
                },
                tool_use_block("c1", "search", json!({"q": "rust"})),
                tool_use_block("c2", "read", json!({"path": "/a"})),
            ],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msg = &lowered["messages"][0];
        assert_eq!(msg["content"], "Checking.");
        let tc = msg["tool_calls"].as_array().unwrap();
        assert_eq!(tc.len(), 2);
        assert_eq!(tc[0]["id"], "c1");
        assert_eq!(tc[0]["function"]["name"], "search");
        assert_eq!(tc[1]["id"], "c2");
        assert_eq!(tc[1]["function"]["name"], "read");
    }

    #[test]
    fn tool_calls_arguments_is_string() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "calc", json!({"expr": "1+1"}))],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let args = &lowered["messages"][0]["tool_calls"][0]["function"]["arguments"];
        assert!(args.is_string(), "arguments should be a JSON string");
    }

    #[test]
    fn tool_result_produces_tool_role() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("c1", "42", false)],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msg = &lowered["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "c1");
        assert_eq!(msg["content"], "42");
    }

    #[test]
    fn multiple_tool_results_expand_to_multiple_messages() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![
                tool_result_block("c1", "result1", false),
                tool_result_block("c2", "result2", false),
            ],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["tool_call_id"], "c1");
        assert_eq!(msgs[1]["tool_call_id"], "c2");
    }

    #[test]
    fn tools_array_present_when_provided() {
        let tools = vec![tool_def("calc", "Calculator")];
        let lowered = lower_to_openai(&IrConversation::new(), &tools);
        let t = lowered["tools"].as_array().unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0]["type"], "function");
        assert_eq!(t[0]["function"]["name"], "calc");
        assert_eq!(t[0]["function"]["description"], "Calculator");
    }

    #[test]
    fn tools_absent_when_empty() {
        let lowered = lower_to_openai(&IrConversation::new(), &[]);
        assert!(lowered.get("tools").is_none());
    }

    #[test]
    fn multiple_tools() {
        let tools = vec![
            tool_def("search", "Search the web"),
            tool_def("calc", "Calculator"),
        ];
        let lowered = lower_to_openai(&IrConversation::new(), &tools);
        assert_eq!(lowered["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn tool_parameters_preserved() {
        let params =
            json!({"type": "object", "properties": {"x": {"type": "number"}}, "required": ["x"]});
        let tools = vec![tool_def_with_params("fn1", "desc", params.clone())];
        let lowered = lower_to_openai(&IrConversation::new(), &tools);
        assert_eq!(lowered["tools"][0]["function"]["parameters"], params);
    }

    #[test]
    fn assistant_only_tool_calls_no_content_field() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "search", json!({}))],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msg = &lowered["messages"][0];
        // When there's no text, content should not be set (or empty string)
        assert!(msg.get("content").is_none() || msg["content"].as_str().unwrap().is_empty());
        assert!(msg.get("tool_calls").is_some());
    }

    #[test]
    fn role_mapping_all_ir_roles() {
        assert_eq!(
            ir_role_to_dialect(IrRole::System, Dialect::OpenAi),
            "system"
        );
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::OpenAi), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::OpenAi),
            "assistant"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi), "tool");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Claude lowering
// ═══════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;

    #[test]
    fn system_extracted_to_top_level() {
        let lowered = lower_to_claude(&simple_conv(), &[]);
        assert_eq!(lowered["system"], "You are helpful.");
    }

    #[test]
    fn system_messages_not_in_messages_array() {
        let lowered = lower_to_claude(&simple_conv(), &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn no_system_message_omits_system_field() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let lowered = lower_to_claude(&conv, &[]);
        assert!(lowered.get("system").is_none());
    }

    #[test]
    fn messages_format_with_content_blocks() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let lowered = lower_to_claude(&conv, &[]);
        let msg = &lowered["messages"][0];
        assert_eq!(msg["role"], "user");
        let content = msg["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "hello");
    }

    #[test]
    fn tool_use_content_block() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("tu1", "search", json!({"q": "rust"}))],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "tu1");
        assert_eq!(block["name"], "search");
        assert_eq!(block["input"], json!({"q": "rust"}));
    }

    #[test]
    fn tool_result_content_block() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("tu1", "found it", false)],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "tu1");
        assert_eq!(block["is_error"], false);
        let inner = block["content"].as_array().unwrap();
        assert_eq!(inner[0]["type"], "text");
        assert_eq!(inner[0]["text"], "found it");
    }

    #[test]
    fn tool_result_error_flag() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("tu1", "failed", true)],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["is_error"], true);
    }

    #[test]
    fn tool_role_maps_to_user() {
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
    }

    #[test]
    fn tools_use_input_schema_not_parameters() {
        let tools = vec![tool_def("fn1", "desc")];
        let lowered = lower_to_claude(&IrConversation::new(), &tools);
        let t = &lowered["tools"][0];
        assert!(t.get("input_schema").is_some());
        assert!(t.get("parameters").is_none());
    }

    #[test]
    fn image_content_block() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBOR...".into(),
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert_eq!(block["source"]["data"], "iVBOR...");
    }

    #[test]
    fn thinking_content_block() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "Let me think...".into(),
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "thinking");
        assert_eq!(block["thinking"], "Let me think...");
    }

    #[test]
    fn mixed_text_and_tool_use_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "I'll search".into(),
                },
                tool_use_block("c1", "search", json!({"q": "x"})),
                IrContentBlock::Text {
                    text: "and also read".into(),
                },
            ],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[2]["type"], "text");
    }

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
        assert!(lowered.get("system").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Gemini lowering
// ═══════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;

    #[test]
    fn contents_format() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let lowered = lower_to_gemini(&conv, &[]);
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn system_instruction_extracted() {
        let lowered = lower_to_gemini(&simple_conv(), &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"],
            "You are helpful."
        );
    }

    #[test]
    fn system_messages_excluded_from_contents() {
        let lowered = lower_to_gemini(&simple_conv(), &[]);
        let contents = lowered["contents"].as_array().unwrap();
        assert!(contents.iter().all(|c| c["role"] != "system"));
    }

    #[test]
    fn no_system_omits_system_instruction() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let lowered = lower_to_gemini(&conv, &[]);
        assert!(lowered.get("system_instruction").is_none());
    }

    #[test]
    fn parts_structure_text() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "hello");
    }

    #[test]
    fn function_call_part() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "search", json!({"q": "test"}))],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["functionCall"]["name"], "search");
        assert_eq!(part["functionCall"]["args"], json!({"q": "test"}));
    }

    #[test]
    fn function_response_part() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("search", "found 3 results", false)],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["functionResponse"]["name"], "search");
        assert_eq!(
            part["functionResponse"]["response"]["result"],
            "found 3 results"
        );
    }

    #[test]
    fn thinking_blocks_filtered_out() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking...".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "answer");
    }

    #[test]
    fn tools_use_function_declarations() {
        let tools = vec![tool_def("calc", "Math")];
        let lowered = lower_to_gemini(&IrConversation::new(), &tools);
        let decls = lowered["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0]["name"], "calc");
        assert_eq!(decls[0]["description"], "Math");
    }

    #[test]
    fn assistant_role_becomes_model() {
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
            "model"
        );
    }

    #[test]
    fn tool_role_becomes_user() {
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Gemini), "user");
    }

    #[test]
    fn image_part() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "base64data".into(),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["inline_data"]["mime_type"], "image/jpeg");
        assert_eq!(part["inline_data"]["data"], "base64data");
    }

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(lowered["contents"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn multiple_function_declarations() {
        let tools = vec![
            tool_def("search", "Search"),
            tool_def("calc", "Calc"),
            tool_def("read", "Read file"),
        ];
        let lowered = lower_to_gemini(&IrConversation::new(), &tools);
        let decls = lowered["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Codex / Kimi / Copilot lowering
// ═══════════════════════════════════════════════════════════════════════

mod openai_compatible_aliases {
    use super::*;

    #[test]
    fn kimi_matches_openai() {
        let conv = simple_conv();
        let tools = vec![tool_def("t1", "d1")];
        assert_eq!(lower_to_kimi(&conv, &tools), lower_to_openai(&conv, &tools));
    }

    #[test]
    fn codex_matches_openai() {
        let conv = simple_conv();
        let tools = vec![tool_def("t1", "d1")];
        assert_eq!(
            lower_to_codex(&conv, &tools),
            lower_to_openai(&conv, &tools)
        );
    }

    #[test]
    fn copilot_matches_openai() {
        let conv = simple_conv();
        let tools = vec![tool_def("t1", "d1")];
        assert_eq!(
            lower_to_copilot(&conv, &tools),
            lower_to_openai(&conv, &tools)
        );
    }

    #[test]
    fn kimi_empty_conversation() {
        let lowered = lower_to_kimi(&IrConversation::new(), &[]);
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn codex_with_tools() {
        let tools = vec![tool_def("compile", "Compile code")];
        let lowered = lower_to_codex(&IrConversation::new(), &tools);
        assert_eq!(lowered["tools"][0]["function"]["name"], "compile");
    }

    #[test]
    fn copilot_message_format() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "help"))
            .push(IrMessage::text(IrRole::Assistant, "sure"));
        let lowered = lower_to_copilot(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
    }

    #[test]
    fn kimi_role_mapping() {
        assert_eq!(ir_role_to_dialect(IrRole::System, Dialect::Kimi), "system");
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Kimi), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Kimi),
            "assistant"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Kimi), "tool");
    }

    #[test]
    fn codex_role_mapping() {
        assert_eq!(ir_role_to_dialect(IrRole::System, Dialect::Codex), "system");
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Codex), "tool");
    }

    #[test]
    fn copilot_role_mapping() {
        assert_eq!(
            ir_role_to_dialect(IrRole::System, Dialect::Copilot),
            "system"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Copilot), "tool");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Cross-dialect roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod cross_dialect {
    use super::*;

    #[test]
    fn lower_to_openai_parse_back_messages() {
        let conv = simple_conv();
        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();

        // Parse back: verify structure survives round-trip
        let mut rebuilt = IrConversation::new();
        for m in msgs {
            let role = match m["role"].as_str().unwrap() {
                "system" => IrRole::System,
                "user" => IrRole::User,
                "assistant" => IrRole::Assistant,
                "tool" => IrRole::Tool,
                _ => panic!("unexpected role"),
            };
            let text = m["content"].as_str().unwrap_or("");
            rebuilt = rebuilt.push(IrMessage::text(role, text));
        }
        assert_eq!(rebuilt.len(), conv.len());
        assert_eq!(
            rebuilt.system_message().unwrap().text_content(),
            conv.system_message().unwrap().text_content()
        );
    }

    #[test]
    fn openai_claude_same_user_text() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello world"));
        let oai = lower_to_openai(&conv, &[]);
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(oai["messages"][0]["content"], "hello world");
        assert_eq!(claude["messages"][0]["content"][0]["text"], "hello world");
    }

    #[test]
    fn openai_gemini_same_user_text() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello world"));
        let oai = lower_to_openai(&conv, &[]);
        let gem = lower_to_gemini(&conv, &[]);
        assert_eq!(oai["messages"][0]["content"], "hello world");
        assert_eq!(gem["contents"][0]["parts"][0]["text"], "hello world");
    }

    #[test]
    fn all_dialects_preserve_user_count() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "a"))
            .push(IrMessage::text(IrRole::Assistant, "b"))
            .push(IrMessage::text(IrRole::User, "c"));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            let key = if *dialect == Dialect::Gemini {
                "contents"
            } else {
                "messages"
            };
            assert_eq!(
                lowered[key].as_array().unwrap().len(),
                3,
                "{dialect}: message count mismatch"
            );
        }
    }

    #[test]
    fn lower_for_dialect_dispatches_correctly() {
        let conv = simple_conv();
        let tools = vec![tool_def("t", "d")];
        assert_eq!(
            lower_for_dialect(Dialect::OpenAi, &conv, &tools),
            lower_to_openai(&conv, &tools)
        );
        assert_eq!(
            lower_for_dialect(Dialect::Claude, &conv, &tools),
            lower_to_claude(&conv, &tools)
        );
        assert_eq!(
            lower_for_dialect(Dialect::Gemini, &conv, &tools),
            lower_to_gemini(&conv, &tools)
        );
        assert_eq!(
            lower_for_dialect(Dialect::Kimi, &conv, &tools),
            lower_to_kimi(&conv, &tools)
        );
        assert_eq!(
            lower_for_dialect(Dialect::Codex, &conv, &tools),
            lower_to_codex(&conv, &tools)
        );
        assert_eq!(
            lower_for_dialect(Dialect::Copilot, &conv, &tools),
            lower_to_copilot(&conv, &tools)
        );
    }

    #[test]
    fn all_dialects_produce_json_objects() {
        let conv = simple_conv();
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(lowered.is_object(), "{dialect}: should produce JSON object");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Edge cases
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_text_message_openai() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, ""));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"], "");
    }

    #[test]
    fn empty_text_message_claude() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, ""));
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"][0]["text"], "");
    }

    #[test]
    fn unicode_text_preserved_openai() {
        let text = "こんにちは 🌍 مرحبا Привет";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"], text);
    }

    #[test]
    fn unicode_text_preserved_claude() {
        let text = "中文测试 🎉 Ñoño";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"][0]["text"], text);
    }

    #[test]
    fn unicode_text_preserved_gemini() {
        let text = "日本語テスト 🤖 Ölüm";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(lowered["contents"][0]["parts"][0]["text"], text);
    }

    #[test]
    fn very_long_message() {
        let long_text = "x".repeat(100_000);
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &long_text));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(
            lowered["messages"][0]["content"].as_str().unwrap().len(),
            100_000
        );
    }

    #[test]
    fn nested_tool_result_content() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![
                    IrContentBlock::Text {
                        text: "line1\n".into(),
                    },
                    IrContentBlock::Text {
                        text: "line2".into(),
                    },
                ],
                is_error: false,
            }],
        ));
        // OpenAI: concatenates text blocks
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(oai["messages"][0]["content"], "line1\nline2");
        // Claude: preserves each as typed block
        let claude = lower_to_claude(&conv, &[]);
        let inner = claude["messages"][0]["content"][0]["content"]
            .as_array()
            .unwrap();
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn special_characters_in_tool_names() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "my-tool_v2.0", json!({}))],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(
            lowered["messages"][0]["tool_calls"][0]["function"]["name"],
            "my-tool_v2.0"
        );
    }

    #[test]
    fn newlines_in_text_content() {
        let text = "line1\nline2\n\nline4";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"], text);
    }

    #[test]
    fn json_special_chars_in_text() {
        let text = r#"She said "hello" and \n escaped"#;
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"], text);
    }

    #[test]
    fn empty_tool_input() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "noop", json!({}))],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let args = &lowered["messages"][0]["tool_calls"][0]["function"]["arguments"];
        assert_eq!(args, "{}");
    }

    #[test]
    fn message_with_only_thinking_block_gemini() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "deep thought".into(),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(
            parts.len(),
            0,
            "thinking blocks should be filtered in Gemini"
        );
    }

    #[test]
    fn metadata_not_leaked_into_lowered_output() {
        let mut meta = BTreeMap::new();
        meta.insert("vendor_id".to_string(), json!("secret123"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);

        let oai = lower_to_openai(&conv, &[]);
        let oai_str = serde_json::to_string(&oai).unwrap();
        assert!(!oai_str.contains("secret123"));

        let claude = lower_to_claude(&conv, &[]);
        let claude_str = serde_json::to_string(&claude).unwrap();
        assert!(!claude_str.contains("secret123"));

        let gem = lower_to_gemini(&conv, &[]);
        let gem_str = serde_json::to_string(&gem).unwrap();
        assert!(!gem_str.contains("secret123"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Normalization before lowering
// ═══════════════════════════════════════════════════════════════════════

mod normalize_then_lower {
    use super::*;

    #[test]
    fn dedup_system_then_lower_openai() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Rule 1."))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "Rule 2."));
        let normalized = normalize::dedup_system(&conv);
        let lowered = lower_to_openai(&normalized, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        // Only one system message after dedup
        let sys_count = msgs.iter().filter(|m| m["role"] == "system").count();
        assert_eq!(sys_count, 1);
        assert_eq!(msgs[0]["content"], "Rule 1.\nRule 2.");
    }

    #[test]
    fn dedup_system_then_lower_claude() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "A"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "B"));
        let normalized = normalize::dedup_system(&conv);
        let lowered = lower_to_claude(&normalized, &[]);
        assert_eq!(lowered["system"], "A\nB");
    }

    #[test]
    fn trim_then_lower() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello world  "));
        let normalized = normalize::trim_text(&conv);
        let lowered = lower_to_openai(&normalized, &[]);
        assert_eq!(lowered["messages"][0]["content"], "hello world");
    }

    #[test]
    fn strip_empty_then_lower() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::new(IrRole::Assistant, vec![]))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let normalized = normalize::strip_empty(&conv);
        let lowered = lower_to_openai(&normalized, &[]);
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn full_pipeline_then_lower() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys1  "))
            .push(IrMessage::text(IrRole::User, "  hi  "))
            .push(IrMessage::text(IrRole::System, "  sys2  "))
            .push(IrMessage::new(IrRole::Assistant, vec![]));
        let normalized = normalize::normalize(&conv);
        let lowered = lower_to_openai(&normalized, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        // Deduped systems + stripped empty = system, user
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        // dedup_system joins raw texts, trim_text only trims outer whitespace
        assert_eq!(msgs[0]["content"], "sys1  \n  sys2");
        assert_eq!(msgs[1]["content"], "hi");
    }

    #[test]
    fn normalize_idempotent_on_lowered_structure() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " q "));
        let once = normalize::normalize(&conv);
        let twice = normalize::normalize(&once);
        assert_eq!(lower_to_openai(&once, &[]), lower_to_openai(&twice, &[]));
    }

    #[test]
    fn merge_adjacent_text_then_lower_claude() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "hello ".into(),
                },
                IrContentBlock::Text {
                    text: "world".into(),
                },
            ],
        ));
        let normalized = normalize::merge_adjacent_text(&conv);
        let lowered = lower_to_claude(&normalized, &[]);
        let content = lowered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"], "hello world");
    }

    #[test]
    fn normalize_tools_then_lower() {
        let tools = vec![IrToolDefinition {
            name: "search".into(),
            description: "Search".into(),
            parameters: json!({"properties": {"q": {"type": "string"}}}),
        }];
        let normalized = normalize::normalize_tool_schemas(&tools);
        let lowered = lower_to_openai(&IrConversation::new(), &normalized);
        assert_eq!(
            lowered["tools"][0]["function"]["parameters"]["type"],
            "object"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: Error / boundary cases
// ═══════════════════════════════════════════════════════════════════════

mod error_and_boundary {
    use super::*;

    #[test]
    fn no_tools_field_for_any_dialect_when_empty() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(
                lowered.get("tools").is_none(),
                "{dialect}: tools field should be absent"
            );
        }
    }

    #[test]
    fn all_dialects_handle_empty_conversation() {
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &IrConversation::new(), &[]);
            let key = if *dialect == Dialect::Gemini {
                "contents"
            } else {
                "messages"
            };
            assert_eq!(
                lowered[key].as_array().unwrap().len(),
                0,
                "{dialect}: should handle empty conversation"
            );
        }
    }

    #[test]
    fn tool_result_with_empty_content_openai() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![],
                is_error: false,
            }],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"], "");
    }

    #[test]
    fn tool_result_with_empty_content_claude() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![],
                is_error: false,
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["content"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn conversation_with_only_system_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys1"))
            .push(IrMessage::text(IrRole::System, "sys2"));

        // OpenAI: both system messages inline
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(oai["messages"].as_array().unwrap().len(), 2);

        // Claude: extracted, no messages
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(claude["messages"].as_array().unwrap().len(), 0);

        // Gemini: extracted, no contents
        let gem = lower_to_gemini(&conv, &[]);
        assert_eq!(gem["contents"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn message_with_no_content_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(IrRole::User, vec![]));
        let lowered = lower_to_openai(&conv, &[]);
        let msg = &lowered["messages"][0];
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["content"], "");
    }

    #[test]
    fn all_role_strings_valid_for_all_dialects() {
        let roles = [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ];
        for dialect in Dialect::all() {
            for role in &roles {
                let s = ir_role_to_dialect(*role, *dialect);
                assert!(
                    !s.is_empty(),
                    "{dialect}/{role:?}: role string should be non-empty"
                );
            }
        }
    }

    #[test]
    fn lowered_output_is_valid_json_for_all_dialects() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hello"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "checking".into(),
                    },
                    tool_use_block("c1", "search", json!({"q": "x"})),
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("c1", "result", false)],
            ));
        let tools = vec![tool_def("search", "Search")];
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            // Serialize + deserialize to confirm valid JSON
            let serialized = serde_json::to_string(&lowered).unwrap();
            let _: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        }
    }

    #[test]
    fn complex_tool_input_preserved() {
        let input = json!({
            "query": "SELECT * FROM users",
            "options": {
                "limit": 10,
                "nested": {"deep": [1, 2, 3]}
            }
        });
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "db_query", input.clone())],
        ));
        // OpenAI: arguments is a JSON string
        let oai = lower_to_openai(&conv, &[]);
        let args_str = oai["messages"][0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(parsed, input);

        // Claude: input is kept as object
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(claude["messages"][0]["content"][0]["input"], input);

        // Gemini: args is kept as object
        let gem = lower_to_gemini(&conv, &[]);
        assert_eq!(
            gem["contents"][0]["parts"][0]["functionCall"]["args"],
            input
        );
    }
}
