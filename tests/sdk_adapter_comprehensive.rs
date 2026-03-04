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
//! Comprehensive tests for the SDK adapter layer (abp-openai-sdk, abp-claude-sdk, abp-gemini-sdk).
//!
//! Categories:
//! 1. SDK adapter initialization (constants, configs, registration)
//! 2. Request construction for each SDK
//! 3. Response parsing for each SDK
//! 4. Streaming event handling
//! 5. Error types per SDK
//! 6. SDK-specific model definitions
//! 7. Cross-SDK compatibility (same IR from different SDKs)
//! 8. Edge cases

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};
use serde_json::json;
use std::path::Path;

// ═══════════════════════════════════════════════════════════════════════════
// 1. SDK Adapter Initialization
// ═══════════════════════════════════════════════════════════════════════════

mod initialization {
    use super::*;
    use abp_claude_sdk::dialect::ClaudeConfig;
    use abp_gemini_sdk::dialect::GeminiConfig;
    use abp_openai_sdk::dialect::OpenAIConfig;

    // ── OpenAI ──────────────────────────────────────────────────────────

    #[test]
    fn openai_backend_name_constant() {
        assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
    }

    #[test]
    fn openai_host_script_relative_constant() {
        assert_eq!(abp_openai_sdk::HOST_SCRIPT_RELATIVE, "hosts/openai/host.js");
    }

    #[test]
    fn openai_default_node_command() {
        assert_eq!(abp_openai_sdk::DEFAULT_NODE_COMMAND, "node");
    }

    #[test]
    fn openai_sidecar_script_joins_path() {
        let root = Path::new("/project");
        let script = abp_openai_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/openai/host.js"));
    }

    #[test]
    fn openai_default_config_base_url() {
        let cfg = OpenAIConfig::default();
        assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn openai_default_config_model() {
        let cfg = OpenAIConfig::default();
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn openai_default_config_max_tokens() {
        let cfg = OpenAIConfig::default();
        assert_eq!(cfg.max_tokens, Some(4096));
    }

    #[test]
    fn openai_default_config_api_key_empty() {
        let cfg = OpenAIConfig::default();
        assert!(cfg.api_key.is_empty());
    }

    #[test]
    fn openai_default_config_temperature_none() {
        let cfg = OpenAIConfig::default();
        assert!(cfg.temperature.is_none());
    }

    // ── Claude ──────────────────────────────────────────────────────────

    #[test]
    fn claude_backend_name_constant() {
        assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
    }

    #[test]
    fn claude_host_script_relative_constant() {
        assert_eq!(abp_claude_sdk::HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
    }

    #[test]
    fn claude_default_node_command() {
        assert_eq!(abp_claude_sdk::DEFAULT_NODE_COMMAND, "node");
    }

    #[test]
    fn claude_sidecar_script_joins_path() {
        let root = Path::new("/project");
        let script = abp_claude_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/claude/host.js"));
    }

    #[test]
    fn claude_default_config_base_url() {
        let cfg = ClaudeConfig::default();
        assert_eq!(cfg.base_url, "https://api.anthropic.com/v1");
    }

    #[test]
    fn claude_default_config_model() {
        let cfg = ClaudeConfig::default();
        assert_eq!(cfg.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn claude_default_config_max_tokens() {
        let cfg = ClaudeConfig::default();
        assert_eq!(cfg.max_tokens, 4096);
    }

    #[test]
    fn claude_default_config_no_system_prompt() {
        let cfg = ClaudeConfig::default();
        assert!(cfg.system_prompt.is_none());
    }

    #[test]
    fn claude_default_config_no_thinking() {
        let cfg = ClaudeConfig::default();
        assert!(cfg.thinking.is_none());
    }

    // ── Gemini ──────────────────────────────────────────────────────────

    #[test]
    fn gemini_backend_name_constant() {
        assert_eq!(abp_gemini_sdk::BACKEND_NAME, "sidecar:gemini");
    }

    #[test]
    fn gemini_host_script_relative_constant() {
        assert_eq!(abp_gemini_sdk::HOST_SCRIPT_RELATIVE, "hosts/gemini/host.js");
    }

    #[test]
    fn gemini_default_node_command() {
        assert_eq!(abp_gemini_sdk::DEFAULT_NODE_COMMAND, "node");
    }

    #[test]
    fn gemini_sidecar_script_joins_path() {
        let root = Path::new("/project");
        let script = abp_gemini_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/gemini/host.js"));
    }

    #[test]
    fn gemini_default_config_base_url() {
        let cfg = GeminiConfig::default();
        assert!(cfg.base_url.contains("googleapis.com"));
    }

    #[test]
    fn gemini_default_config_model() {
        let cfg = GeminiConfig::default();
        assert_eq!(cfg.model, "gemini-2.5-flash");
    }

    #[test]
    fn gemini_default_config_max_output_tokens() {
        let cfg = GeminiConfig::default();
        assert_eq!(cfg.max_output_tokens, Some(4096));
    }

    #[test]
    fn gemini_default_config_temperature_none() {
        let cfg = GeminiConfig::default();
        assert!(cfg.temperature.is_none());
    }

    // ── Registration (nonexistent path) ─────────────────────────────────

    #[test]
    fn openai_register_default_nonexistent_path() {
        let mut runtime = abp_runtime::Runtime::new();
        let bogus = Path::new("/nonexistent/sdk_test_openai");
        let result = abp_openai_sdk::register_default(&mut runtime, bogus, None).unwrap_or(false);
        assert!(!result);
    }

    #[test]
    fn claude_register_default_nonexistent_path() {
        let mut runtime = abp_runtime::Runtime::new();
        let bogus = Path::new("/nonexistent/sdk_test_claude");
        let result = abp_claude_sdk::register_default(&mut runtime, bogus, None).unwrap_or(false);
        assert!(!result);
    }

    #[test]
    fn gemini_register_default_nonexistent_path() {
        let mut runtime = abp_runtime::Runtime::new();
        let bogus = Path::new("/nonexistent/sdk_test_gemini");
        let result = abp_gemini_sdk::register_default(&mut runtime, bogus, None).unwrap_or(false);
        assert!(!result);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request Construction
// ═══════════════════════════════════════════════════════════════════════════

mod request_construction {
    use super::*;
    use abp_claude_sdk::dialect::{self as claude_dialect, ClaudeConfig};
    use abp_gemini_sdk::dialect::{self as gemini_dialect, GeminiConfig, GeminiPart};
    use abp_openai_sdk::dialect::{self as openai_dialect, OpenAIConfig};

    // ── OpenAI ──────────────────────────────────────────────────────────

    #[test]
    fn openai_request_has_user_message() {
        let wo = WorkOrderBuilder::new("Write tests").build();
        let cfg = OpenAIConfig::default();
        let req = openai_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(
            req.messages[0]
                .content
                .as_deref()
                .unwrap()
                .contains("Write tests")
        );
    }

    #[test]
    fn openai_request_uses_config_model_as_default() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = OpenAIConfig::default();
        let req = openai_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn openai_request_respects_work_order_model() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
        let cfg = OpenAIConfig::default();
        let req = openai_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "gpt-4-turbo");
    }

    #[test]
    fn openai_request_carries_temperature() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = OpenAIConfig {
            temperature: Some(0.7),
            ..Default::default()
        };
        let req = openai_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.temperature, Some(0.7));
    }

    #[test]
    fn openai_request_carries_max_tokens() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = OpenAIConfig {
            max_tokens: Some(1024),
            ..Default::default()
        };
        let req = openai_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.max_tokens, Some(1024));
    }

    // ── Claude ──────────────────────────────────────────────────────────

    #[test]
    fn claude_request_has_user_message() {
        let wo = WorkOrderBuilder::new("Fix bug").build();
        let cfg = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(req.messages[0].content.contains("Fix bug"));
    }

    #[test]
    fn claude_request_uses_config_model_as_default() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn claude_request_respects_work_order_model() {
        let wo = WorkOrderBuilder::new("task")
            .model("claude-opus-4-20250514")
            .build();
        let cfg = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "claude-opus-4-20250514");
    }

    #[test]
    fn claude_request_carries_max_tokens() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig {
            max_tokens: 2048,
            ..Default::default()
        };
        let req = claude_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.max_tokens, 2048);
    }

    #[test]
    fn claude_request_carries_system_prompt() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig {
            system_prompt: Some("Be concise".into()),
            ..Default::default()
        };
        let req = claude_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.system.as_deref(), Some("Be concise"));
    }

    #[test]
    fn claude_request_no_system_prompt_when_none() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig::default();
        let req = claude_dialect::map_work_order(&wo, &cfg);
        assert!(req.system.is_none());
    }

    #[test]
    fn claude_request_carries_thinking_config() {
        use abp_claude_sdk::dialect::ThinkingConfig;
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = ClaudeConfig {
            thinking: Some(ThinkingConfig::new(8192)),
            ..Default::default()
        };
        let req = claude_dialect::map_work_order(&wo, &cfg);
        let thinking = req.thinking.unwrap();
        assert_eq!(thinking.budget_tokens, 8192);
        assert_eq!(thinking.thinking_type, "enabled");
    }

    // ── Gemini ──────────────────────────────────────────────────────────

    #[test]
    fn gemini_request_has_user_content() {
        let wo = WorkOrderBuilder::new("Optimize query").build();
        let cfg = GeminiConfig::default();
        let req = gemini_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, "user");
        match &req.contents[0].parts[0] {
            GeminiPart::Text(t) => assert!(t.contains("Optimize query")),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn gemini_request_uses_config_model_as_default() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = GeminiConfig::default();
        let req = gemini_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "gemini-2.5-flash");
    }

    #[test]
    fn gemini_request_respects_work_order_model() {
        let wo = WorkOrderBuilder::new("task")
            .model("gemini-2.5-pro")
            .build();
        let cfg = GeminiConfig::default();
        let req = gemini_dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "gemini-2.5-pro");
    }

    #[test]
    fn gemini_request_generation_config_present() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = GeminiConfig::default();
        let req = gemini_dialect::map_work_order(&wo, &cfg);
        let gen_cfg = req.generation_config.unwrap();
        assert_eq!(gen_cfg.max_output_tokens, Some(4096));
    }

    #[test]
    fn gemini_request_no_generation_config_when_all_none() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = GeminiConfig {
            max_output_tokens: None,
            temperature: None,
            ..Default::default()
        };
        let req = gemini_dialect::map_work_order(&wo, &cfg);
        assert!(req.generation_config.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response Parsing
// ═══════════════════════════════════════════════════════════════════════════

mod response_parsing {
    use super::*;
    use abp_claude_sdk::dialect::{self as claude_dialect, *};
    use abp_gemini_sdk::dialect::{self as gemini_dialect, *};
    use abp_openai_sdk::dialect::{self as openai_dialect, *};

    // ── OpenAI ──────────────────────────────────────────────────────────

    #[test]
    fn openai_response_text_produces_assistant_message() {
        let resp = OpenAIResponse {
            id: "cmpl-1".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: Some("Hello world".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        };
        let events = openai_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::AssistantMessage { text } if text == "Hello world")
        );
    }

    #[test]
    fn openai_response_tool_call_produces_tool_call_event() {
        let resp = OpenAIResponse {
            id: "cmpl-2".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![OpenAIToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "read_file".into(),
                            arguments: r#"{"path":"lib.rs"}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let events = openai_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input, &json!({"path": "lib.rs"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn openai_response_multiple_choices() {
        let resp = OpenAIResponse {
            id: "cmpl-3".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![
                OpenAIChoice {
                    index: 0,
                    message: OpenAIMessage {
                        role: "assistant".into(),
                        content: Some("Option A".into()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    finish_reason: Some("stop".into()),
                },
                OpenAIChoice {
                    index: 1,
                    message: OpenAIMessage {
                        role: "assistant".into(),
                        content: Some("Option B".into()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    finish_reason: Some("stop".into()),
                },
            ],
            usage: None,
        };
        let events = openai_dialect::map_response(&resp);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn openai_response_empty_content_skipped() {
        let resp = OpenAIResponse {
            id: "cmpl-4".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".into(),
                    content: Some(String::new()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        };
        let events = openai_dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn openai_response_usage_serde() {
        let usage = OpenAIUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["total_tokens"], 150);
    }

    // ── Claude ──────────────────────────────────────────────────────────

    #[test]
    fn claude_response_text_produces_assistant_message() {
        let resp = ClaudeResponse {
            id: "msg_1".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Text {
                text: "Hi there".into(),
            }],
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        let events = claude_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::AssistantMessage { text } if text == "Hi there")
        );
    }

    #[test]
    fn claude_response_tool_use_produces_tool_call_event() {
        let resp = ClaudeResponse {
            id: "msg_2".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: json!({"command": "ls"}),
            }],
            stop_reason: Some("tool_use".into()),
            usage: None,
        };
        let events = claude_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
                assert_eq!(input, &json!({"command": "ls"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn claude_response_tool_result_produces_tool_result_event() {
        let resp = ClaudeResponse {
            id: "msg_3".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: Some("file list".into()),
                is_error: None,
            }],
            stop_reason: None,
            usage: None,
        };
        let events = claude_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn claude_response_thinking_block_has_ext() {
        let resp = ClaudeResponse {
            id: "msg_4".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Thinking {
                thinking: "Let me think...".into(),
                signature: Some("sig_abc".into()),
            }],
            stop_reason: Some("end_turn".into()),
            usage: None,
        };
        let events = claude_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        let ext = events[0].ext.as_ref().unwrap();
        assert_eq!(ext["thinking"], serde_json::Value::Bool(true));
        assert_eq!(ext["signature"], json!("sig_abc"));
    }

    #[test]
    fn claude_response_image_block_no_event() {
        let resp = ClaudeResponse {
            id: "msg_5".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock::Image {
                source: ClaudeImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
            }],
            stop_reason: None,
            usage: None,
        };
        let events = claude_dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn claude_usage_serde_with_cache() {
        let usage = ClaudeUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_creation_input_tokens: Some(50),
            cache_read_input_tokens: Some(30),
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["cache_creation_input_tokens"], 50);
    }

    // ── Gemini ──────────────────────────────────────────────────────────

    #[test]
    fn gemini_response_text_produces_assistant_message() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Result".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let events = gemini_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::AssistantMessage { text } if text == "Result")
        );
    }

    #[test]
    fn gemini_response_function_call_produces_tool_call() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::FunctionCall {
                        name: "search".into(),
                        args: json!({"q": "test"}),
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let events = gemini_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "search");
                assert!(tool_use_id.is_none()); // Gemini has no per-call IDs
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn gemini_response_function_response_produces_tool_result() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::FunctionResponse {
                        name: "api".into(),
                        response: json!("ok"),
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let events = gemini_dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolResult { tool_name, .. } => {
                assert_eq!(tool_name, "api");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn gemini_response_inline_data_no_event() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::InlineData(GeminiInlineData {
                        mime_type: "image/png".into(),
                        data: "base64".into(),
                    })],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let events = gemini_dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn gemini_usage_metadata_serde() {
        let meta = GeminiUsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 100,
            total_token_count: 150,
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["totalTokenCount"], 150);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming Event Handling
// ═══════════════════════════════════════════════════════════════════════════

mod streaming {
    use super::*;
    use abp_claude_sdk::dialect::{self as claude_dialect, *};
    use abp_gemini_sdk::dialect::{self as gemini_dialect, *};
    use abp_openai_sdk::streaming::*;

    // ── OpenAI Streaming ────────────────────────────────────────────────

    #[test]
    fn openai_chunk_text_delta_produces_assistant_delta() {
        let chunk = ChatCompletionChunk {
            id: "chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 0,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let events = abp_openai_sdk::streaming::map_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "Hello")
        );
    }

    #[test]
    fn openai_chunk_empty_content_no_event() {
        let chunk = ChatCompletionChunk {
            id: "chunk-2".into(),
            object: "chat.completion.chunk".into(),
            created: 0,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant".into()),
                    content: Some(String::new()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let events = abp_openai_sdk::streaming::map_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn openai_tool_call_accumulator_basic() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"#.into()),
            }),
        }]);
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(ChunkFunctionCall {
                name: None,
                arguments: Some(r#""rust"}"#.into()),
            }),
        }]);
        let events = acc.finish();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input, &json!({"q": "rust"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn openai_tool_call_accumulator_multiple() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[
            ChunkToolCall {
                index: 0,
                id: Some("c1".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("a".into()),
                    arguments: Some("{}".into()),
                }),
            },
            ChunkToolCall {
                index: 1,
                id: Some("c2".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("b".into()),
                    arguments: Some("{}".into()),
                }),
            },
        ]);
        let events = acc.finish();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn openai_tool_call_accumulator_finish_as_openai() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("read".into()),
                arguments: Some(r#"{"f":"x"}"#.into()),
            }),
        }]);
        let pairs = acc.finish_as_openai();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "c1");
        assert_eq!(pairs[0].1.name, "read");
        assert_eq!(pairs[0].1.arguments, r#"{"f":"x"}"#);
    }

    #[test]
    fn openai_tool_call_accumulator_empty_name_filtered() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: None,
            function: None,
        }]);
        let events = acc.finish();
        assert!(events.is_empty());
    }

    #[test]
    fn openai_chunk_usage_serde() {
        let usage = ChunkUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["total_tokens"], 30);
    }

    // ── Claude Streaming ────────────────────────────────────────────────

    #[test]
    fn claude_stream_message_start_produces_run_started() {
        let event = ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_1".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        };
        let events = claude_dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    }

    #[test]
    fn claude_stream_message_stop_produces_run_completed() {
        let event = ClaudeStreamEvent::MessageStop {};
        let events = claude_dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[test]
    fn claude_stream_text_delta() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "chunk".into(),
            },
        };
        let events = claude_dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "chunk")
        );
    }

    #[test]
    fn claude_stream_thinking_delta_has_ext() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::ThinkingDelta {
                thinking: "hmm".into(),
            },
        };
        let events = claude_dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        let ext = events[0].ext.as_ref().unwrap();
        assert_eq!(ext["thinking"], serde_json::Value::Bool(true));
    }

    #[test]
    fn claude_stream_error_produces_error_event() {
        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "overloaded_error".into(),
                message: "server busy".into(),
            },
        };
        let events = claude_dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("overloaded_error"));
                assert!(message.contains("server busy"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn claude_stream_ping_no_events() {
        let event = ClaudeStreamEvent::Ping {};
        let events = claude_dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn claude_stream_content_block_stop_no_events() {
        let event = ClaudeStreamEvent::ContentBlockStop { index: 0 };
        let events = claude_dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn claude_stream_message_delta_no_events() {
        let event = ClaudeStreamEvent::MessageDelta {
            delta: ClaudeMessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: None,
        };
        let events = claude_dialect::map_stream_event(&event);
        assert!(events.is_empty());
    }

    #[test]
    fn claude_stream_content_block_start_tool_use() {
        let event = ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::ToolUse {
                id: "tu_5".into(),
                name: "glob".into(),
                input: json!({"pattern": "*.rs"}),
            },
        };
        let events = claude_dialect::map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "glob");
                assert_eq!(tool_use_id.as_deref(), Some("tu_5"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    // ── Gemini Streaming ────────────────────────────────────────────────

    #[test]
    fn gemini_stream_chunk_text_produces_delta() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("partial".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = gemini_dialect::map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "partial")
        );
    }

    #[test]
    fn gemini_stream_chunk_function_call() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::FunctionCall {
                        name: "ls".into(),
                        args: json!({}),
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = gemini_dialect::map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "ls")
        );
    }

    #[test]
    fn gemini_map_stream_event_alias() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("via alias".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let events = gemini_dialect::map_stream_event(&chunk);
        assert_eq!(events.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Error Types Per SDK
// ═══════════════════════════════════════════════════════════════════════════

mod error_types {
    use abp_claude_sdk::dialect::*;
    use abp_openai_sdk::validation::*;

    // ── OpenAI Validation Errors ────────────────────────────────────────

    #[test]
    fn unmappable_param_display() {
        let err = UnmappableParam {
            param: "logprobs".into(),
            reason: "not supported".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("not supported"));
    }

    #[test]
    fn validation_errors_display() {
        let errs = ValidationErrors {
            errors: vec![
                UnmappableParam {
                    param: "a".into(),
                    reason: "r1".into(),
                },
                UnmappableParam {
                    param: "b".into(),
                    reason: "r2".into(),
                },
            ],
        };
        let msg = format!("{errs}");
        assert!(msg.contains("2 unmappable"));
    }

    #[test]
    fn validate_clean_fields_ok() {
        let fields = ExtendedRequestFields::default();
        assert!(validate_for_mapped_mode(&fields).is_ok());
    }

    #[test]
    fn validate_logprobs_fails() {
        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "logprobs"));
    }

    #[test]
    fn validate_top_logprobs_fails() {
        let fields = ExtendedRequestFields {
            top_logprobs: Some(5),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "logprobs"));
    }

    #[test]
    fn validate_logit_bias_fails() {
        let mut bias = std::collections::BTreeMap::new();
        bias.insert("123".into(), 1.0);
        let fields = ExtendedRequestFields {
            logit_bias: Some(bias),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "logit_bias"));
    }

    #[test]
    fn validate_seed_fails() {
        let fields = ExtendedRequestFields {
            seed: Some(42),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "seed"));
    }

    #[test]
    fn validate_multiple_errors_accumulated() {
        let mut bias = std::collections::BTreeMap::new();
        bias.insert("1".into(), 0.5);
        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            logit_bias: Some(bias),
            seed: Some(99),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert_eq!(err.errors.len(), 3);
    }

    // ── Claude API Error ────────────────────────────────────────────────

    #[test]
    fn claude_api_error_serde_roundtrip() {
        let error = ClaudeApiError {
            error_type: "invalid_request_error".into(),
            message: "max_tokens too large".into(),
        };
        let json = serde_json::to_string(&error).unwrap();
        let parsed: ClaudeApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.error_type, "invalid_request_error");
        assert_eq!(parsed.message, "max_tokens too large");
    }

    // ── Claude Stop Reasons ─────────────────────────────────────────────

    #[test]
    fn claude_parse_stop_reason_end_turn() {
        assert_eq!(
            parse_stop_reason("end_turn"),
            Some(ClaudeStopReason::EndTurn)
        );
    }

    #[test]
    fn claude_parse_stop_reason_tool_use() {
        assert_eq!(
            parse_stop_reason("tool_use"),
            Some(ClaudeStopReason::ToolUse)
        );
    }

    #[test]
    fn claude_parse_stop_reason_max_tokens() {
        assert_eq!(
            parse_stop_reason("max_tokens"),
            Some(ClaudeStopReason::MaxTokens)
        );
    }

    #[test]
    fn claude_parse_stop_reason_stop_sequence() {
        assert_eq!(
            parse_stop_reason("stop_sequence"),
            Some(ClaudeStopReason::StopSequence)
        );
    }

    #[test]
    fn claude_parse_stop_reason_unknown() {
        assert_eq!(parse_stop_reason("unknown"), None);
    }

    #[test]
    fn claude_map_stop_reason_roundtrip() {
        for reason in [
            ClaudeStopReason::EndTurn,
            ClaudeStopReason::ToolUse,
            ClaudeStopReason::MaxTokens,
            ClaudeStopReason::StopSequence,
        ] {
            let s = map_stop_reason(reason);
            assert_eq!(parse_stop_reason(s), Some(reason));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. SDK-Specific Model Definitions
// ═══════════════════════════════════════════════════════════════════════════

mod model_definitions {

    // ── OpenAI Models ───────────────────────────────────────────────────

    #[test]
    fn openai_dialect_version() {
        assert_eq!(abp_openai_sdk::dialect::DIALECT_VERSION, "openai/v0.1");
    }

    #[test]
    fn openai_default_model() {
        assert_eq!(abp_openai_sdk::dialect::DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn openai_known_models() {
        assert!(abp_openai_sdk::dialect::is_known_model("gpt-4o"));
        assert!(abp_openai_sdk::dialect::is_known_model("gpt-4o-mini"));
        assert!(abp_openai_sdk::dialect::is_known_model("gpt-4-turbo"));
        assert!(abp_openai_sdk::dialect::is_known_model("o1"));
        assert!(abp_openai_sdk::dialect::is_known_model("o1-mini"));
        assert!(abp_openai_sdk::dialect::is_known_model("o3-mini"));
        assert!(!abp_openai_sdk::dialect::is_known_model("gpt-3.5-turbo"));
    }

    #[test]
    fn openai_canonical_model_mapping() {
        assert_eq!(
            abp_openai_sdk::dialect::to_canonical_model("gpt-4o"),
            "openai/gpt-4o"
        );
        assert_eq!(
            abp_openai_sdk::dialect::from_canonical_model("openai/gpt-4o"),
            "gpt-4o"
        );
    }

    #[test]
    fn openai_from_canonical_no_prefix_passthrough() {
        assert_eq!(
            abp_openai_sdk::dialect::from_canonical_model("gpt-4o"),
            "gpt-4o"
        );
    }

    // ── Claude Models ───────────────────────────────────────────────────

    #[test]
    fn claude_dialect_version() {
        assert_eq!(abp_claude_sdk::dialect::DIALECT_VERSION, "claude/v0.1");
    }

    #[test]
    fn claude_default_model() {
        assert_eq!(
            abp_claude_sdk::dialect::DEFAULT_MODEL,
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn claude_known_models() {
        assert!(abp_claude_sdk::dialect::is_known_model(
            "claude-sonnet-4-20250514"
        ));
        assert!(abp_claude_sdk::dialect::is_known_model(
            "claude-opus-4-20250514"
        ));
        assert!(abp_claude_sdk::dialect::is_known_model(
            "claude-haiku-3-5-20241022"
        ));
        assert!(!abp_claude_sdk::dialect::is_known_model("claude-2"));
    }

    #[test]
    fn claude_canonical_model_mapping() {
        assert_eq!(
            abp_claude_sdk::dialect::to_canonical_model("claude-sonnet-4-20250514"),
            "anthropic/claude-sonnet-4-20250514"
        );
        assert_eq!(
            abp_claude_sdk::dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn claude_from_canonical_no_prefix_passthrough() {
        assert_eq!(
            abp_claude_sdk::dialect::from_canonical_model("claude-sonnet-4-20250514"),
            "claude-sonnet-4-20250514"
        );
    }

    // ── Gemini Models ───────────────────────────────────────────────────

    #[test]
    fn gemini_dialect_version() {
        assert_eq!(abp_gemini_sdk::dialect::DIALECT_VERSION, "gemini/v0.1");
    }

    #[test]
    fn gemini_default_model() {
        assert_eq!(abp_gemini_sdk::dialect::DEFAULT_MODEL, "gemini-2.5-flash");
    }

    #[test]
    fn gemini_known_models() {
        assert!(abp_gemini_sdk::dialect::is_known_model("gemini-2.5-flash"));
        assert!(abp_gemini_sdk::dialect::is_known_model("gemini-2.5-pro"));
        assert!(abp_gemini_sdk::dialect::is_known_model("gemini-1.5-pro"));
        assert!(!abp_gemini_sdk::dialect::is_known_model("gemini-ultra"));
    }

    #[test]
    fn gemini_canonical_model_mapping() {
        assert_eq!(
            abp_gemini_sdk::dialect::to_canonical_model("gemini-2.5-flash"),
            "google/gemini-2.5-flash"
        );
        assert_eq!(
            abp_gemini_sdk::dialect::from_canonical_model("google/gemini-2.5-flash"),
            "gemini-2.5-flash"
        );
    }

    #[test]
    fn gemini_from_canonical_no_prefix_passthrough() {
        assert_eq!(
            abp_gemini_sdk::dialect::from_canonical_model("gemini-2.5-flash"),
            "gemini-2.5-flash"
        );
    }

    // ── Capability Manifests ────────────────────────────────────────────

    #[test]
    fn openai_capability_manifest_has_streaming() {
        use abp_core::{Capability, SupportLevel};
        let m = abp_openai_sdk::dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn claude_capability_manifest_has_native_tool_read() {
        use abp_core::{Capability, SupportLevel};
        let m = abp_claude_sdk::dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolRead),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn gemini_capability_manifest_glob_unsupported() {
        use abp_core::{Capability, SupportLevel};
        let m = abp_gemini_sdk::dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolGlob),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn openai_mcp_unsupported() {
        use abp_core::{Capability, SupportLevel};
        let m = abp_openai_sdk::dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::McpClient),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn claude_mcp_client_native() {
        use abp_core::{Capability, SupportLevel};
        let m = abp_claude_sdk::dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::McpClient),
            Some(SupportLevel::Native)
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Cross-SDK Compatibility (Same IR)
// ═══════════════════════════════════════════════════════════════════════════

mod cross_sdk_compatibility {
    use super::*;
    use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeMessage};
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
    use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

    // ── Text message IR convergence ─────────────────────────────────────

    #[test]
    fn user_text_all_sdks_same_ir() {
        let openai_msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let claude_msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let gemini_contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }];

        let ir_openai = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let ir_claude = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let ir_gemini = abp_gemini_sdk::lowering::to_ir(&gemini_contents, None);

        // All produce exactly 1 user message with text "Hello"
        assert_eq!(ir_openai.messages.len(), 1);
        assert_eq!(ir_claude.messages.len(), 1);
        assert_eq!(ir_gemini.messages.len(), 1);

        assert_eq!(ir_openai.messages[0].role, IrRole::User);
        assert_eq!(ir_claude.messages[0].role, IrRole::User);
        assert_eq!(ir_gemini.messages[0].role, IrRole::User);

        assert_eq!(ir_openai.messages[0].text_content(), "Hello");
        assert_eq!(ir_claude.messages[0].text_content(), "Hello");
        assert_eq!(ir_gemini.messages[0].text_content(), "Hello");
    }

    #[test]
    fn assistant_text_all_sdks_same_ir_role() {
        let openai_msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: Some("Sure".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let claude_msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: "Sure".into(),
        }];
        let gemini_contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Sure".into())],
        }];

        let ir_openai = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let ir_claude = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);
        let ir_gemini = abp_gemini_sdk::lowering::to_ir(&gemini_contents, None);

        assert_eq!(ir_openai.messages[0].role, IrRole::Assistant);
        assert_eq!(ir_claude.messages[0].role, IrRole::Assistant);
        assert_eq!(ir_gemini.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn system_prompt_all_sdks_ir_convergence() {
        let openai_msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be helpful".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let claude_msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let gemini_sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be helpful".into())],
        };
        let gemini_contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hi".into())],
        }];

        let ir_openai = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let ir_claude = abp_claude_sdk::lowering::to_ir(&claude_msgs, Some("Be helpful"));
        let ir_gemini = abp_gemini_sdk::lowering::to_ir(&gemini_contents, Some(&gemini_sys));

        // All have system message
        let sys_openai = ir_openai.system_message().unwrap().text_content();
        let sys_claude = ir_claude.system_message().unwrap().text_content();
        let sys_gemini = ir_gemini.system_message().unwrap().text_content();

        assert_eq!(sys_openai, "Be helpful");
        assert_eq!(sys_claude, "Be helpful");
        assert_eq!(sys_gemini, "Be helpful");
    }

    // ── Tool call IR convergence ────────────────────────────────────────

    #[test]
    fn tool_call_openai_and_claude_produce_tool_use_ir() {
        let openai_msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let claude_blocks = vec![ClaudeContentBlock::ToolUse {
            id: "call_1".into(),
            name: "read_file".into(),
            input: json!({"path": "a.rs"}),
        }];
        let claude_msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&claude_blocks).unwrap(),
        }];

        let ir_openai = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let ir_claude = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);

        // Both produce ToolUse with same name and input
        match (
            &ir_openai.messages[0].content[0],
            &ir_claude.messages[0].content[0],
        ) {
            (
                IrContentBlock::ToolUse {
                    name: n1,
                    input: i1,
                    ..
                },
                IrContentBlock::ToolUse {
                    name: n2,
                    input: i2,
                    ..
                },
            ) => {
                assert_eq!(n1, n2);
                assert_eq!(i1, i2);
            }
            (a, b) => panic!("expected ToolUse blocks, got {a:?} and {b:?}"),
        }
    }

    #[test]
    fn tool_result_openai_and_claude_produce_tool_result_ir() {
        let openai_msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("file data".into()),
            tool_calls: None,
            tool_call_id: Some("call_1".into()),
        }];
        let claude_blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: Some("file data".into()),
            is_error: None,
        }];
        let claude_msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&claude_blocks).unwrap(),
        }];

        let ir_openai = abp_openai_sdk::lowering::to_ir(&openai_msgs);
        let ir_claude = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);

        // Both produce ToolResult with same tool_use_id
        match (
            &ir_openai.messages[0].content[0],
            &ir_claude.messages[0].content[0],
        ) {
            (
                IrContentBlock::ToolResult {
                    tool_use_id: id1, ..
                },
                IrContentBlock::ToolResult {
                    tool_use_id: id2, ..
                },
            ) => {
                assert_eq!(id1, id2);
            }
            (a, b) => panic!("expected ToolResult blocks, got {a:?} and {b:?}"),
        }
    }

    // ── Tool definition cross-format ────────────────────────────────────

    #[test]
    fn tool_def_openai_to_canonical_roundtrip() {
        use abp_openai_sdk::dialect::*;
        let canonical = CanonicalToolDef {
            name: "search".into(),
            description: "Search code".into(),
            parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        };
        let openai = tool_def_to_openai(&canonical);
        assert_eq!(openai.tool_type, "function");
        let back = tool_def_from_openai(&openai);
        assert_eq!(back, canonical);
    }

    #[test]
    fn tool_def_claude_to_canonical_roundtrip() {
        use abp_claude_sdk::dialect::*;
        let canonical = CanonicalToolDef {
            name: "search".into(),
            description: "Search code".into(),
            parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        };
        let claude = tool_def_to_claude(&canonical);
        assert_eq!(claude.input_schema, canonical.parameters_schema);
        let back = tool_def_from_claude(&claude);
        assert_eq!(back, canonical);
    }

    #[test]
    fn tool_def_gemini_to_canonical_roundtrip() {
        use abp_gemini_sdk::dialect::*;
        let canonical = CanonicalToolDef {
            name: "search".into(),
            description: "Search code".into(),
            parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        };
        let gemini = tool_def_to_gemini(&canonical);
        assert_eq!(gemini.parameters, canonical.parameters_schema);
        let back = tool_def_from_gemini(&gemini);
        assert_eq!(back, canonical);
    }

    #[test]
    fn canonical_tool_def_all_sdks_interchangeable() {
        let schema = json!({"type": "object", "properties": {"file": {"type": "string"}}});
        let canonical = abp_openai_sdk::dialect::CanonicalToolDef {
            name: "read".into(),
            description: "Read a file".into(),
            parameters_schema: schema.clone(),
        };
        let openai = abp_openai_sdk::dialect::tool_def_to_openai(&canonical);
        let back_from_openai = abp_openai_sdk::dialect::tool_def_from_openai(&openai);

        // Same canonical def can be used with Claude
        let claude_canonical = abp_claude_sdk::dialect::CanonicalToolDef {
            name: back_from_openai.name.clone(),
            description: back_from_openai.description.clone(),
            parameters_schema: back_from_openai.parameters_schema.clone(),
        };
        let claude = abp_claude_sdk::dialect::tool_def_to_claude(&claude_canonical);
        assert_eq!(claude.name, "read");
        assert_eq!(claude.input_schema, schema);

        // And with Gemini
        let gemini_canonical = abp_gemini_sdk::dialect::CanonicalToolDef {
            name: back_from_openai.name,
            description: back_from_openai.description,
            parameters_schema: back_from_openai.parameters_schema,
        };
        let gemini = abp_gemini_sdk::dialect::tool_def_to_gemini(&gemini_canonical);
        assert_eq!(gemini.name, "read");
        assert_eq!(gemini.parameters, schema);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;
    use abp_claude_sdk::dialect::*;
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
    use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

    // ── Empty inputs ────────────────────────────────────────────────────

    #[test]
    fn openai_lowering_empty_messages() {
        let conv = abp_openai_sdk::lowering::to_ir(&[]);
        assert!(conv.is_empty());
        let back = abp_openai_sdk::lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn claude_lowering_empty_messages() {
        let conv = abp_claude_sdk::lowering::to_ir(&[], None);
        assert!(conv.is_empty());
        let back = abp_claude_sdk::lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn gemini_lowering_empty_contents() {
        let conv = abp_gemini_sdk::lowering::to_ir(&[], None);
        assert!(conv.is_empty());
        let back = abp_gemini_sdk::lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    // ── OpenAI malformed tool arguments ─────────────────────────────────

    #[test]
    fn openai_malformed_tool_args_kept_as_string() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "test".into(),
                    arguments: "not-json".into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = abp_openai_sdk::lowering::to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert_eq!(input, &serde_json::Value::String("not-json".into()));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    // ── Claude empty system prompt ──────────────────────────────────────

    #[test]
    fn claude_empty_system_prompt_not_added() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let conv = abp_claude_sdk::lowering::to_ir(&msgs, Some(""));
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── Gemini empty system instruction ─────────────────────────────────

    #[test]
    fn gemini_empty_system_instruction_not_added() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![],
        };
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hi".into())],
        }];
        let conv = abp_gemini_sdk::lowering::to_ir(&contents, Some(&sys));
        assert_eq!(conv.messages.len(), 1);
    }

    // ── OpenAI unknown role ─────────────────────────────────────────────

    #[test]
    fn openai_unknown_role_defaults_to_user() {
        let msgs = vec![OpenAIMessage {
            role: "custom_role".into(),
            content: Some("hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = abp_openai_sdk::lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── Claude unknown role ─────────────────────────────────────────────

    #[test]
    fn claude_unknown_role_defaults_to_user() {
        let msgs = vec![ClaudeMessage {
            role: "custom".into(),
            content: "hello".into(),
        }];
        let conv = abp_claude_sdk::lowering::to_ir(&msgs, None);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── Gemini unknown role ─────────────────────────────────────────────

    #[test]
    fn gemini_unknown_role_defaults_to_user() {
        let contents = vec![GeminiContent {
            role: "unknown".into(),
            parts: vec![GeminiPart::Text("test".into())],
        }];
        let conv = abp_gemini_sdk::lowering::to_ir(&contents, None);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    // ── OpenAI response format serde ────────────────────────────────────

    #[test]
    fn openai_response_format_text_serde() {
        use abp_openai_sdk::response_format::ResponseFormat;
        let rf = ResponseFormat::text();
        let json = serde_json::to_value(&rf).unwrap();
        assert_eq!(json["type"], "text");
    }

    #[test]
    fn openai_response_format_json_object_serde() {
        use abp_openai_sdk::response_format::ResponseFormat;
        let rf = ResponseFormat::json_object();
        let json = serde_json::to_value(&rf).unwrap();
        assert_eq!(json["type"], "json_object");
    }

    #[test]
    fn openai_response_format_json_schema_serde() {
        use abp_openai_sdk::response_format::ResponseFormat;
        let rf = ResponseFormat::json_schema("my_schema", json!({"type": "object"}));
        let json = serde_json::to_value(&rf).unwrap();
        assert_eq!(json["type"], "json_schema");
        assert_eq!(json["json_schema"]["name"], "my_schema");
        assert_eq!(json["json_schema"]["strict"], true);
    }

    // ── Claude passthrough ──────────────────────────────────────────────

    #[test]
    fn claude_passthrough_roundtrip_text_delta() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "hello".into(),
            },
        };
        let wrapped = to_passthrough_event(&event);
        let ext = wrapped.ext.as_ref().unwrap();
        assert_eq!(ext["dialect"], json!("claude"));
        let extracted = from_passthrough_event(&wrapped).unwrap();
        assert_eq!(extracted, event);
    }

    #[test]
    fn claude_passthrough_roundtrip_error() {
        let event = ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "rate_limit_error".into(),
                message: "too many requests".into(),
            },
        };
        let wrapped = to_passthrough_event(&event);
        let extracted = from_passthrough_event(&wrapped).unwrap();
        assert_eq!(extracted, event);
    }

    #[test]
    fn claude_verify_passthrough_fidelity_multiple() {
        let events = vec![
            ClaudeStreamEvent::Ping {},
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta { text: "x".into() },
            },
            ClaudeStreamEvent::MessageStop {},
        ];
        assert!(verify_passthrough_fidelity(&events));
    }

    #[test]
    fn claude_passthrough_non_passthrough_event_returns_none() {
        use abp_core::AgentEvent;
        use chrono::Utc;
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "plain".into(),
            },
            ext: None,
        };
        assert!(from_passthrough_event(&event).is_none());
    }

    // ── Claude tool result helper ───────────────────────────────────────

    #[test]
    fn claude_map_tool_result_success() {
        let msg = map_tool_result("tu_1", "output data", false);
        assert_eq!(msg.role, "user");
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tu_1");
                assert_eq!(content.as_deref(), Some("output data"));
                assert!(is_error.is_none());
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn claude_map_tool_result_error() {
        let msg = map_tool_result("tu_err", "failed", true);
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── Claude ThinkingConfig ───────────────────────────────────────────

    #[test]
    fn claude_thinking_config_new() {
        let tc = ThinkingConfig::new(16384);
        assert_eq!(tc.thinking_type, "enabled");
        assert_eq!(tc.budget_tokens, 16384);
    }

    // ── Claude CacheControl ─────────────────────────────────────────────

    #[test]
    fn claude_cache_control_ephemeral() {
        let cc = ClaudeCacheControl::ephemeral();
        assert_eq!(cc.cache_type, "ephemeral");
    }

    // ── Gemini safety settings serde ────────────────────────────────────

    #[test]
    fn gemini_safety_setting_serde() {
        use abp_gemini_sdk::dialect::{GeminiSafetySetting, HarmBlockThreshold, HarmCategory};
        let setting = GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let json = serde_json::to_value(&setting).unwrap();
        assert_eq!(json["category"], "HARM_CATEGORY_HARASSMENT");
        assert_eq!(json["threshold"], "BLOCK_MEDIUM_AND_ABOVE");
    }

    // ── Gemini tool config serde ────────────────────────────────────────

    #[test]
    fn gemini_tool_config_serde() {
        use abp_gemini_sdk::dialect::{
            FunctionCallingMode, GeminiFunctionCallingConfig, GeminiToolConfig,
        };
        let config = GeminiToolConfig {
            function_calling_config: GeminiFunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["functionCallingConfig"]["mode"], "AUTO");
    }

    #[test]
    fn gemini_function_calling_mode_any() {
        use abp_gemini_sdk::dialect::FunctionCallingMode;
        let json = serde_json::to_value(FunctionCallingMode::Any).unwrap();
        assert_eq!(json, "ANY");
    }

    #[test]
    fn gemini_function_calling_mode_none() {
        use abp_gemini_sdk::dialect::FunctionCallingMode;
        let json = serde_json::to_value(FunctionCallingMode::None).unwrap();
        assert_eq!(json, "NONE");
    }

    // ── Gemini grounding config serde ───────────────────────────────────

    #[test]
    fn gemini_grounding_config_serde() {
        use abp_gemini_sdk::dialect::{
            DynamicRetrievalConfig, GeminiGroundingConfig, GoogleSearchRetrieval,
        };
        let cfg = GeminiGroundingConfig {
            google_search_retrieval: Some(GoogleSearchRetrieval {
                dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".into(),
                    dynamic_threshold: Some(0.5),
                }),
            }),
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json["googleSearchRetrieval"]["dynamicRetrievalConfig"]["dynamicThreshold"]
                .as_f64()
                .is_some()
        );
    }

    // ── Claude system block serde ───────────────────────────────────────

    #[test]
    fn claude_system_block_text_serde() {
        let block = ClaudeSystemBlock::Text {
            text: "Be helpful".into(),
            cache_control: Some(ClaudeCacheControl::ephemeral()),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "Be helpful");
        assert_eq!(json["cache_control"]["type"], "ephemeral");
    }

    // ── Claude stream delta serde ───────────────────────────────────────

    #[test]
    fn claude_stream_delta_input_json_serde() {
        let delta = ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"key":"#.into(),
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json["type"], "input_json_delta");
        assert_eq!(json["partial_json"], r#"{"key":"#);
    }

    #[test]
    fn claude_stream_delta_signature_serde() {
        let delta = ClaudeStreamDelta::SignatureDelta {
            signature: "sig_fragment".into(),
        };
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json["type"], "signature_delta");
    }

    // ── OpenAI ToolChoice serde ─────────────────────────────────────────

    #[test]
    fn openai_tool_choice_mode_auto_serde() {
        use abp_openai_sdk::dialect::{ToolChoice, ToolChoiceMode};
        let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json, "auto");
    }

    #[test]
    fn openai_tool_choice_mode_none_serde() {
        use abp_openai_sdk::dialect::{ToolChoice, ToolChoiceMode};
        let tc = ToolChoice::Mode(ToolChoiceMode::None);
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json, "none");
    }

    #[test]
    fn openai_tool_choice_mode_required_serde() {
        use abp_openai_sdk::dialect::{ToolChoice, ToolChoiceMode};
        let tc = ToolChoice::Mode(ToolChoiceMode::Required);
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json, "required");
    }

    #[test]
    fn openai_tool_choice_function_serde() {
        use abp_openai_sdk::dialect::{ToolChoice, ToolChoiceFunctionRef};
        let tc = ToolChoice::Function {
            tool_type: "function".into(),
            function: ToolChoiceFunctionRef {
                name: "my_func".into(),
            },
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "my_func");
    }

    // ── Gemini extract_system_instruction ────────────────────────────────

    #[test]
    fn gemini_extract_system_instruction_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be terse"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = abp_gemini_sdk::lowering::extract_system_instruction(&conv).unwrap();
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be terse"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    // ── Claude extract_system_prompt ─────────────────────────────────────

    #[test]
    fn claude_extract_system_prompt_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "go"),
        ]);
        assert_eq!(
            abp_claude_sdk::lowering::extract_system_prompt(&conv).as_deref(),
            Some("instructions")
        );
    }

    #[test]
    fn claude_extract_system_prompt_none_when_absent() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        assert!(abp_claude_sdk::lowering::extract_system_prompt(&conv).is_none());
    }

    // ── Gemini HarmProbability serde ─────────────────────────────────────

    #[test]
    fn gemini_harm_probability_serde() {
        use abp_gemini_sdk::dialect::HarmProbability;
        assert_eq!(
            serde_json::to_value(HarmProbability::Negligible).unwrap(),
            "NEGLIGIBLE"
        );
        assert_eq!(serde_json::to_value(HarmProbability::Low).unwrap(), "LOW");
        assert_eq!(
            serde_json::to_value(HarmProbability::Medium).unwrap(),
            "MEDIUM"
        );
        assert_eq!(serde_json::to_value(HarmProbability::High).unwrap(), "HIGH");
    }

    // ── OpenAI request serde roundtrip ──────────────────────────────────

    #[test]
    fn openai_request_serde_roundtrip() {
        use abp_openai_sdk::dialect::{OpenAIMessage, OpenAIRequest};
        let req = OpenAIRequest {
            model: "gpt-4o".into(),
            messages: vec![OpenAIMessage {
                role: "user".into(),
                content: Some("test".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            tool_choice: None,
            temperature: Some(0.5),
            max_tokens: Some(100),
            response_format: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.temperature, Some(0.5));
    }

    // ── Claude request serde roundtrip ──────────────────────────────────

    #[test]
    fn claude_request_serde_roundtrip() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            system: Some("be helpful".into()),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            thinking: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ClaudeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "claude-sonnet-4-20250514");
        assert_eq!(parsed.system.as_deref(), Some("be helpful"));
    }

    // ── Gemini request serde roundtrip ──────────────────────────────────

    #[test]
    fn gemini_request_serde_roundtrip() {
        use abp_gemini_sdk::dialect::GeminiRequest;
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("hello".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: GeminiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gemini-2.5-flash");
    }

    // ── Gemini citation metadata serde ──────────────────────────────────

    #[test]
    fn gemini_citation_metadata_serde() {
        use abp_gemini_sdk::dialect::{GeminiCitationMetadata, GeminiCitationSource};
        let meta = GeminiCitationMetadata {
            citation_sources: vec![GeminiCitationSource {
                start_index: Some(0),
                end_index: Some(10),
                uri: Some("https://example.com".into()),
                license: None,
            }],
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["citationSources"][0]["startIndex"], 0);
        assert_eq!(json["citationSources"][0]["uri"], "https://example.com");
    }
}
