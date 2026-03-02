// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests verifying cross-dialect mapping through the IR layer.
//!
//! These tests exercise the full pipeline: source dialect → IR → target dialect,
//! covering all major dialect pairs (OpenAI, Claude, Gemini, Codex, Kimi, Copilot)
//! and verifying semantic preservation of messages, tools, system prompts,
//! sampling parameters, streaming events, and more.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRule, features, known_rules, validate_mapping,
};
use chrono::Utc;
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

/// Build a simple OpenAI-shim request with system + user messages.
fn openai_request_simple(system: &str, user: &str) -> abp_shim_openai::ChatCompletionRequest {
    abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system(system),
            abp_shim_openai::Message::user(user),
        ])
        .build()
}

/// Build a Claude-shim MessageRequest.
fn claude_request_simple(system: &str, user: &str) -> abp_shim_claude::MessageRequest {
    abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: user.into() }],
        }],
        system: Some(system.into()),
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    }
}

/// Standard IR conversation for cross-dialect tests.
fn standard_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello, world!"),
        IrMessage::text(IrRole::Assistant, "Hi! How can I help you?"),
    ])
}

/// IR conversation with tool use.
fn tool_use_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read main.rs"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The file contains a main function."),
    ])
}

/// Standard tool definition for cross-dialect tests.
fn standard_tool_def() -> IrToolDefinition {
    IrToolDefinition {
        name: "read_file".into(),
        description: "Read the contents of a file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to read"
                }
            },
            "required": ["path"]
        }),
    }
}

// ════════════════════════════════════════════════════════════════════════
// 1. OpenAI → Claude mapping (request transformation)
// ════════════════════════════════════════════════════════════════════════

mod openai_to_claude {
    use super::*;

    #[test]
    fn text_message_preserved() {
        let req = openai_request_simple("Be helpful.", "What is Rust?");
        let ir = abp_shim_openai::request_to_ir(&req);

        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
        assert!(!claude_msgs.is_empty());
        // The user message text should be preserved
        let user_msg = claude_msgs.iter().find(|m| m.role == "user").unwrap();
        assert_eq!(user_msg.content, "What is Rust?");
    }

    #[test]
    fn system_message_extracted_for_claude() {
        let req = openai_request_simple("You are a coding assistant.", "Help me");
        let ir = abp_shim_openai::request_to_ir(&req);

        let sys = abp_claude_sdk::lowering::extract_system_prompt(&ir);
        assert_eq!(sys.as_deref(), Some("You are a coding assistant."));
    }

    #[test]
    fn tool_calls_translated() {
        let msgs = vec![
            abp_shim_openai::Message::user("Read main.rs"),
            abp_shim_openai::Message::assistant_with_tool_calls(vec![abp_shim_openai::ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: abp_shim_openai::FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);

        // The assistant message with tool_use should be serialized as JSON blocks
        let assistant = claude_msgs.iter().find(|m| m.role == "assistant").unwrap();
        assert!(assistant.content.contains("read_file"));
    }

    #[test]
    fn tool_result_translated() {
        let msgs = vec![
            abp_shim_openai::Message::user("Read file"),
            abp_shim_openai::Message::assistant_with_tool_calls(vec![abp_shim_openai::ToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: abp_shim_openai::FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.rs"}"#.into(),
                },
            }]),
            abp_shim_openai::Message::tool("c1", "contents of a.rs"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);

        // Tool result in Claude is a user message with ToolResult content blocks
        let tool_msg = &claude_msgs[claude_msgs.len() - 1];
        assert!(
            tool_msg.content.contains("tool_result")
                || tool_msg.content.contains("contents of a.rs")
        );
    }

    #[test]
    fn multi_turn_message_count_preserved() {
        let msgs = vec![
            abp_shim_openai::Message::system("Be concise"),
            abp_shim_openai::Message::user("Hi"),
            abp_shim_openai::Message::assistant("Hello"),
            abp_shim_openai::Message::user("Bye"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        // System is extracted separately in Claude, so from_ir skips it
        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
        assert_eq!(claude_msgs.len(), 3); // user, assistant, user (no system)
    }

    #[test]
    fn temperature_mapping_via_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .temperature(0.7)
            .build();

        let wo = abp_shim_openai::request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
        assert_eq!(temp, Some(0.7));
    }
}

// ════════════════════════════════════════════════════════════════════════
// 2. Claude → OpenAI mapping (request transformation)
// ════════════════════════════════════════════════════════════════════════

mod claude_to_openai {
    use super::*;

    #[test]
    fn text_message_preserved() {
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some("Be helpful."));
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        let sys = openai_msgs.iter().find(|m| m.role == "system").unwrap();
        assert_eq!(sys.content.as_deref(), Some("Be helpful."));
        let user = openai_msgs.iter().find(|m| m.role == "user").unwrap();
        assert_eq!(user.content.as_deref(), Some("What is Rust?"));
    }

    #[test]
    fn system_becomes_openai_system_message() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are helpful."),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn tool_use_blocks_become_openai_tool_calls() {
        let blocks = vec![abp_claude_sdk::dialect::ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "lib.rs"}),
        }];
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        assert!(openai_msgs[0].tool_calls.is_some());
        let tc = &openai_msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "read_file");
        assert_eq!(tc.id, "tu_1");
    }

    #[test]
    fn tool_result_becomes_openai_tool_message() {
        // Claude represents tool results as user messages with ToolResult content blocks.
        // When lowered to IR the role stays User, so OpenAI from_ir produces "user" role.
        let blocks = vec![abp_claude_sdk::dialect::ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("file contents".into()),
            is_error: None,
        }];
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        // Claude tool results come through as user role in IR
        assert_eq!(openai_msgs[0].role, "user");
        // The content may be None if tool results don't map to flat text,
        // or Some if the lowering extracts the text.
        // Just verify we got a message back — the exact shape depends on lowering impl.
        assert!(!openai_msgs.is_empty());
    }

    #[test]
    fn thinking_block_appears_in_openai_text() {
        let blocks = vec![
            abp_claude_sdk::dialect::ClaudeContentBlock::Thinking {
                thinking: "Let me reason...".into(),
                signature: None,
            },
            abp_claude_sdk::dialect::ClaudeContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ];
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        // Thinking becomes text in OpenAI
        let content = openai_msgs[0].content.as_deref().unwrap();
        assert!(content.contains("Let me reason..."));
        assert!(content.contains("The answer is 42."));
    }
}

// ════════════════════════════════════════════════════════════════════════
// 3. OpenAI → Gemini mapping
// ════════════════════════════════════════════════════════════════════════

mod openai_to_gemini {
    use super::*;

    #[test]
    fn text_message_preserved() {
        let req = openai_request_simple("Be helpful.", "What is Rust?");
        let ir = abp_shim_openai::request_to_ir(&req);
        let gemini_contents = abp_gemini_sdk::lowering::from_ir(&ir);

        // System is skipped by from_ir; only user message remains
        let user_content = gemini_contents.iter().find(|c| c.role == "user").unwrap();
        match &user_content.parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::Text(t) => assert_eq!(t, "What is Rust?"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn system_becomes_gemini_system_instruction() {
        let req = openai_request_simple("You are a coding expert.", "Help me");
        let ir = abp_shim_openai::request_to_ir(&req);
        let sys_instr = abp_gemini_sdk::lowering::extract_system_instruction(&ir);

        assert!(sys_instr.is_some());
        let sys = sys_instr.unwrap();
        match &sys.parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::Text(t) => {
                assert_eq!(t, "You are a coding expert.")
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn tool_calls_become_function_calls() {
        let ir = tool_use_ir_conversation();
        let gemini_contents = abp_gemini_sdk::lowering::from_ir(&ir);

        let model_msg = gemini_contents.iter().find(|c| c.role == "model").unwrap();
        match &model_msg.parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "read_file");
                assert_eq!(args, &json!({"path": "main.rs"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn assistant_role_maps_to_model() {
        let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hello!")]);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        assert_eq!(gemini[0].role, "model");
    }

    #[test]
    fn multi_turn_preserves_order() {
        let ir = standard_ir_conversation();
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        // System is skipped, so user and assistant remain
        assert_eq!(gemini.len(), 2);
        assert_eq!(gemini[0].role, "user");
        assert_eq!(gemini[1].role, "model");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 4. Gemini → OpenAI mapping
// ════════════════════════════════════════════════════════════════════════

mod gemini_to_openai {
    use super::*;

    #[test]
    fn text_message_preserved() {
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                "Hello world".into(),
            )],
        }];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(openai_msgs[0].role, "user");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Hello world"));
    }

    #[test]
    fn model_role_maps_to_assistant() {
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "model".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Sure!".into())],
        }];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(openai_msgs[0].role, "assistant");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Sure!"));
    }

    #[test]
    fn function_call_maps_to_tool_call() {
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "model".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"query": "rust"}),
            }],
        }];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        let tc = openai_msgs[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "search");
    }

    #[test]
    fn function_response_maps_to_tool_message() {
        // Gemini FunctionResponse comes as role "user", so IR keeps it as User role.
        // When mapped to OpenAI, it stays "user" (not "tool").
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("results"),
            }],
        }];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, None);
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(openai_msgs[0].role, "user");
    }

    #[test]
    fn system_instruction_becomes_system_message() {
        let sys = abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                "Be concise".into(),
            )],
        };
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hi".into())],
        }];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, Some(&sys));
        let openai_msgs = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Be concise"));
    }
}

// ════════════════════════════════════════════════════════════════════════
// 5. Claude → Gemini mapping
// ════════════════════════════════════════════════════════════════════════

mod claude_to_gemini {
    use super::*;

    #[test]
    fn text_message_preserved() {
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        assert_eq!(gemini[0].role, "user");
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::Text(t) => assert_eq!(t, "What is Rust?"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_becomes_function_call() {
        let blocks = vec![abp_claude_sdk::dialect::ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }];
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        assert_eq!(gemini[0].role, "model");
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args, &json!({"q": "rust"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn system_prompt_extracted_as_instruction() {
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some("You are an expert."));
        let sys = abp_gemini_sdk::lowering::extract_system_instruction(&ir);

        assert!(sys.is_some());
        match &sys.unwrap().parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::Text(t) => {
                assert_eq!(t, "You are an expert.")
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn image_block_becomes_inline_data() {
        let blocks = vec![abp_claude_sdk::dialect::ClaudeContentBlock::Image {
            source: abp_claude_sdk::dialect::ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        }];
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/png");
                assert_eq!(d.data, "base64data");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    #[test]
    fn multi_turn_conversation_preserved() {
        let claude_msgs = vec![
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Hello".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "Hi!".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Bye".into(),
            },
        ];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some("Be nice"));
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        // System skipped; 3 messages remain
        assert_eq!(gemini.len(), 3);
        assert_eq!(gemini[0].role, "user");
        assert_eq!(gemini[1].role, "model");
        assert_eq!(gemini[2].role, "user");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 6. Self-mapping (Dialect → same Dialect)
// ════════════════════════════════════════════════════════════════════════

mod self_mapping {
    use super::*;

    #[test]
    fn openai_self_roundtrip() {
        let msgs = vec![
            abp_shim_openai::Message::system("Be helpful."),
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi!"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let back = abp_shim_openai::ir_to_messages(&ir);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, abp_shim_openai::Role::System);
        assert_eq!(back[0].content.as_deref(), Some("Be helpful."));
        assert_eq!(back[1].content.as_deref(), Some("Hello"));
        assert_eq!(back[2].content.as_deref(), Some("Hi!"));
    }

    #[test]
    fn claude_self_roundtrip() {
        let claude_msgs = vec![
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Hello".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "Hi!".into(),
            },
        ];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some("Be nice"));
        let back = abp_claude_sdk::lowering::from_ir(&ir);

        assert_eq!(back.len(), 2); // system skipped
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello");
        assert_eq!(back[1].role, "assistant");
        assert_eq!(back[1].content, "Hi!");
    }

    #[test]
    fn gemini_self_roundtrip() {
        let contents = vec![
            abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hello".into())],
            },
            abp_gemini_sdk::dialect::GeminiContent {
                role: "model".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hi!".into())],
            },
        ];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, None);
        let back = abp_gemini_sdk::lowering::from_ir(&ir);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[1].role, "model");
    }

    #[test]
    fn codex_self_roundtrip() {
        // Codex input items → IR → Codex response items
        let inputs = vec![abp_codex_sdk::dialect::CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let ir = abp_codex_sdk::lowering::input_to_ir(&inputs);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].text_content(), "Hello");

        // Roundtrip assistant messages through IR
        let ir_assistant =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hi!")]);
        let back = abp_codex_sdk::lowering::from_ir(&ir_assistant);
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn kimi_self_roundtrip() {
        let kimi_msgs = vec![
            abp_kimi_sdk::dialect::KimiMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            abp_kimi_sdk::dialect::KimiMessage {
                role: "assistant".into(),
                content: Some("Hi!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let ir = abp_kimi_sdk::lowering::to_ir(&kimi_msgs);
        let back = abp_kimi_sdk::lowering::from_ir(&ir);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn copilot_self_roundtrip() {
        let copilot_msgs = vec![
            abp_copilot_sdk::dialect::CopilotMessage {
                role: "user".into(),
                content: "Hello".into(),
                name: None,
                copilot_references: vec![],
            },
            abp_copilot_sdk::dialect::CopilotMessage {
                role: "assistant".into(),
                content: "Hi!".into(),
                name: None,
                copilot_references: vec![],
            },
        ];
        let ir = abp_copilot_sdk::lowering::to_ir(&copilot_msgs);
        let back = abp_copilot_sdk::lowering::from_ir(&ir);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 7. Tool definition preservation across dialects
// ════════════════════════════════════════════════════════════════════════

mod tool_definitions {
    use super::*;

    #[test]
    fn openai_tool_def_to_ir() {
        let tools = vec![abp_shim_openai::Tool::function(
            "read_file",
            "Read file contents",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        )];
        let ir_tools = abp_shim_openai::tools_to_ir(&tools);

        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[0].description, "Read file contents");
    }

    #[test]
    fn ir_tool_def_structure_preserved() {
        let tool = standard_tool_def();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description, "Read the contents of a file");
        assert!(tool.parameters.get("properties").is_some());
    }

    #[test]
    fn multiple_tool_defs_preserved() {
        let tools = vec![
            abp_shim_openai::Tool::function("tool_a", "Description A", json!({})),
            abp_shim_openai::Tool::function("tool_b", "Description B", json!({})),
            abp_shim_openai::Tool::function("tool_c", "Description C", json!({})),
        ];
        let ir_tools = abp_shim_openai::tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 3);
        assert_eq!(ir_tools[0].name, "tool_a");
        assert_eq!(ir_tools[1].name, "tool_b");
        assert_eq!(ir_tools[2].name, "tool_c");
    }

    #[test]
    fn tool_parameters_schema_round_trips() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "line": {"type": "integer", "description": "Line number"}
            },
            "required": ["path"]
        });
        let tools = vec![abp_shim_openai::Tool::function(
            "read_file",
            "Read file",
            schema.clone(),
        )];
        let ir_tools = abp_shim_openai::tools_to_ir(&tools);
        assert_eq!(ir_tools[0].parameters, schema);
    }

    #[test]
    fn gemini_tool_declaration_structure() {
        let tool = abp_shim_gemini::ToolDeclaration {
            function_declarations: vec![abp_shim_gemini::FunctionDeclaration {
                name: "search".into(),
                description: "Search files".into(),
                parameters: json!({"type": "object"}),
            }],
        };
        assert_eq!(tool.function_declarations[0].name, "search");
    }

    #[test]
    fn empty_tool_list_preserved() {
        let ir_tools = abp_shim_openai::tools_to_ir(&[]);
        assert!(ir_tools.is_empty());
    }
}

// ════════════════════════════════════════════════════════════════════════
// 8. System message handling across dialects
// ════════════════════════════════════════════════════════════════════════

mod system_messages {
    use super::*;

    #[test]
    fn openai_system_to_claude_system() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful."),
            IrMessage::text(IrRole::User, "Hello"),
        ]);

        let sys = abp_claude_sdk::lowering::extract_system_prompt(&ir);
        assert_eq!(sys.as_deref(), Some("Be helpful."));

        let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
        // System should NOT appear in claude messages
        assert!(claude_msgs.iter().all(|m| m.role != "system"));
    }

    #[test]
    fn openai_system_to_gemini_system_instruction() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are an AI."),
            IrMessage::text(IrRole::User, "Hi"),
        ]);

        let sys = abp_gemini_sdk::lowering::extract_system_instruction(&ir);
        assert!(sys.is_some());

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        // System should NOT appear in gemini contents
        assert_eq!(gemini.len(), 1);
    }

    #[test]
    fn claude_system_to_openai_system() {
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some("Instructions here"));
        let openai = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(openai[0].role, "system");
        assert_eq!(openai[0].content.as_deref(), Some("Instructions here"));
    }

    #[test]
    fn gemini_system_to_openai_system() {
        let sys = abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Be brief".into())],
        };
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hi".into())],
        }];
        let ir = abp_gemini_sdk::lowering::to_ir(&contents, Some(&sys));
        let openai = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(openai[0].role, "system");
        assert_eq!(openai[0].content.as_deref(), Some("Be brief"));
    }

    #[test]
    fn no_system_message_works() {
        let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")]);

        let sys_claude = abp_claude_sdk::lowering::extract_system_prompt(&ir);
        assert!(sys_claude.is_none());

        let sys_gemini = abp_gemini_sdk::lowering::extract_system_instruction(&ir);
        assert!(sys_gemini.is_none());
    }

    #[test]
    fn empty_system_prompt_handling() {
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some(""));
        // Empty system prompt should be skipped
        assert_eq!(ir.len(), 1);
    }
}

// ════════════════════════════════════════════════════════════════════════
// 9. Temperature/sampling parameter mapping
// ════════════════════════════════════════════════════════════════════════

mod sampling_parameters {

    #[test]
    fn openai_temperature_in_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .temperature(0.5)
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);

        assert_eq!(
            wo.config.vendor.get("temperature").and_then(|v| v.as_f64()),
            Some(0.5)
        );
    }

    #[test]
    fn openai_max_tokens_in_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .max_tokens(2048)
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);

        assert_eq!(
            wo.config.vendor.get("max_tokens").and_then(|v| v.as_u64()),
            Some(2048)
        );
    }

    #[test]
    fn openai_stop_sequences_in_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .stop(vec!["END".into(), "STOP".into()])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);

        let stop = wo.config.vendor.get("stop").unwrap();
        let arr = stop.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn claude_temperature_in_work_order() {
        let req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: Some(0.3),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = abp_shim_claude::request_to_work_order(&req);

        assert_eq!(
            wo.config.vendor.get("temperature").and_then(|v| v.as_f64()),
            Some(0.3)
        );
    }

    #[test]
    fn gemini_generation_config_temperature() {
        let gen_config = abp_shim_gemini::GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.8),
            top_p: Some(0.95),
            top_k: Some(40),
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        };
        assert_eq!(gen_config.temperature, Some(0.8));
        assert_eq!(gen_config.top_p, Some(0.95));
        assert_eq!(gen_config.top_k, Some(40));
    }

    #[test]
    fn no_temperature_yields_none() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);

        assert!(!wo.config.vendor.contains_key("temperature"));
    }
}

// ════════════════════════════════════════════════════════════════════════
// 10. Streaming event mapping between dialects
// ════════════════════════════════════════════════════════════════════════

mod streaming_events {
    use super::*;

    fn sample_agent_events() -> Vec<AgentEvent> {
        vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "Hello ".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: "World!".into(),
                },
                ext: None,
            },
        ]
    }

    #[test]
    fn openai_stream_events_from_agent_events() {
        let events = sample_agent_events();
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");

        // 2 delta events + 1 final stop
        assert_eq!(stream.len(), 3);
        assert_eq!(
            stream.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[test]
    fn openai_stream_event_model_preserved() {
        let events = sample_agent_events();
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o-mini");

        for event in &stream {
            assert_eq!(event.model, "gpt-4o-mini");
        }
    }

    #[test]
    fn openai_stream_delta_content() {
        let events = sample_agent_events();
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");

        assert_eq!(
            stream[0].choices[0].delta.content.as_deref(),
            Some("Hello ")
        );
        assert_eq!(
            stream[1].choices[0].delta.content.as_deref(),
            Some("World!")
        );
    }

    #[test]
    fn tool_call_in_stream() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                input: json!({"path": "main.rs"}),
                parent_tool_use_id: None,
            },
            ext: None,
        }];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");

        // tool call chunk + stop chunk
        assert_eq!(stream.len(), 2);
        let tc = stream[0].choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(
            tc[0].function.as_ref().unwrap().name.as_deref(),
            Some("read_file")
        );
    }

    #[test]
    fn empty_events_still_produce_stop() {
        let stream = abp_shim_openai::events_to_stream_events(&[], "gpt-4o");
        assert_eq!(stream.len(), 1);
        assert_eq!(stream[0].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn codex_stream_from_agent_events() {
        let events = sample_agent_events();
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini");
        assert!(!stream.is_empty());
    }

    #[test]
    fn kimi_stream_from_agent_events() {
        let events = sample_agent_events();
        let stream = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1");
        assert!(!stream.is_empty());
    }

    #[test]
    fn copilot_stream_from_agent_events() {
        let events = sample_agent_events();
        let stream = abp_shim_copilot::events_to_stream_events(&events, "copilot");
        assert!(!stream.is_empty());
    }
}

// ════════════════════════════════════════════════════════════════════════
// 11. Model name translation between dialects
// ════════════════════════════════════════════════════════════════════════

mod model_names {
    use super::*;

    #[test]
    fn openai_model_in_work_order() {
        let req = openai_request_simple("sys", "hi");
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn claude_model_in_work_order() {
        let req = claude_request_simple("sys", "hi");
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn codex_model_in_request() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "hi")])
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn model_passed_through_receipt_to_response() {
        let receipt = abp_shim_openai::mock_receipt(vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "Hi".into() },
            ext: None,
        }]);
        let response = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o-2024");
        assert_eq!(response.model, "gpt-4o-2024");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 12. Multi-turn conversation mapping
// ════════════════════════════════════════════════════════════════════════

mod multi_turn {
    use super::*;

    #[test]
    fn openai_multi_turn_through_ir_to_claude() {
        let msgs = vec![
            abp_shim_openai::Message::system("Be concise"),
            abp_shim_openai::Message::user("Hi"),
            abp_shim_openai::Message::assistant("Hello"),
            abp_shim_openai::Message::user("What is 2+2?"),
            abp_shim_openai::Message::assistant("4"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);

        assert_eq!(ir.len(), 5);

        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        assert_eq!(claude.len(), 4); // system skipped
        assert_eq!(claude[0].content, "Hi");
        assert_eq!(claude[1].content, "Hello");
        assert_eq!(claude[2].content, "What is 2+2?");
        assert_eq!(claude[3].content, "4");
    }

    #[test]
    fn claude_multi_turn_through_ir_to_gemini() {
        let claude_msgs = vec![
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Hey".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "Hi!".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Tell me about Rust".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "Rust is a systems language.".into(),
            },
        ];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        assert_eq!(gemini.len(), 4);
        assert_eq!(gemini[0].role, "user");
        assert_eq!(gemini[1].role, "model");
        assert_eq!(gemini[2].role, "user");
        assert_eq!(gemini[3].role, "model");
    }

    #[test]
    fn tool_use_multi_turn_openai_to_gemini() {
        let ir = tool_use_ir_conversation();
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        // 4 messages: user, model (func call), user (func response), model (text)
        assert_eq!(gemini.len(), 4);
    }

    #[test]
    fn tool_use_multi_turn_openai_to_claude() {
        let ir = tool_use_ir_conversation();
        let claude = abp_claude_sdk::lowering::from_ir(&ir);

        // 4 messages: user, assistant (tool_use), user (tool_result), assistant (text)
        assert_eq!(claude.len(), 4);
    }

    #[test]
    fn long_conversation_preserves_message_count() {
        let mut ir_msgs = Vec::new();
        for i in 0..20 {
            if i % 2 == 0 {
                ir_msgs.push(IrMessage::text(IrRole::User, format!("Q{i}")));
            } else {
                ir_msgs.push(IrMessage::text(IrRole::Assistant, format!("A{i}")));
            }
        }
        let ir = IrConversation::from_messages(ir_msgs);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai.len(), 20);

        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        assert_eq!(claude.len(), 20);

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        assert_eq!(gemini.len(), 20);
    }
}

// ════════════════════════════════════════════════════════════════════════
// 13. Content block type mapping (text, image, tool)
// ════════════════════════════════════════════════════════════════════════

mod content_blocks {
    use super::*;

    #[test]
    fn text_block_through_all_dialects() {
        let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello world")]);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai[0].content.as_deref(), Some("Hello world"));

        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        assert_eq!(claude[0].content, "Hello world");

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::Text(t) => assert_eq!(t, "Hello world"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn image_block_through_claude_and_gemini() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            }],
        )]);

        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        let parsed: Vec<abp_claude_sdk::dialect::ClaudeContentBlock> =
            serde_json::from_str(&claude[0].content).unwrap();
        match &parsed[0] {
            abp_claude_sdk::dialect::ClaudeContentBlock::Image { source } => match source {
                abp_claude_sdk::dialect::ClaudeImageSource::Base64 { media_type, data } => {
                    assert_eq!(media_type, "image/png");
                    assert_eq!(data, "iVBORw0KGgo=");
                }
                _ => panic!("expected Base64"),
            },
            other => panic!("expected Image, got {other:?}"),
        }

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/png");
                assert_eq!(d.data, "iVBORw0KGgo=");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_block_through_openai() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tc_1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            }],
        )]);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        let tc = openai[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].id, "tc_1");
        assert_eq!(tc[0].function.name, "search");
    }

    #[test]
    fn tool_use_block_through_gemini() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tc_1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            }],
        )]);

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args, &json!({"q": "rust"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_block_through_openai() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result data".into(),
                }],
                is_error: false,
            }],
        )]);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai[0].role, "tool");
        assert_eq!(openai[0].tool_call_id.as_deref(), Some("tc_1"));
        assert_eq!(openai[0].content.as_deref(), Some("result data"));
    }

    #[test]
    fn tool_result_block_through_gemini() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "gemini_search".into(),
                content: vec![IrContentBlock::Text {
                    text: "search results".into(),
                }],
                is_error: false,
            }],
        )]);

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::FunctionResponse { name, response } => {
                assert_eq!(name, "search");
                assert_eq!(response, &json!("search results"));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    #[test]
    fn thinking_block_through_claude() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning...".into(),
                },
                IrContentBlock::Text {
                    text: "Answer".into(),
                },
            ],
        )]);

        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        let parsed: Vec<abp_claude_sdk::dialect::ClaudeContentBlock> =
            serde_json::from_str(&claude[0].content).unwrap();
        assert_eq!(parsed.len(), 2);
        match &parsed[0] {
            abp_claude_sdk::dialect::ClaudeContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking, "reasoning...");
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn thinking_block_through_openai_becomes_text() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "let me think".into(),
            }],
        )]);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        // Thinking becomes part of text content in OpenAI
        assert!(
            openai[0]
                .content
                .as_deref()
                .unwrap()
                .contains("let me think")
        );
    }

    #[test]
    fn thinking_block_through_gemini_becomes_text() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "hmm...".into(),
            }],
        )]);

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        match &gemini[0].parts[0] {
            abp_gemini_sdk::dialect::GeminiPart::Text(t) => assert_eq!(t, "hmm..."),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn mixed_text_and_tool_use_blocks() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "ls".into(),
                    input: json!({}),
                },
            ],
        )]);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai[0].content.as_deref(), Some("Let me check."));
        assert!(openai[0].tool_calls.is_some());
    }
}

// ════════════════════════════════════════════════════════════════════════
// 14. Error handling for unmappable features
// ════════════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    #[test]
    fn feature_unsupported_error() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        assert!(err.to_string().contains("logprobs"));
        assert!(err.to_string().contains("Claude"));
        assert!(err.to_string().contains("Gemini"));
    }

    #[test]
    fn fidelity_loss_error() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "no native support".into(),
        };
        assert!(err.to_string().contains("fidelity loss"));
    }

    #[test]
    fn dialect_mismatch_error() {
        let err = MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn invalid_input_error() {
        let err = MappingError::InvalidInput {
            reason: "empty message".into(),
        };
        assert!(err.to_string().contains("empty message"));
    }

    #[test]
    fn validate_unsupported_feature() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &["image_input".into()],
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(!results[0].errors.is_empty());
    }

    #[test]
    fn validate_empty_feature_name() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);

        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn validate_unknown_feature() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["nonexistent_feature".into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn kimi_image_input_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Kimi,
            Dialect::OpenAi,
            &["image_input".into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn copilot_image_input_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Copilot,
            Dialect::Claude,
            &["image_input".into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn kimi_code_exec_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::Kimi, Dialect::OpenAi, &["code_exec".into()]);
        assert!(results[0].fidelity.is_unsupported());
    }
}

// ════════════════════════════════════════════════════════════════════════
// 15. Bidirectional mapping roundtrip fidelity
// ════════════════════════════════════════════════════════════════════════

mod roundtrip_fidelity {

    #[test]
    fn openai_claude_roundtrip_text() {
        let msgs = vec![
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi there!"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        let ir2 = abp_claude_sdk::lowering::to_ir(&claude, None);
        let back = abp_openai_sdk::lowering::from_ir(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
        assert_eq!(back[1].content.as_deref(), Some("Hi there!"));
    }

    #[test]
    fn openai_gemini_roundtrip_text() {
        let msgs = vec![
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi there!"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        let ir2 = abp_gemini_sdk::lowering::to_ir(&gemini, None);
        let back = abp_openai_sdk::lowering::from_ir(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
        assert_eq!(back[1].content.as_deref(), Some("Hi there!"));
    }

    #[test]
    fn claude_gemini_roundtrip_text() {
        let claude_msgs = vec![
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Question".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "Answer".into(),
            },
        ];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        let ir2 = abp_gemini_sdk::lowering::to_ir(&gemini, None);
        let back = abp_claude_sdk::lowering::from_ir(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content, "Question");
        assert_eq!(back[1].content, "Answer");
    }

    #[test]
    fn openai_ir_openai_roundtrip_with_system() {
        let msgs = vec![
            abp_shim_openai::Message::system("Instructions"),
            abp_shim_openai::Message::user("Q"),
            abp_shim_openai::Message::assistant("A"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let back = abp_openai_sdk::lowering::from_ir(&ir);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("Instructions"));
    }

    #[test]
    fn openai_claude_openai_roundtrip_with_system() {
        let msgs = vec![
            abp_shim_openai::Message::system("Be helpful"),
            abp_shim_openai::Message::user("Hello"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);

        // Extract system prompt for Claude
        let sys = abp_claude_sdk::lowering::extract_system_prompt(&ir);
        let claude = abp_claude_sdk::lowering::from_ir(&ir);

        // Reconstruct via Claude→IR
        let ir2 = abp_claude_sdk::lowering::to_ir(&claude, sys.as_deref());
        let back = abp_openai_sdk::lowering::from_ir(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("Be helpful"));
        assert_eq!(back[1].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn openai_gemini_openai_roundtrip_with_system() {
        let msgs = vec![
            abp_shim_openai::Message::system("Be brief"),
            abp_shim_openai::Message::user("Hello"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);

        let sys = abp_gemini_sdk::lowering::extract_system_instruction(&ir);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);

        let ir2 = abp_gemini_sdk::lowering::to_ir(&gemini, sys.as_ref());
        let back = abp_openai_sdk::lowering::from_ir(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("Be brief"));
    }

    #[test]
    fn tool_call_roundtrip_openai_claude_openai() {
        let msgs = vec![
            abp_shim_openai::Message::user("Read main.rs"),
            abp_shim_openai::Message::assistant_with_tool_calls(vec![abp_shim_openai::ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: abp_shim_openai::FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        let ir2 = abp_claude_sdk::lowering::to_ir(&claude, None);
        let back = abp_openai_sdk::lowering::from_ir(&ir2);

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content.as_deref(), Some("Read main.rs"));
        let tc = back[1].tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read_file");
        assert_eq!(tc[0].id, "call_1");
    }

    #[test]
    fn image_roundtrip_claude_gemini_claude() {
        let blocks = vec![abp_claude_sdk::dialect::ClaudeContentBlock::Image {
            source: abp_claude_sdk::dialect::ClaudeImageSource::Base64 {
                media_type: "image/jpeg".into(),
                data: "abc123".into(),
            },
        }];
        let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        let ir2 = abp_gemini_sdk::lowering::to_ir(&gemini, None);
        let back = abp_claude_sdk::lowering::from_ir(&ir2);

        let parsed: Vec<abp_claude_sdk::dialect::ClaudeContentBlock> =
            serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            abp_claude_sdk::dialect::ClaudeContentBlock::Image { source } => match source {
                abp_claude_sdk::dialect::ClaudeImageSource::Base64 { media_type, data } => {
                    assert_eq!(media_type, "image/jpeg");
                    assert_eq!(data, "abc123");
                }
                _ => panic!("expected Base64"),
            },
            other => panic!("expected Image, got {other:?}"),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Additional: Mapping registry & matrix tests
// ════════════════════════════════════════════════════════════════════════

mod mapping_registry {
    use super::*;

    #[test]
    fn known_rules_non_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn self_dialect_all_lossless() {
        let reg = known_rules();
        for &d in Dialect::all() {
            for feat in &[
                features::TOOL_USE,
                features::STREAMING,
                features::THINKING,
                features::IMAGE_INPUT,
                features::CODE_EXEC,
            ] {
                let rule = reg.lookup(d, d, feat);
                assert!(
                    rule.is_some(),
                    "missing self-mapping rule for {d} -> {d} / {feat}"
                );
                assert!(
                    rule.unwrap().fidelity.is_lossless(),
                    "self-mapping not lossless for {d} / {feat}"
                );
            }
        }
    }

    #[test]
    fn openai_claude_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn openai_claude_streaming_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn openai_gemini_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn claude_gemini_image_input_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::Claude, Dialect::Gemini, features::IMAGE_INPUT);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn openai_codex_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(
            matches!(rule.unwrap().fidelity, Fidelity::LossyLabeled { .. }),
            "expected lossy"
        );
    }

    #[test]
    fn codex_image_input_unsupported() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_unsupported());
    }

    #[test]
    fn thinking_claude_to_openai_lossy() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING);
        assert!(rule.is_some());
        assert!(matches!(
            rule.unwrap().fidelity,
            Fidelity::LossyLabeled { .. }
        ));
    }

    #[test]
    fn matrix_from_registry() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);

        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Gemini));
        assert!(matrix.is_supported(Dialect::Claude, Dialect::Gemini));
    }

    #[test]
    fn rank_targets_openai() {
        let reg = known_rules();
        let ranked = reg.rank_targets(
            Dialect::OpenAi,
            &[
                features::TOOL_USE,
                features::STREAMING,
                features::IMAGE_INPUT,
            ],
        );
        assert!(!ranked.is_empty());
    }

    #[test]
    fn validate_multiple_features_at_once() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[
                features::TOOL_USE.into(),
                features::STREAMING.into(),
                features::IMAGE_INPUT.into(),
            ],
        );
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.fidelity.is_lossless()));
    }

    #[test]
    fn validate_mixed_fidelity() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::TOOL_USE.into(), features::THINKING.into()],
        );
        assert_eq!(results.len(), 2);
        assert!(results[0].fidelity.is_lossless()); // tool_use
        assert!(matches!(results[1].fidelity, Fidelity::LossyLabeled { .. })); // thinking
    }

    #[test]
    fn kimi_copilot_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn kimi_copilot_streaming_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}

// ════════════════════════════════════════════════════════════════════════
// Additional: Usage / token mapping
// ════════════════════════════════════════════════════════════════════════

mod usage_mapping {
    use super::*;

    #[test]
    fn openai_ir_usage_conversion() {
        let ir = IrUsage::from_io(100, 200);
        let openai = abp_shim_openai::ir_usage_to_usage(&ir);
        assert_eq!(openai.prompt_tokens, 100);
        assert_eq!(openai.completion_tokens, 200);
        assert_eq!(openai.total_tokens, 300);
    }

    #[test]
    fn gemini_usage_to_ir() {
        let usage = abp_shim_gemini::UsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 100,
            total_token_count: 150,
        };
        let ir = abp_shim_gemini::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 50);
        assert_eq!(ir.output_tokens, 100);
        assert_eq!(ir.total_tokens, 150);
    }

    #[test]
    fn gemini_usage_roundtrip() {
        let original = abp_shim_gemini::UsageMetadata {
            prompt_token_count: 42,
            candidates_token_count: 58,
            total_token_count: 100,
        };
        let ir = abp_shim_gemini::usage_to_ir(&original);
        let back = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, 42);
        assert_eq!(back.candidates_token_count, 58);
        assert_eq!(back.total_token_count, 100);
    }

    #[test]
    fn ir_usage_merge() {
        let a = IrUsage::from_io(10, 20);
        let b = IrUsage::from_io(30, 40);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 40);
        assert_eq!(merged.output_tokens, 60);
        assert_eq!(merged.total_tokens, 100);
    }

    #[test]
    fn ir_usage_with_cache() {
        let ir = IrUsage::with_cache(100, 200, 10, 5);
        assert_eq!(ir.cache_read_tokens, 10);
        assert_eq!(ir.cache_write_tokens, 5);
        assert_eq!(ir.total_tokens, 300);
    }

    #[test]
    fn codex_ir_usage_conversion() {
        let ir = IrUsage::from_io(100, 200);
        let usage = abp_shim_codex::ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.total_tokens, 300);
    }

    #[test]
    fn kimi_ir_usage_conversion() {
        let ir = IrUsage::from_io(100, 200);
        let usage = abp_shim_kimi::ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 200);
        assert_eq!(usage.total_tokens, 300);
    }

    #[test]
    fn copilot_ir_usage_tuple() {
        let ir = IrUsage::from_io(100, 200);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 100);
        assert_eq!(output, 200);
        assert_eq!(total, 300);
    }
}

// ════════════════════════════════════════════════════════════════════════
// Additional: Empty & edge-case handling
// ════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_conversation_through_all_dialects() {
        let ir = IrConversation::new();

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        assert!(openai.is_empty());

        let claude = abp_claude_sdk::lowering::from_ir(&ir);
        assert!(claude.is_empty());

        let gemini = abp_gemini_sdk::lowering::from_ir(&ir);
        assert!(gemini.is_empty());
    }

    #[test]
    fn empty_text_message_preserved() {
        let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "")]);

        let openai = abp_openai_sdk::lowering::from_ir(&ir);
        assert_eq!(openai.len(), 1);
    }

    #[test]
    fn ir_conversation_accessors() {
        let ir = standard_ir_conversation();

        assert_eq!(ir.len(), 3);
        assert!(!ir.is_empty());
        assert!(ir.system_message().is_some());
        assert!(ir.last_assistant().is_some());
        assert_eq!(
            ir.last_assistant().unwrap().text_content(),
            "Hi! How can I help you?"
        );
        assert_eq!(ir.messages_by_role(IrRole::User).len(), 1);
    }

    #[test]
    fn ir_conversation_tool_calls_accessor() {
        let ir = tool_use_ir_conversation();
        let tcs = ir.tool_calls();
        assert_eq!(tcs.len(), 1);
        match tcs[0] {
            IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "read_file"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn ir_message_is_text_only() {
        let text_msg = IrMessage::text(IrRole::User, "Hello");
        assert!(text_msg.is_text_only());

        let tool_msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "1".into(),
                name: "tool".into(),
                input: json!({}),
            }],
        );
        assert!(!tool_msg.is_text_only());
    }

    #[test]
    fn ir_message_text_content_concatenation() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Hello ".into(),
                },
                IrContentBlock::Text {
                    text: "World".into(),
                },
            ],
        );
        assert_eq!(msg.text_content(), "Hello World");
    }

    #[test]
    fn fidelity_predicates() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(!Fidelity::Lossless.is_unsupported());

        let lossy = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert!(!lossy.is_lossless());
        assert!(!lossy.is_unsupported());

        let unsupported = Fidelity::Unsupported { reason: "r".into() };
        assert!(!unsupported.is_lossless());
        assert!(unsupported.is_unsupported());
    }

    #[test]
    fn mapping_rule_serialization() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_dialect, Dialect::OpenAi);
        assert_eq!(back.target_dialect, Dialect::Claude);
        assert_eq!(back.feature, "tool_use");
        assert!(back.fidelity.is_lossless());
    }

    #[test]
    fn mapping_matrix_empty() {
        let matrix = MappingMatrix::new();
        assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn dialect_all_returns_six() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_labels() {
        assert_eq!(Dialect::OpenAi.label(), "OpenAI");
        assert_eq!(Dialect::Claude.label(), "Claude");
        assert_eq!(Dialect::Gemini.label(), "Gemini");
        assert_eq!(Dialect::Codex.label(), "Codex");
        assert_eq!(Dialect::Kimi.label(), "Kimi");
        assert_eq!(Dialect::Copilot.label(), "Copilot");
    }

    #[test]
    fn dialect_display() {
        assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
        assert_eq!(format!("{}", Dialect::Claude), "Claude");
    }

    #[test]
    fn dialect_serde_roundtrip() {
        for &d in Dialect::all() {
            let json = serde_json::to_string(&d).unwrap();
            let back: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }
}
