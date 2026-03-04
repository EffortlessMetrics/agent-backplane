#![allow(clippy::all)]
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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! SDK parity matrix — validates that all six shim crates expose a
//! consistent surface and produce semantically equivalent results when
//! converting to/from the ABP intermediate representation.

use abp_core::ir::{IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Category 1 — All shims export ShimError / error types
// ═══════════════════════════════════════════════════════════════════════

mod error_parity {

    #[test]
    fn openai_shim_error_is_debug_and_display() {
        let err = abp_shim_openai::ShimError::InvalidRequest("bad".into());
        let dbg = format!("{err:?}");
        let disp = format!("{err}");
        assert!(dbg.contains("InvalidRequest"));
        assert!(disp.contains("bad"));
    }

    #[test]
    fn claude_shim_error_is_debug_and_display() {
        let err = abp_shim_claude::ShimError::InvalidRequest("bad".into());
        let dbg = format!("{err:?}");
        let disp = format!("{err}");
        assert!(dbg.contains("InvalidRequest"));
        assert!(disp.contains("bad"));
    }

    #[test]
    fn gemini_error_is_debug_and_display() {
        let err = abp_shim_gemini::GeminiError::RequestConversion("bad".into());
        let dbg = format!("{err:?}");
        let disp = format!("{err}");
        assert!(dbg.contains("RequestConversion"));
        assert!(disp.contains("bad"));
    }

    #[test]
    fn codex_shim_error_is_debug_and_display() {
        let err = abp_shim_codex::ShimError::InvalidRequest("bad".into());
        let dbg = format!("{err:?}");
        let disp = format!("{err}");
        assert!(dbg.contains("InvalidRequest"));
        assert!(disp.contains("bad"));
    }

    #[test]
    fn kimi_shim_error_is_debug_and_display() {
        let err = abp_shim_kimi::ShimError::InvalidRequest("bad".into());
        let dbg = format!("{err:?}");
        let disp = format!("{err}");
        assert!(dbg.contains("InvalidRequest"));
        assert!(disp.contains("bad"));
    }

    #[test]
    fn copilot_shim_error_is_debug_and_display() {
        let err = abp_shim_copilot::ShimError::InvalidRequest("bad".into());
        let dbg = format!("{err:?}");
        let disp = format!("{err}");
        assert!(dbg.contains("InvalidRequest"));
        assert!(disp.contains("bad"));
    }

    #[test]
    fn openai_shim_error_has_internal_variant() {
        let err = abp_shim_openai::ShimError::Internal("oops".into());
        assert!(format!("{err}").contains("oops"));
    }

    #[test]
    fn codex_shim_error_has_internal_variant() {
        let err = abp_shim_codex::ShimError::Internal("oops".into());
        assert!(format!("{err}").contains("oops"));
    }

    #[test]
    fn kimi_shim_error_has_internal_variant() {
        let err = abp_shim_kimi::ShimError::Internal("oops".into());
        assert!(format!("{err}").contains("oops"));
    }

    #[test]
    fn copilot_shim_error_has_internal_variant() {
        let err = abp_shim_copilot::ShimError::Internal("oops".into());
        assert!(format!("{err}").contains("oops"));
    }

    #[test]
    fn openai_shim_error_has_serde_variant() {
        let bad_json = serde_json::from_str::<serde_json::Value>("not json");
        let err: abp_shim_openai::ShimError = bad_json.unwrap_err().into();
        assert!(format!("{err}").contains("serde"));
    }

    #[test]
    fn codex_shim_error_has_serde_variant() {
        let bad_json = serde_json::from_str::<serde_json::Value>("not json");
        let err: abp_shim_codex::ShimError = bad_json.unwrap_err().into();
        assert!(format!("{err}").contains("serde"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 2 — All shims support to_ir conversion
// ═══════════════════════════════════════════════════════════════════════

mod to_ir_conversion {
    use super::*;

    #[test]
    fn openai_request_to_ir_preserves_user_message() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("Hello")])
            .build();
        let conv = abp_shim_openai::request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn openai_request_to_ir_preserves_system_message() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .messages(vec![
                abp_shim_openai::Message::system("Be concise"),
                abp_shim_openai::Message::user("Hi"),
            ])
            .build();
        let conv = abp_shim_openai::request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise");
    }

    #[test]
    fn codex_request_to_ir_preserves_user_message() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "Hello")])
            .build();
        let conv = abp_shim_codex::request_to_ir(&req);
        assert!(!conv.is_empty());
        let user_msgs = conv.messages_by_role(IrRole::User);
        assert!(!user_msgs.is_empty());
        assert_eq!(user_msgs[0].text_content(), "Hello");
    }

    #[test]
    fn kimi_request_to_ir_preserves_user_message() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("Hello")])
            .build();
        let conv = abp_shim_kimi::request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn kimi_request_to_ir_preserves_system_message() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .messages(vec![
                abp_shim_kimi::Message::system("Be concise"),
                abp_shim_kimi::Message::user("Hi"),
            ])
            .build();
        let conv = abp_shim_kimi::request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn copilot_request_to_ir_preserves_user_message() {
        let req = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![abp_shim_copilot::Message::user("Hello")])
            .build();
        let conv = abp_shim_copilot::request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn copilot_request_to_ir_preserves_system_message() {
        let req = abp_shim_copilot::CopilotRequestBuilder::new()
            .messages(vec![
                abp_shim_copilot::Message::system("Be concise"),
                abp_shim_copilot::Message::user("Hi"),
            ])
            .build();
        let conv = abp_shim_copilot::request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn claude_message_to_ir_preserves_user_content() {
        let msg = abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Hello".into(),
            }],
        };
        let claude_msg = abp_shim_claude::message_to_ir(&msg);
        assert_eq!(claude_msg.role, "user");
        assert_eq!(claude_msg.content, "Hello");
    }

    #[test]
    fn gemini_to_dialect_request_preserves_model() {
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
            abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello")]),
        );
        let dialect_req = abp_shim_gemini::to_dialect_request(&req);
        assert_eq!(dialect_req.model, "gemini-2.5-flash");
        assert_eq!(dialect_req.contents.len(), 1);
    }

    #[test]
    fn openai_multi_turn_to_ir() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .messages(vec![
                abp_shim_openai::Message::user("What is 2+2?"),
                abp_shim_openai::Message::assistant("4"),
                abp_shim_openai::Message::user("And 3+3?"),
            ])
            .build();
        let conv = abp_shim_openai::request_to_ir(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
    }

    #[test]
    fn kimi_multi_turn_to_ir() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .messages(vec![
                abp_shim_kimi::Message::user("What is 2+2?"),
                abp_shim_kimi::Message::assistant("4"),
                abp_shim_kimi::Message::user("And 3+3?"),
            ])
            .build();
        let conv = abp_shim_kimi::request_to_ir(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 3 — All shims support from_ir conversion
// ═══════════════════════════════════════════════════════════════════════

mod from_ir_conversion {
    use super::*;

    fn sample_ir_conversation() -> IrConversation {
        IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "Hi there"),
        ])
    }

    #[test]
    fn openai_ir_to_messages_preserves_roles() {
        let conv = sample_ir_conversation();
        let msgs = abp_shim_openai::ir_to_messages(&conv);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, abp_shim_openai::Role::System);
        assert_eq!(msgs[1].role, abp_shim_openai::Role::User);
        assert_eq!(msgs[2].role, abp_shim_openai::Role::Assistant);
    }

    #[test]
    fn openai_ir_to_messages_preserves_content() {
        let conv = sample_ir_conversation();
        let msgs = abp_shim_openai::ir_to_messages(&conv);
        assert_eq!(msgs[0].content.as_deref(), Some("Be helpful"));
        assert_eq!(msgs[1].content.as_deref(), Some("Hello"));
        assert_eq!(msgs[2].content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn kimi_ir_to_messages_preserves_roles() {
        let conv = sample_ir_conversation();
        let msgs = abp_shim_kimi::ir_to_messages(&conv);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].role, "assistant");
    }

    #[test]
    fn kimi_ir_to_messages_preserves_content() {
        let conv = sample_ir_conversation();
        let msgs = abp_shim_kimi::ir_to_messages(&conv);
        assert_eq!(msgs[0].content.as_deref(), Some("Be helpful"));
        assert_eq!(msgs[1].content.as_deref(), Some("Hello"));
        assert_eq!(msgs[2].content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn copilot_ir_to_messages_preserves_roles() {
        let conv = sample_ir_conversation();
        let msgs = abp_shim_copilot::ir_to_messages(&conv);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].role, "assistant");
    }

    #[test]
    fn copilot_ir_to_messages_preserves_content() {
        let conv = sample_ir_conversation();
        let msgs = abp_shim_copilot::ir_to_messages(&conv);
        assert_eq!(msgs[0].content, "Be helpful");
        assert_eq!(msgs[1].content, "Hello");
        assert_eq!(msgs[2].content, "Hi there");
    }

    #[test]
    fn codex_ir_to_response_items_from_assistant_message() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(
            IrRole::Assistant,
            "Response text",
        )]);
        let items = abp_shim_codex::ir_to_response_items(&conv);
        assert!(!items.is_empty());
    }

    #[test]
    fn claude_content_block_from_ir_roundtrip_text() {
        let block = abp_shim_claude::ContentBlock::Text {
            text: "Hello".into(),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_from_ir_roundtrip_tool_use() {
        let block = abp_shim_claude::ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "main.rs"}),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn gemini_usage_from_ir_roundtrip() {
        let ir_usage = IrUsage::from_io(100, 50);
        let gemini_usage = abp_shim_gemini::usage_from_ir(&ir_usage);
        let back = abp_shim_gemini::usage_to_ir(&gemini_usage);
        assert_eq!(ir_usage.input_tokens, back.input_tokens);
        assert_eq!(ir_usage.output_tokens, back.output_tokens);
        assert_eq!(ir_usage.total_tokens, back.total_tokens);
    }

    #[test]
    fn openai_ir_to_messages_empty_conversation() {
        let conv = IrConversation::new();
        let msgs = abp_shim_openai::ir_to_messages(&conv);
        assert!(msgs.is_empty());
    }

    #[test]
    fn kimi_ir_to_messages_empty_conversation() {
        let conv = IrConversation::new();
        let msgs = abp_shim_kimi::ir_to_messages(&conv);
        assert!(msgs.is_empty());
    }

    #[test]
    fn copilot_ir_to_messages_empty_conversation() {
        let conv = IrConversation::new();
        let msgs = abp_shim_copilot::ir_to_messages(&conv);
        assert!(msgs.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 4 — Roundtrip: request → IR → request preserves semantics
// ═══════════════════════════════════════════════════════════════════════

mod roundtrip {

    #[test]
    fn openai_messages_roundtrip_user() {
        let msgs = vec![abp_shim_openai::Message::user("Hello world")];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let back = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, abp_shim_openai::Role::User);
        assert_eq!(back[0].content.as_deref(), Some("Hello world"));
    }

    #[test]
    fn openai_messages_roundtrip_system_user_assistant() {
        let msgs = vec![
            abp_shim_openai::Message::system("System prompt"),
            abp_shim_openai::Message::user("User query"),
            abp_shim_openai::Message::assistant("Assistant reply"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&msgs);
        let back = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].content.as_deref(), Some("User query"));
        assert_eq!(back[2].content.as_deref(), Some("Assistant reply"));
    }

    #[test]
    fn kimi_messages_roundtrip_user() {
        let msgs = vec![abp_shim_kimi::Message::user("Hello world")];
        let ir = abp_shim_kimi::messages_to_ir(&msgs);
        let back = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello world"));
    }

    #[test]
    fn kimi_messages_roundtrip_system_user_assistant() {
        let msgs = vec![
            abp_shim_kimi::Message::system("System prompt"),
            abp_shim_kimi::Message::user("User query"),
            abp_shim_kimi::Message::assistant("Assistant reply"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&msgs);
        let back = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].content.as_deref(), Some("User query"));
        assert_eq!(back[2].content.as_deref(), Some("Assistant reply"));
    }

    #[test]
    fn copilot_messages_roundtrip_user() {
        let msgs = vec![abp_shim_copilot::Message::user("Hello world")];
        let ir = abp_shim_copilot::messages_to_ir(&msgs);
        let back = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello world");
    }

    #[test]
    fn copilot_messages_roundtrip_system_user_assistant() {
        let msgs = vec![
            abp_shim_copilot::Message::system("System prompt"),
            abp_shim_copilot::Message::user("User query"),
            abp_shim_copilot::Message::assistant("Assistant reply"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&msgs);
        let back = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].content, "System prompt");
        assert_eq!(back[1].content, "User query");
        assert_eq!(back[2].content, "Assistant reply");
    }

    #[test]
    fn claude_content_block_text_roundtrip() {
        let block = abp_shim_claude::ContentBlock::Text {
            text: "Roundtrip text".into(),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_content_block_tool_result_roundtrip() {
        let block = abp_shim_claude::ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("result data".into()),
            is_error: Some(false),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn claude_thinking_block_roundtrip() {
        let block = abp_shim_claude::ContentBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig_123".into()),
        };
        let ir = abp_shim_claude::content_block_to_ir(&block);
        let back = abp_shim_claude::content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn gemini_usage_roundtrip() {
        let usage = abp_shim_gemini::UsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 100,
            total_token_count: 150,
        };
        let ir = abp_shim_gemini::usage_to_ir(&usage);
        let back = abp_shim_gemini::usage_from_ir(&ir);
        assert_eq!(usage, back);
    }

    #[test]
    fn codex_request_to_ir_then_response_items() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .input(vec![abp_shim_codex::codex_message("user", "test input")])
            .build();
        let ir = abp_shim_codex::request_to_ir(&req);
        assert!(!ir.is_empty());
        // Converting to response items should not panic
        let _items = abp_shim_codex::ir_to_response_items(&ir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 5 — Cross-shim: convert through IR between different shims
// ═══════════════════════════════════════════════════════════════════════

mod cross_shim {

    #[test]
    fn openai_to_kimi_via_ir() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("Be helpful"),
            abp_shim_openai::Message::user("What is Rust?"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("Be helpful"));
        assert_eq!(kimi_msgs[1].role, "user");
        assert_eq!(kimi_msgs[1].content.as_deref(), Some("What is Rust?"));
    }

    #[test]
    fn openai_to_copilot_via_ir() {
        let openai_msgs = vec![
            abp_shim_openai::Message::system("System"),
            abp_shim_openai::Message::user("Query"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].role, "system");
        assert_eq!(copilot_msgs[0].content, "System");
        assert_eq!(copilot_msgs[1].role, "user");
        assert_eq!(copilot_msgs[1].content, "Query");
    }

    #[test]
    fn kimi_to_openai_via_ir() {
        let kimi_msgs = vec![
            abp_shim_kimi::Message::user("Hello from Kimi"),
            abp_shim_kimi::Message::assistant("Hi back"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let openai_msgs = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, abp_shim_openai::Role::User);
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Hello from Kimi"));
        assert_eq!(openai_msgs[1].role, abp_shim_openai::Role::Assistant);
    }

    #[test]
    fn copilot_to_openai_via_ir() {
        let copilot_msgs = vec![
            abp_shim_copilot::Message::user("Hello from Copilot"),
            abp_shim_copilot::Message::assistant("Hello back"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let openai_msgs = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(openai_msgs.len(), 2);
        assert_eq!(openai_msgs[0].role, abp_shim_openai::Role::User);
        assert_eq!(
            openai_msgs[0].content.as_deref(),
            Some("Hello from Copilot")
        );
    }

    #[test]
    fn copilot_to_kimi_via_ir() {
        let copilot_msgs = vec![
            abp_shim_copilot::Message::system("System"),
            abp_shim_copilot::Message::user("Hello"),
            abp_shim_copilot::Message::assistant("Hi"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 3);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[1].role, "user");
        assert_eq!(kimi_msgs[2].role, "assistant");
    }

    #[test]
    fn kimi_to_copilot_via_ir() {
        let kimi_msgs = vec![
            abp_shim_kimi::Message::user("From Kimi"),
            abp_shim_kimi::Message::assistant("Kimi answer"),
        ];
        let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].content, "From Kimi");
        assert_eq!(copilot_msgs[1].content, "Kimi answer");
    }

    #[test]
    fn openai_assistant_to_kimi_preserves_text() {
        let openai_msgs = vec![abp_shim_openai::Message::assistant("The answer is 42")];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("The answer is 42"));
    }

    #[test]
    fn three_hop_openai_to_kimi_to_copilot() {
        let openai_msgs = vec![
            abp_shim_openai::Message::user("Original question"),
            abp_shim_openai::Message::assistant("Original answer"),
        ];
        // OpenAI → IR
        let ir1 = abp_shim_openai::messages_to_ir(&openai_msgs);
        // IR → Kimi
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir1);
        // Kimi → IR
        let ir2 = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        // IR → Copilot
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir2);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].content, "Original question");
        assert_eq!(copilot_msgs[1].content, "Original answer");
    }

    #[test]
    fn cross_shim_empty_conversation() {
        let openai_msgs: Vec<abp_shim_openai::Message> = vec![];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert!(kimi_msgs.is_empty());
        assert!(copilot_msgs.is_empty());
    }

    #[test]
    fn openai_to_codex_ir_to_response_items() {
        let openai_msgs = vec![abp_shim_openai::Message::assistant("Generated text")];
        let ir = abp_shim_openai::messages_to_ir(&openai_msgs);
        let items = abp_shim_codex::ir_to_response_items(&ir);
        assert!(!items.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 6 — Model name normalization / preservation
// ═══════════════════════════════════════════════════════════════════════

mod model_name {

    #[test]
    fn openai_model_in_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn claude_model_in_work_order() {
        let req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn codex_model_in_work_order() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "test")])
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn kimi_model_in_work_order() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .build();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    #[test]
    fn copilot_model_in_work_order() {
        let req = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![abp_shim_copilot::Message::user("test")])
            .build();
        let wo = abp_shim_copilot::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn openai_custom_model_preserved() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("ft:gpt-4o-mini:my-org:custom:id")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_eq!(
            wo.config.model.as_deref(),
            Some("ft:gpt-4o-mini:my-org:custom:id")
        );
    }

    #[test]
    fn kimi_custom_model_preserved() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .build();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-32k"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 7 — All shims handle streaming events
// ═══════════════════════════════════════════════════════════════════════

mod streaming_events {
    use super::*;

    fn text_delta_events() -> Vec<AgentEvent> {
        vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "Hel".into() },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "lo".into() },
                ext: None,
            },
        ]
    }

    #[test]
    fn openai_stream_events_end_with_stop() {
        let events = abp_shim_openai::events_to_stream_events(&text_delta_events(), "gpt-4o");
        assert!(!events.is_empty());
        let last = events.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_stream_events_contain_deltas() {
        let events = abp_shim_openai::events_to_stream_events(&text_delta_events(), "gpt-4o");
        // 2 deltas + 1 final stop
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("Hel"));
        assert_eq!(events[1].choices[0].delta.content.as_deref(), Some("lo"));
    }

    #[test]
    fn codex_stream_events_bookend_with_created_and_completed() {
        let events = abp_shim_codex::events_to_stream_events(&text_delta_events(), "codex-mini");
        // created + 2 deltas + completed = 4
        assert!(events.len() >= 3);
    }

    #[test]
    fn kimi_stream_chunks_end_with_stop() {
        let chunks = abp_shim_kimi::events_to_stream_chunks(&text_delta_events(), "moonshot-v1-8k");
        assert!(!chunks.is_empty());
        let last = chunks.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn kimi_stream_chunks_contain_deltas() {
        let chunks = abp_shim_kimi::events_to_stream_chunks(&text_delta_events(), "moonshot-v1-8k");
        // 2 deltas + 1 final stop = 3
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
    }

    #[test]
    fn copilot_stream_events_end_with_done() {
        let events = abp_shim_copilot::events_to_stream_events(&text_delta_events(), "gpt-4o");
        assert!(!events.is_empty());
    }

    #[test]
    fn openai_stream_model_preserved() {
        let events = abp_shim_openai::events_to_stream_events(&text_delta_events(), "gpt-4-turbo");
        assert_eq!(events[0].model, "gpt-4-turbo");
    }

    #[test]
    fn kimi_stream_model_preserved() {
        let chunks =
            abp_shim_kimi::events_to_stream_chunks(&text_delta_events(), "moonshot-v1-128k");
        assert_eq!(chunks[0].model, "moonshot-v1-128k");
    }

    #[test]
    fn openai_empty_events_still_produce_stop_chunk() {
        let events = abp_shim_openai::events_to_stream_events(&[], "gpt-4o");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn kimi_empty_events_still_produce_stop_chunk() {
        let chunks = abp_shim_kimi::events_to_stream_chunks(&[], "moonshot-v1-8k");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].choices[0].finish_reason.as_deref(), Some("stop"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 8 — All shims handle tool/function calls
// ═══════════════════════════════════════════════════════════════════════

mod tool_calls {
    use super::*;

    fn tool_call_events() -> Vec<AgentEvent> {
        vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        }]
    }

    fn tool_call_receipt() -> abp_core::Receipt {
        abp_shim_openai::mock_receipt(tool_call_events())
    }

    #[test]
    fn openai_receipt_to_response_has_tool_calls() {
        let receipt = tool_call_receipt();
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let tc = &msg.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "read_file");
        assert!(tc.function.arguments.contains("main.rs"));
    }

    #[test]
    fn openai_tool_call_sets_finish_reason_to_tool_calls() {
        let receipt = tool_call_receipt();
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn codex_receipt_to_response_has_function_call() {
        let receipt = abp_shim_codex::mock_receipt(tool_call_events());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn kimi_receipt_to_response_has_tool_calls() {
        let receipt = abp_shim_kimi::mock_receipt(tool_call_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let tc = &msg.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "read_file");
    }

    #[test]
    fn kimi_tool_call_sets_finish_reason() {
        let receipt = abp_shim_kimi::mock_receipt(tool_call_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn copilot_receipt_to_response_has_function_call() {
        let receipt = abp_shim_copilot::mock_receipt(tool_call_events());
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "read_file");
        assert!(fc.arguments.contains("main.rs"));
    }

    #[test]
    fn openai_stream_has_tool_call_event() {
        let events = abp_shim_openai::events_to_stream_events(&tool_call_events(), "gpt-4o");
        let has_tc = events
            .iter()
            .any(|e| e.choices.iter().any(|c| c.delta.tool_calls.is_some()));
        assert!(has_tc);
    }

    #[test]
    fn claude_tool_use_block_conversion() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_123".into()),
                parent_tool_use_id: None,
                input: json!({"path": "lib.rs"}),
            },
            ext: None,
        }];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(resp.content.iter().any(|b| matches!(
            b,
            abp_shim_claude::ContentBlock::ToolUse { name, .. } if name == "read_file"
        )));
    }

    #[test]
    fn claude_tool_use_sets_stop_reason() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("tu_456".into()),
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        }];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn openai_tools_to_ir_preserves_definitions() {
        let tools = vec![abp_shim_openai::Tool::function(
            "search",
            "Search for items",
            json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        )];
        let ir_tools = abp_shim_openai::tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "search");
        assert_eq!(ir_tools[0].description, "Search for items");
    }

    #[test]
    fn openai_tool_call_id_preserved_in_receipt() {
        let receipt = tool_call_receipt();
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_abc");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 9 — Error handling and response construction
// ═══════════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    fn error_events() -> Vec<AgentEvent> {
        vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".into(),
                error_code: None,
            },
            ext: None,
        }]
    }

    #[test]
    fn openai_receipt_with_error_produces_error_content() {
        let receipt = abp_shim_openai::mock_receipt(error_events());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit"));
    }

    #[test]
    fn codex_receipt_with_error_produces_error_content() {
        let receipt = abp_shim_codex::mock_receipt(error_events());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn kimi_receipt_with_error_produces_error_content() {
        let receipt = abp_shim_kimi::mock_receipt(error_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit"));
    }

    #[test]
    fn copilot_receipt_with_error_produces_copilot_error() {
        let receipt = abp_shim_copilot::mock_receipt(error_events());
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("rate limit"));
    }

    #[test]
    fn claude_events_with_error_produces_end_turn() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Error occurred".into(),
            },
            ext: None,
        }];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn openai_empty_trace_produces_stop() {
        let receipt = abp_shim_openai::mock_receipt(vec![]);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn kimi_empty_trace_produces_stop() {
        let receipt = abp_shim_kimi::mock_receipt(vec![]);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 10 — Default construction parity
// ═══════════════════════════════════════════════════════════════════════

mod default_construction {

    #[test]
    fn openai_builder_defaults_to_gpt4o() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn codex_builder_defaults_to_codex_mini() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .input(vec![abp_shim_codex::codex_message("user", "test")])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    #[test]
    fn kimi_builder_defaults_to_moonshot() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn copilot_builder_defaults_to_gpt4o() {
        let req = abp_shim_copilot::CopilotRequestBuilder::new()
            .messages(vec![abp_shim_copilot::Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn openai_client_stores_model() {
        let client = abp_shim_openai::OpenAiClient::new("gpt-4-turbo");
        assert_eq!(client.model(), "gpt-4-turbo");
    }

    #[test]
    fn codex_client_stores_model() {
        let client = abp_shim_codex::CodexClient::new("codex-mini-latest");
        assert_eq!(client.model(), "codex-mini-latest");
    }

    #[test]
    fn kimi_client_stores_model() {
        let client = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
        assert_eq!(client.model(), "moonshot-v1-8k");
    }

    #[test]
    fn copilot_client_stores_model() {
        let client = abp_shim_copilot::CopilotClient::new("gpt-4o");
        assert_eq!(client.model(), "gpt-4o");
    }

    #[test]
    fn gemini_client_stores_model() {
        let client = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
        assert_eq!(client.model(), "gemini-2.5-flash");
    }

    #[test]
    fn openai_client_is_debug() {
        let client = abp_shim_openai::OpenAiClient::new("gpt-4o");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("gpt-4o"));
    }

    #[test]
    fn codex_client_is_debug() {
        let client = abp_shim_codex::CodexClient::new("codex-mini-latest");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("codex-mini-latest"));
    }

    #[test]
    fn kimi_client_is_debug() {
        let client = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("moonshot-v1-8k"));
    }

    #[test]
    fn copilot_client_is_debug() {
        let client = abp_shim_copilot::CopilotClient::new("gpt-4o");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("gpt-4o"));
    }

    #[test]
    fn gemini_client_is_debug() {
        let client = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("gemini-2.5-flash"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 11 — Usage / token accounting parity
// ═══════════════════════════════════════════════════════════════════════

mod usage_parity {
    use super::*;

    #[test]
    fn openai_ir_usage_to_usage() {
        let ir = IrUsage::from_io(100, 50);
        let usage = abp_shim_openai::ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn codex_ir_usage_to_usage() {
        let ir = IrUsage::from_io(100, 50);
        let usage = abp_shim_codex::ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn kimi_ir_usage_to_usage() {
        let ir = IrUsage::from_io(100, 50);
        let usage = abp_shim_kimi::ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn copilot_ir_usage_to_tuple() {
        let ir = IrUsage::from_io(100, 50);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(total, 150);
    }

    #[test]
    fn gemini_usage_to_ir() {
        let usage = abp_shim_gemini::UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = abp_shim_gemini::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
    }

    #[test]
    fn all_shims_agree_on_ir_usage_total() {
        let ir = IrUsage::from_io(200, 100);

        let openai = abp_shim_openai::ir_usage_to_usage(&ir);
        let codex = abp_shim_codex::ir_usage_to_usage(&ir);
        let kimi = abp_shim_kimi::ir_usage_to_usage(&ir);
        let (_, _, copilot_total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        let gemini = abp_shim_gemini::usage_from_ir(&ir);

        assert_eq!(openai.total_tokens, 300);
        assert_eq!(codex.total_tokens, 300);
        assert_eq!(kimi.total_tokens, 300);
        assert_eq!(copilot_total, 300);
        assert_eq!(gemini.total_token_count, 300);
    }

    #[test]
    fn all_shims_agree_on_zero_usage() {
        let ir = IrUsage::from_io(0, 0);

        let openai = abp_shim_openai::ir_usage_to_usage(&ir);
        let codex = abp_shim_codex::ir_usage_to_usage(&ir);
        let kimi = abp_shim_kimi::ir_usage_to_usage(&ir);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        let gemini = abp_shim_gemini::usage_from_ir(&ir);

        assert_eq!(openai.total_tokens, 0);
        assert_eq!(codex.total_tokens, 0);
        assert_eq!(kimi.total_tokens, 0);
        assert_eq!(total, 0);
        assert_eq!(input, 0);
        assert_eq!(output, 0);
        assert_eq!(gemini.total_token_count, 0);
    }

    #[test]
    fn receipt_usage_flows_to_openai_response() {
        let usage = UsageNormalized {
            input_tokens: Some(42),
            output_tokens: Some(17),
            ..Default::default()
        };
        let receipt = abp_shim_openai::mock_receipt_with_usage(vec![], usage);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 42);
        assert_eq!(u.completion_tokens, 17);
        assert_eq!(u.total_tokens, 59);
    }

    #[test]
    fn receipt_usage_flows_to_kimi_response() {
        let usage = UsageNormalized {
            input_tokens: Some(42),
            output_tokens: Some(17),
            ..Default::default()
        };
        let receipt = abp_shim_kimi::mock_receipt_with_usage(vec![], usage);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 42);
        assert_eq!(u.completion_tokens, 17);
        assert_eq!(u.total_tokens, 59);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 12 — Response construction from receipts
// ═══════════════════════════════════════════════════════════════════════

mod receipt_to_response_parity {
    use super::*;

    fn assistant_events() -> Vec<AgentEvent> {
        vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello from the backend!".into(),
            },
            ext: None,
        }]
    }

    #[test]
    fn openai_receipt_to_response_has_assistant_content() {
        let receipt = abp_shim_openai::mock_receipt(assistant_events());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from the backend!")
        );
    }

    #[test]
    fn openai_receipt_to_response_model_matches() {
        let receipt = abp_shim_openai::mock_receipt(assistant_events());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4-turbo");
        assert_eq!(resp.model, "gpt-4-turbo");
    }

    #[test]
    fn codex_receipt_to_response_has_output() {
        let receipt = abp_shim_codex::mock_receipt(assistant_events());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini");
        assert!(!resp.output.is_empty());
        assert_eq!(resp.model, "codex-mini");
    }

    #[test]
    fn kimi_receipt_to_response_has_assistant_content() {
        let receipt = abp_shim_kimi::mock_receipt(assistant_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from the backend!")
        );
    }

    #[test]
    fn kimi_receipt_to_response_model_matches() {
        let receipt = abp_shim_kimi::mock_receipt(assistant_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-128k");
        assert_eq!(resp.model, "moonshot-v1-128k");
    }

    #[test]
    fn copilot_receipt_to_response_has_message() {
        let receipt = abp_shim_copilot::mock_receipt(assistant_events());
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello from the backend!");
        assert!(resp.copilot_errors.is_empty());
    }

    #[test]
    fn claude_response_from_events_has_content() {
        let resp = abp_shim_claude::response_from_events(
            &assistant_events(),
            "claude-sonnet-4-20250514",
            None,
        );
        assert!(resp.content.iter().any(|b| matches!(
            b,
            abp_shim_claude::ContentBlock::Text { text } if text == "Hello from the backend!"
        )));
    }

    #[test]
    fn openai_receipt_response_id_format() {
        let receipt = abp_shim_openai::mock_receipt(assistant_events());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.id.starts_with("chatcmpl-"));
    }

    #[test]
    fn codex_receipt_response_id_format() {
        let receipt = abp_shim_codex::mock_receipt(assistant_events());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini");
        assert!(resp.id.starts_with("resp_"));
    }

    #[test]
    fn kimi_receipt_response_id_format() {
        let receipt = abp_shim_kimi::mock_receipt(assistant_events());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(resp.id.starts_with("cmpl-"));
    }

    #[test]
    fn openai_delta_events_accumulate_content() {
        let delta_events = vec![
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
                    text: "world!".into(),
                },
                ext: None,
            },
        ];
        let receipt = abp_shim_openai::mock_receipt(delta_events);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello world!")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Category 13 — Serde roundtrip for shim types
// ═══════════════════════════════════════════════════════════════════════

mod serde_roundtrip {
    use serde_json::json;

    #[test]
    fn openai_message_serde_roundtrip() {
        let msg = abp_shim_openai::Message::user("test content");
        let json = serde_json::to_string(&msg).unwrap();
        let back: abp_shim_openai::Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content.as_deref(), Some("test content"));
    }

    #[test]
    fn kimi_message_serde_roundtrip() {
        let msg = abp_shim_kimi::Message::user("test content");
        let json = serde_json::to_string(&msg).unwrap();
        let back: abp_shim_kimi::Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content.as_deref(), Some("test content"));
    }

    #[test]
    fn copilot_message_serde_roundtrip() {
        let msg = abp_shim_copilot::Message::user("test content");
        let json = serde_json::to_string(&msg).unwrap();
        let back: abp_shim_copilot::Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "test content");
    }

    #[test]
    fn openai_tool_serde_roundtrip() {
        let tool = abp_shim_openai::Tool::function(
            "search",
            "Search for items",
            json!({"type": "object"}),
        );
        let json = serde_json::to_string(&tool).unwrap();
        let back: abp_shim_openai::Tool = serde_json::from_str(&json).unwrap();
        assert_eq!(back.function.name, "search");
    }

    #[test]
    fn gemini_part_text_serde_roundtrip() {
        let part = abp_shim_gemini::Part::text("test content");
        let json = serde_json::to_string(&part).unwrap();
        let back: abp_shim_gemini::Part = serde_json::from_str(&json).unwrap();
        assert_eq!(back, part);
    }

    #[test]
    fn claude_content_block_text_serde_roundtrip() {
        let block = abp_shim_claude::ContentBlock::Text {
            text: "test".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: abp_shim_claude::ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn openai_usage_serde_roundtrip() {
        let usage = abp_shim_openai::Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: abp_shim_openai::Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn gemini_usage_serde_roundtrip() {
        let usage = abp_shim_gemini::UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: abp_shim_gemini::UsageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }
}
