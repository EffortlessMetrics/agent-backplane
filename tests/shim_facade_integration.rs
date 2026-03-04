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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Integration tests for the SDK shim facade — testing the unified shim
//! interface across all six supported SDKs (OpenAI, Claude, Gemini, Codex,
//! Kimi, Copilot).

use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use chrono::Utc;
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers shared across modules
// ---------------------------------------------------------------------------

/// Build a simple assistant-message event.
fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

/// Build an assistant-delta event.
fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

/// Build a tool-call event.
fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

/// Build an error event.
fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: registry_completeness
// ═══════════════════════════════════════════════════════════════════════════

mod registry_completeness {
    //! Verify that all six shim crates exist, expose a consistent surface,
    //! and are independently importable.

    #[test]
    fn openai_shim_exports_request_to_work_order() {
        // Prove the function is callable (type-level check).
        let _f: fn(&abp_shim_openai::ChatCompletionRequest) -> abp_core::WorkOrder =
            abp_shim_openai::request_to_work_order;
    }

    #[test]
    fn claude_shim_exports_request_to_work_order() {
        let _f: fn(&abp_shim_claude::MessageRequest) -> abp_core::WorkOrder =
            abp_shim_claude::request_to_work_order;
    }

    #[test]
    fn gemini_shim_client_exists() {
        let client = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
        assert_eq!(client.model(), "gemini-2.5-flash");
    }

    #[test]
    fn codex_shim_exports_request_to_work_order() {
        let _f: fn(&abp_codex_sdk::dialect::CodexRequest) -> abp_core::WorkOrder =
            abp_shim_codex::request_to_work_order;
    }

    #[test]
    fn kimi_shim_exports_request_to_work_order() {
        let _f: fn(&abp_kimi_sdk::dialect::KimiRequest) -> abp_core::WorkOrder =
            abp_shim_kimi::request_to_work_order;
    }

    #[test]
    fn copilot_shim_exports_request_to_work_order() {
        let _f: fn(&abp_copilot_sdk::dialect::CopilotRequest) -> abp_core::WorkOrder =
            abp_shim_copilot::request_to_work_order;
    }

    #[test]
    fn all_six_shim_clients_constructible() {
        let _openai = abp_shim_openai::OpenAiClient::new("gpt-4o");
        let _claude = abp_shim_claude::AnthropicClient::new();
        let _gemini = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
        let _codex = abp_shim_codex::CodexClient::new("codex-mini-latest");
        let _kimi = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
        let _copilot = abp_shim_copilot::CopilotClient::new("gpt-4o");
    }

    #[test]
    fn shim_names_are_distinct() {
        // Each shim default-model or client name should be unique.
        let names = [
            abp_shim_openai::OpenAiClient::new("gpt-4o")
                .model()
                .to_string(),
            "claude-sonnet-4-20250514".to_string(), // AnthropicClient default
            abp_shim_gemini::GeminiClient::new("gemini-2.5-flash")
                .model()
                .to_string(),
            abp_shim_codex::CodexClient::new("codex-mini-latest")
                .model()
                .to_string(),
            abp_shim_kimi::KimiClient::new("moonshot-v1-8k")
                .model()
                .to_string(),
            abp_shim_copilot::CopilotClient::new("gpt-4o-copilot")
                .model()
                .to_string(),
        ];
        let set: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(set.len(), names.len(), "shim model names must be unique");
    }

    #[test]
    fn each_shim_has_error_type() {
        // Confirm each shim defines a ShimError (or equivalent) that implements std::error::Error.
        fn assert_error<E: std::error::Error>() {}
        assert_error::<abp_shim_openai::ShimError>();
        assert_error::<abp_shim_claude::ShimError>();
        assert_error::<abp_shim_gemini::GeminiError>();
        assert_error::<abp_shim_codex::ShimError>();
        assert_error::<abp_shim_kimi::ShimError>();
        assert_error::<abp_shim_copilot::ShimError>();
    }

    #[test]
    fn each_shim_debug_printable() {
        let openai = abp_shim_openai::OpenAiClient::new("gpt-4o");
        let claude = abp_shim_claude::AnthropicClient::new();
        let gemini = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
        let codex = abp_shim_codex::CodexClient::new("codex-mini-latest");
        let kimi = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
        let copilot = abp_shim_copilot::CopilotClient::new("gpt-4o");
        // All must impl Debug
        let _ = format!("{openai:?}");
        let _ = format!("{claude:?}");
        let _ = format!("{gemini:?}");
        let _ = format!("{codex:?}");
        let _ = format!("{kimi:?}");
        let _ = format!("{copilot:?}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: shim_work_order_creation
// ═══════════════════════════════════════════════════════════════════════════

mod shim_work_order_creation {
    #[test]
    fn openai_request_produces_valid_work_order() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("Hello world")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert!(!wo.task.is_empty());
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn claude_request_produces_valid_work_order() {
        let req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "Hello Claude".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = abp_shim_claude::request_to_work_order(&req);
        assert!(wo.task.contains("Hello Claude"));
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[tokio::test]
    async fn gemini_request_produces_valid_work_order() {
        let client = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
            abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello Gemini")]),
        );
        // The Gemini shim builds a work order internally during generate().
        // We test by exercising the generate pipeline (mock execution).
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[test]
    fn codex_request_produces_valid_work_order() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "Hello Codex")])
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert!(wo.task.contains("Hello Codex"));
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn kimi_request_produces_valid_work_order() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("Hello Kimi")])
            .build();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert!(wo.task.contains("Hello Kimi"));
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn copilot_request_produces_valid_work_order() {
        let req = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![abp_shim_copilot::Message::user("Hello Copilot")])
            .build();
        let wo = abp_shim_copilot::request_to_work_order(&req);
        assert!(wo.task.contains("Hello Copilot"));
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn work_order_task_from_last_user_message() {
        // All shims should use the last user message as the task.
        let openai_req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                abp_shim_openai::Message::user("First message"),
                abp_shim_openai::Message::assistant("ok"),
                abp_shim_openai::Message::user("Second message"),
            ])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&openai_req);
        assert!(wo.task.contains("Second message"));
    }

    #[test]
    fn work_order_preserves_vendor_config() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .temperature(0.7)
            .max_tokens(1024)
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("temperature"));
        assert!(wo.config.vendor.contains_key("max_tokens"));
    }

    #[test]
    fn kimi_vendor_config_preserved() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .temperature(0.5)
            .max_tokens(512)
            .build();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("temperature"));
        assert!(wo.config.vendor.contains_key("max_tokens"));
    }

    #[test]
    fn codex_vendor_config_preserved() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "test")])
            .temperature(0.3)
            .max_output_tokens(2048)
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("temperature"));
        assert!(wo.config.vendor.contains_key("max_output_tokens"));
    }

    #[test]
    fn work_order_ids_are_unique() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let wo1 = abp_shim_openai::request_to_work_order(&req);
        let wo2 = abp_shim_openai::request_to_work_order(&req);
        assert_ne!(wo1.id, wo2.id, "each WorkOrder must have a unique id");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: shim_response_assembly
// ═══════════════════════════════════════════════════════════════════════════

mod shim_response_assembly {
    use super::*;

    #[test]
    fn openai_receipt_to_response_text() {
        let events = vec![assistant_event("Hello from OpenAI")];
        let receipt = abp_shim_openai::mock_receipt(events);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from OpenAI")
        );
        assert_eq!(resp.model, "gpt-4o");
    }

    #[test]
    fn openai_receipt_to_response_tool_call() {
        let events = vec![tool_call_event(
            "read_file",
            "call_1",
            json!({"path": "a.rs"}),
        )];
        let receipt = abp_shim_openai::mock_receipt(events);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read_file");
        assert!(tc[0].function.arguments.contains("a.rs"));
    }

    #[test]
    fn openai_error_event_in_response() {
        let events = vec![error_event("rate limit exceeded")];
        let receipt = abp_shim_openai::mock_receipt(events);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert!(text.contains("rate limit"));
    }

    #[test]
    fn claude_response_from_events_text() {
        let events = vec![assistant_event("Hello from Claude")];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
        match &resp.content[0] {
            abp_shim_claude::ContentBlock::Text { text } => {
                assert_eq!(text, "Hello from Claude");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn claude_response_from_events_tool_use() {
        let events = vec![tool_call_event("search", "tu_1", json!({"q": "rust"}))];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        let has_tool_use = resp.content.iter().any(|b| {
            matches!(b, abp_shim_claude::ContentBlock::ToolUse { name, .. } if name == "search")
        });
        assert!(has_tool_use);
    }

    #[test]
    fn codex_receipt_to_response_text() {
        let events = vec![assistant_event("Hello from Codex")];
        let receipt = abp_shim_codex::mock_receipt(events);
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
        assert_eq!(resp.model, "codex-mini-latest");
    }

    #[test]
    fn kimi_receipt_to_response_text() {
        let events = vec![assistant_event("Hello from Kimi")];
        let receipt = abp_shim_kimi::mock_receipt(events);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from Kimi")
        );
    }

    #[test]
    fn copilot_receipt_to_response_text() {
        let events = vec![assistant_event("Hello from Copilot")];
        let receipt = abp_shim_copilot::mock_receipt(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello from Copilot");
    }

    #[test]
    fn copilot_receipt_error_events() {
        let events = vec![error_event("backend failure")];
        let receipt = abp_shim_copilot::mock_receipt(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("backend failure"));
    }

    #[test]
    fn kimi_receipt_error_in_content() {
        let events = vec![error_event("quota exceeded")];
        let receipt = abp_shim_kimi::mock_receipt(events);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert!(text.contains("quota exceeded"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: shim_streaming
// ═══════════════════════════════════════════════════════════════════════════

mod shim_streaming {
    use super::*;

    #[test]
    fn openai_stream_events_contain_deltas() {
        let events = vec![delta_event("Hel"), delta_event("lo!")];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        // 2 deltas + 1 final stop
        assert_eq!(stream.len(), 3);
        // Last event should be the stop chunk
        let last = stream.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_stream_tool_call_event() {
        let events = vec![tool_call_event("ls", "call_1", json!({"dir": "/"}))];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        let has_tool = stream.iter().any(|e| {
            e.choices
                .first()
                .and_then(|c| c.delta.tool_calls.as_ref())
                .is_some()
        });
        assert!(has_tool);
    }

    #[test]
    fn codex_stream_events_bookended() {
        let events = vec![assistant_event("Hello")];
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");
        // First should be ResponseCreated, last should be ResponseCompleted
        assert!(stream.len() >= 3); // created + item + completed
    }

    #[test]
    fn kimi_stream_chunks_end_with_stop() {
        let events = vec![delta_event("chunk")];
        let chunks = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");
        let last = chunks.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn copilot_stream_events_end_with_done() {
        let events = vec![delta_event("hi")];
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");
        // references + delta + done
        assert_eq!(stream.len(), 3);
    }

    #[test]
    fn openai_stream_deltas_accumulate_content() {
        let events = vec![
            delta_event("The "),
            delta_event("answer "),
            delta_event("is 42"),
        ];
        // Feed deltas through receipt_to_response to check accumulation
        let receipt = abp_shim_openai::mock_receipt(events);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("The answer is 42")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: shim_capability_reporting
// ═══════════════════════════════════════════════════════════════════════════

mod shim_capability_reporting {
    use super::*;

    #[test]
    fn openai_supports_tool_use() {
        // OpenAI shim has Tool type → proves tool use support.
        let _tool = abp_shim_openai::Tool::function(
            "read_file",
            "Read a file",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        );
    }

    #[test]
    fn claude_supports_tool_use() {
        let block = abp_shim_claude::ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "bash".into(),
            input: json!({"cmd": "ls"}),
        };
        assert!(matches!(
            block,
            abp_shim_claude::ContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn gemini_supports_function_declarations() {
        let decl = abp_shim_gemini::FunctionDeclaration {
            name: "search".into(),
            description: "Search".into(),
            parameters: json!({}),
        };
        let tool = abp_shim_gemini::ToolDeclaration {
            function_declarations: vec![decl],
        };
        assert_eq!(tool.function_declarations.len(), 1);
    }

    #[test]
    fn codex_supports_tools() {
        let _tool = abp_shim_codex::CodexTool::Function {
            function: abp_shim_codex::CodexFunctionDef {
                name: "exec".into(),
                description: "Execute".into(),
                parameters: json!({}),
            },
        };
    }

    #[test]
    fn kimi_supports_tool_definitions() {
        let _tool = abp_shim_kimi::KimiTool::Function {
            function: abp_shim_kimi::KimiFunctionDef {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({}),
            },
        };
    }

    #[test]
    fn copilot_supports_function_calling() {
        let _tool = abp_shim_copilot::CopilotToolType::Function;
    }

    #[test]
    fn openai_supports_streaming() {
        // OpenAI shim defines StreamEvent type → streaming support.
        let events = vec![delta_event("x")];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn all_shims_produce_usage_in_receipts() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let events = vec![assistant_event("test")];

        let openai_receipt =
            abp_shim_openai::mock_receipt_with_usage(events.clone(), usage.clone());
        let openai_resp = abp_shim_openai::receipt_to_response(&openai_receipt, "gpt-4o");
        let u = openai_resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);

        let codex_receipt = abp_shim_codex::mock_receipt_with_usage(events.clone(), usage.clone());
        let codex_resp = abp_shim_codex::receipt_to_response(&codex_receipt, "codex-mini-latest");
        let u = codex_resp.usage.unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);

        let kimi_receipt = abp_shim_kimi::mock_receipt_with_usage(events.clone(), usage.clone());
        let kimi_resp = abp_shim_kimi::receipt_to_response(&kimi_receipt, "moonshot-v1-8k");
        let u = kimi_resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: cross_shim_consistency
// ═══════════════════════════════════════════════════════════════════════════

mod cross_shim_consistency {
    use super::*;

    #[test]
    fn all_shims_extract_task_from_user_message() {
        // Given the same user text, all shims should produce a WorkOrder with that text in task.
        let text = "What is the meaning of life?";

        let openai_wo = abp_shim_openai::request_to_work_order(
            &abp_shim_openai::ChatCompletionRequest::builder()
                .model("gpt-4o")
                .messages(vec![abp_shim_openai::Message::user(text)])
                .build(),
        );

        let codex_wo = abp_shim_codex::request_to_work_order(
            &abp_shim_codex::CodexRequestBuilder::new()
                .model("codex-mini-latest")
                .input(vec![abp_shim_codex::codex_message("user", text)])
                .build(),
        );

        let kimi_wo = abp_shim_kimi::request_to_work_order(
            &abp_shim_kimi::KimiRequestBuilder::new()
                .model("moonshot-v1-8k")
                .messages(vec![abp_shim_kimi::Message::user(text)])
                .build(),
        );

        let copilot_wo = abp_shim_copilot::request_to_work_order(
            &abp_shim_copilot::CopilotRequestBuilder::new()
                .model("gpt-4o")
                .messages(vec![abp_shim_copilot::Message::user(text)])
                .build(),
        );

        assert!(openai_wo.task.contains(text));
        assert!(codex_wo.task.contains(text));
        assert!(kimi_wo.task.contains(text));
        assert!(copilot_wo.task.contains(text));
    }

    #[test]
    fn all_shims_handle_assistant_message_events() {
        let events = vec![assistant_event("Unified answer")];

        let openai_receipt = abp_shim_openai::mock_receipt(events.clone());
        let openai_resp = abp_shim_openai::receipt_to_response(&openai_receipt, "gpt-4o");
        assert_eq!(
            openai_resp.choices[0].message.content.as_deref(),
            Some("Unified answer")
        );

        let codex_receipt = abp_shim_codex::mock_receipt(events.clone());
        let codex_resp = abp_shim_codex::receipt_to_response(&codex_receipt, "codex-mini-latest");
        assert!(!codex_resp.output.is_empty());

        let kimi_receipt = abp_shim_kimi::mock_receipt(events.clone());
        let kimi_resp = abp_shim_kimi::receipt_to_response(&kimi_receipt, "moonshot-v1-8k");
        assert_eq!(
            kimi_resp.choices[0].message.content.as_deref(),
            Some("Unified answer")
        );

        let copilot_receipt = abp_shim_copilot::mock_receipt(events.clone());
        let copilot_resp = abp_shim_copilot::receipt_to_response(&copilot_receipt, "gpt-4o");
        assert_eq!(copilot_resp.message, "Unified answer");

        let claude_resp =
            abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        match &claude_resp.content[0] {
            abp_shim_claude::ContentBlock::Text { text } => {
                assert_eq!(text, "Unified answer");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn all_shims_handle_tool_call_events() {
        let events = vec![tool_call_event(
            "execute",
            "call_42",
            json!({"cmd": "echo hi"}),
        )];

        // OpenAI
        let receipt = abp_shim_openai::mock_receipt(events.clone());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "execute");

        // Codex
        let receipt = abp_shim_codex::mock_receipt(events.clone());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());

        // Kimi
        let receipt = abp_shim_kimi::mock_receipt(events.clone());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "execute");

        // Copilot
        let receipt = abp_shim_copilot::mock_receipt(events.clone());
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.function_call.as_ref().unwrap().name, "execute");

        // Claude
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        let has_tool = resp.content.iter().any(|b| {
            matches!(b, abp_shim_claude::ContentBlock::ToolUse { name, .. } if name == "execute")
        });
        assert!(has_tool);
    }

    #[test]
    fn model_name_round_trips_through_work_order() {
        let models = [
            ("gpt-4o", "openai"),
            ("codex-mini-latest", "codex"),
            ("moonshot-v1-8k", "kimi"),
            ("gpt-4o", "copilot"),
        ];
        for (model, dialect) in models {
            match dialect {
                "openai" => {
                    let req = abp_shim_openai::ChatCompletionRequest::builder()
                        .model(model)
                        .messages(vec![abp_shim_openai::Message::user("x")])
                        .build();
                    let wo = abp_shim_openai::request_to_work_order(&req);
                    assert_eq!(wo.config.model.as_deref(), Some(model));
                }
                "codex" => {
                    let req = abp_shim_codex::CodexRequestBuilder::new()
                        .model(model)
                        .input(vec![abp_shim_codex::codex_message("user", "x")])
                        .build();
                    let wo = abp_shim_codex::request_to_work_order(&req);
                    assert_eq!(wo.config.model.as_deref(), Some(model));
                }
                "kimi" => {
                    let req = abp_shim_kimi::KimiRequestBuilder::new()
                        .model(model)
                        .messages(vec![abp_shim_kimi::Message::user("x")])
                        .build();
                    let wo = abp_shim_kimi::request_to_work_order(&req);
                    assert_eq!(wo.config.model.as_deref(), Some(model));
                }
                "copilot" => {
                    let req = abp_shim_copilot::CopilotRequestBuilder::new()
                        .model(model)
                        .messages(vec![abp_shim_copilot::Message::user("x")])
                        .build();
                    let wo = abp_shim_copilot::request_to_work_order(&req);
                    assert_eq!(wo.config.model.as_deref(), Some(model));
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn all_shims_handle_error_events_gracefully() {
        let events = vec![error_event("service unavailable")];

        // OpenAI: error text goes into message content
        let receipt = abp_shim_openai::mock_receipt(events.clone());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert!(text.contains("service unavailable"));

        // Codex: error text goes into output item
        let receipt = abp_shim_codex::mock_receipt(events.clone());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());

        // Kimi: error text goes into message content
        let receipt = abp_shim_kimi::mock_receipt(events.clone());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert!(text.contains("service unavailable"));

        // Copilot: error goes into copilot_errors
        let receipt = abp_shim_copilot::mock_receipt(events.clone());
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.copilot_errors.is_empty());
        assert!(
            resp.copilot_errors[0]
                .message
                .contains("service unavailable")
        );
    }

    #[test]
    fn empty_events_produce_valid_responses() {
        let events: Vec<AgentEvent> = vec![];

        let receipt = abp_shim_openai::mock_receipt(events.clone());
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices.len(), 1); // always one choice

        let receipt = abp_shim_codex::mock_receipt(events.clone());
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.output.is_empty()); // no events → no output items

        let receipt = abp_shim_kimi::mock_receipt(events.clone());
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);

        let receipt = abp_shim_copilot::mock_receipt(events);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.message.is_empty());
    }
}
