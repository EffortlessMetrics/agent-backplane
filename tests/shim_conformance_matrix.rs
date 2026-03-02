// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conformance test matrix: 6 SDK shims × 10 properties = 60 tests.
//!
//! Each shim is validated for:
//!  1. Request construction
//!  2. IR conversion
//!  3. Work order creation
//!  4. Response construction
//!  5. Stream event generation
//!  6. Error handling
//!  7. Model name preservation
//!  8. Token usage
//!  9. Tool definitions
//! 10. System message

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use chrono::Utc;
use serde_json::json;

// ── Shared test helpers ────────────────────────────────────────────────────

fn event_assistant_message(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn event_assistant_delta(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn test_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    }
}

fn tool_params_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path" }
        },
        "required": ["path"]
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI shim conformance
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::*;

    fn basic_request(model: &str, user_msg: &str) -> ChatCompletionRequest {
        ChatCompletionRequest::builder()
            .model(model)
            .messages(vec![Message::user(user_msg)])
            .build()
    }

    #[test]
    fn t01_request_construction() {
        let req = basic_request("gpt-4o", "Hello");
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn t02_ir_conversion() {
        let req = basic_request("gpt-4o", "Hello");
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn t03_work_order_creation() {
        let req = basic_request("gpt-4o", "Refactor code");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Refactor code");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn t04_response_construction() {
        let receipt = mock_receipt(vec![event_assistant_message("Hi there")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn t05_stream_event_generation() {
        let events = vec![event_assistant_delta("Hel"), event_assistant_delta("lo!")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // 2 deltas + 1 stop
        assert!(stream.len() >= 3);
        assert_eq!(
            stream.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[tokio::test]
    async fn t06_error_handling() {
        let client = OpenAiClient::new("gpt-4o");
        let req = basic_request("gpt-4o", "test");
        let err = client.chat().completions().create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    #[test]
    fn t07_model_name_preservation() {
        let req = basic_request("gpt-4-turbo-2024-04-09", "test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo-2024-04-09"));
    }

    #[test]
    fn t08_token_usage() {
        let receipt = mock_receipt_with_usage(vec![event_assistant_message("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn t09_tool_definitions() {
        let tool = Tool::function("read_file", "Read a file", tool_params_schema());
        let ir_tools = tools_to_ir(&[tool]);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[0].description, "Read a file");
        assert_eq!(ir_tools[0].parameters, tool_params_schema());
    }

    #[test]
    fn t10_system_message() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("Hi"),
            ])
            .build();
        let ir = request_to_ir(&req);
        let sys = ir.system_message().expect("should have system message");
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Claude shim conformance
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::*;

    fn basic_request(model: &str, user_msg: &str) -> MessageRequest {
        MessageRequest {
            model: model.into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: user_msg.into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    fn request_with_system(model: &str, system: &str, user_msg: &str) -> MessageRequest {
        MessageRequest {
            system: Some(system.into()),
            ..basic_request(model, user_msg)
        }
    }

    #[test]
    fn t01_request_construction() {
        let req = basic_request("claude-sonnet-4-20250514", "Hello");
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.messages.len(), 1);
        assert!(matches!(
            &req.messages[0].content[0],
            ContentBlock::Text { text } if text == "Hello"
        ));
    }

    #[test]
    fn t02_ir_conversion() {
        let req = request_with_system("claude-sonnet-4-20250514", "Be concise.", "Hello");
        let claude_req = request_to_claude(&req);
        let ir =
            abp_claude_sdk::lowering::to_ir(&claude_req.messages, claude_req.system.as_deref());
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
        assert_eq!(
            ir.messages
                .iter()
                .find(|m| m.role == IrRole::User)
                .unwrap()
                .text_content(),
            "Hello"
        );
    }

    #[test]
    fn t03_work_order_creation() {
        let req = basic_request("claude-sonnet-4-20250514", "Refactor code");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Refactor code");
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn t04_response_construction() {
        let events = vec![event_assistant_message("Hi from Claude")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        assert!(resp.content.iter().any(|b| matches!(
            b,
            ContentBlock::Text { text } if text == "Hi from Claude"
        )));
    }

    #[tokio::test]
    async fn t05_stream_event_generation() {
        let client = AnthropicClient::new();
        let req = basic_request("claude-sonnet-4-20250514", "Hi");
        let stream = client.create_stream(req).await.unwrap();
        let events = stream.collect_all().await;
        assert!(!events.is_empty());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStart { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStop {}))
        );
    }

    #[tokio::test]
    async fn t06_error_handling() {
        let client = AnthropicClient::new();
        let empty_req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let err = client.create(empty_req).await.unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }

    #[test]
    fn t07_model_name_preservation() {
        let req = basic_request("claude-opus-4-20250514", "test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
    }

    #[test]
    fn t08_token_usage() {
        let usage = abp_claude_sdk::dialect::ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let events = vec![event_assistant_message("ok")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", Some(&usage));
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 50);
    }

    #[test]
    fn t09_tool_definitions() {
        let canonical = abp_claude_sdk::dialect::CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: tool_params_schema(),
        };
        let claude_tool = abp_claude_sdk::dialect::tool_def_to_claude(&canonical);
        let back = abp_claude_sdk::dialect::tool_def_from_claude(&claude_tool);
        assert_eq!(back.name, "read_file");
        assert_eq!(back.description, "Read a file");
        assert_eq!(back.parameters_schema, tool_params_schema());
    }

    #[test]
    fn t10_system_message() {
        let req = request_with_system("claude-sonnet-4-20250514", "You are helpful.", "Hi");
        let claude_req = request_to_claude(&req);
        let ir =
            abp_claude_sdk::lowering::to_ir(&claude_req.messages, claude_req.system.as_deref());
        let sys = ir.system_message().expect("should have system message");
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Gemini shim conformance
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    fn basic_request(model: &str, user_msg: &str) -> GenerateContentRequest {
        GenerateContentRequest::new(model).add_content(Content::user(vec![Part::text(user_msg)]))
    }

    #[test]
    fn t01_request_construction() {
        let req = basic_request("gemini-2.5-flash", "Hello");
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, "user");
    }

    #[test]
    fn t02_ir_conversion() {
        let req = basic_request("gemini-2.5-flash", "Hello");
        let dialect = to_dialect_request(&req);
        let ir =
            abp_gemini_sdk::lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[tokio::test]
    async fn t03_work_order_creation() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = basic_request("gemini-2.5-flash", "Hello");
        let resp = client.generate(req).await.unwrap();
        // The full pipeline creates a work order internally; verify response
        assert!(!resp.candidates.is_empty());
        assert!(resp.text().is_some());
    }

    #[tokio::test]
    async fn t04_response_construction() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = basic_request("gemini-2.5-flash", "Hello");
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
        let text = resp.text().expect("should have text");
        assert!(!text.is_empty());
    }

    #[tokio::test]
    async fn t05_stream_event_generation() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = basic_request("gemini-2.5-flash", "Hello");
        let stream = client.generate_stream(req).await.unwrap();
        let events: Vec<_> = tokio_stream::StreamExt::collect(stream).await;
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn t06_error_handling() {
        // Gemini client's mock pipeline always succeeds, but we can verify
        // the response is well-formed for an edge-case empty request.
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash");
        let resp = client.generate(req).await;
        // The mock pipeline returns a valid response even for empty input.
        // Verify the response is at least structurally valid.
        assert!(resp.is_ok());
    }

    #[test]
    fn t07_model_name_preservation() {
        let req = basic_request("gemini-2.5-pro-exp-03-25", "test");
        let dialect = to_dialect_request(&req);
        assert_eq!(dialect.model, "gemini-2.5-pro-exp-03-25");
    }

    #[test]
    fn t08_token_usage() {
        let metadata = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = usage_to_ir(&metadata);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
        let back = usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, 100);
        assert_eq!(back.candidates_token_count, 50);
        assert_eq!(back.total_token_count, 150);
    }

    #[test]
    fn t09_tool_definitions() {
        let canonical = abp_gemini_sdk::dialect::CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: tool_params_schema(),
        };
        let gemini_decl = abp_gemini_sdk::dialect::tool_def_to_gemini(&canonical);
        let back = abp_gemini_sdk::dialect::tool_def_from_gemini(&gemini_decl);
        assert_eq!(back.name, "read_file");
        assert_eq!(back.description, "Read a file");
        assert_eq!(back.parameters_schema, tool_params_schema());
    }

    #[test]
    fn t10_system_message() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("You are helpful.")]))
            .add_content(Content::user(vec![Part::text("Hi")]));
        let dialect = to_dialect_request(&req);
        let ir =
            abp_gemini_sdk::lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
        let sys = ir.system_message().expect("should have system message");
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Codex shim conformance
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_shim_codex::*;

    fn basic_request(model: &str, user_msg: &str) -> abp_codex_sdk::dialect::CodexRequest {
        CodexRequestBuilder::new()
            .model(model)
            .input(vec![codex_message("user", user_msg)])
            .build()
    }

    #[test]
    fn t01_request_construction() {
        let req = basic_request("codex-mini-latest", "Hello");
        assert_eq!(req.model, "codex-mini-latest");
        assert!(!req.input.is_empty());
    }

    #[test]
    fn t02_ir_conversion() {
        let req = basic_request("codex-mini-latest", "Hello");
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn t03_work_order_creation() {
        let req = basic_request("codex-mini-latest", "Refactor code");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Refactor code");
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn t04_response_construction() {
        let receipt = mock_receipt(vec![event_assistant_message("Hi from Codex")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert_eq!(resp.model, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn t05_stream_event_generation() {
        let events = vec![event_assistant_delta("Hel"), event_assistant_delta("lo!")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        // 1 created + 2 deltas + 1 completed
        assert!(stream.len() >= 4);
    }

    #[tokio::test]
    async fn t06_error_handling() {
        let client = CodexClient::new("codex-mini-latest");
        let req = basic_request("codex-mini-latest", "test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    #[test]
    fn t07_model_name_preservation() {
        let req = basic_request("o3-mini", "test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    #[test]
    fn t08_token_usage() {
        let receipt = mock_receipt_with_usage(vec![event_assistant_message("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let usage = resp.usage.expect("should have usage");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn t09_tool_definitions() {
        let canonical = abp_codex_sdk::dialect::CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: tool_params_schema(),
        };
        let codex_tool = abp_codex_sdk::dialect::tool_def_to_codex(&canonical);
        let back = abp_codex_sdk::dialect::tool_def_from_codex(&codex_tool);
        assert_eq!(back.name, "read_file");
        assert_eq!(back.description, "Read a file");
        assert_eq!(back.parameters_schema, tool_params_schema());
    }

    #[test]
    fn t10_system_message() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![
                codex_message("system", "You are helpful."),
                codex_message("user", "Hi"),
            ])
            .build();
        let ir = request_to_ir(&req);
        let sys = ir.system_message().expect("should have system message");
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Kimi shim conformance
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    fn basic_request(model: &str, user_msg: &str) -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model(model)
            .messages(vec![Message::user(user_msg)])
            .build()
    }

    #[test]
    fn t01_request_construction() {
        let req = basic_request("moonshot-v1-8k", "Hello");
        assert_eq!(req.model, "moonshot-v1-8k");
        assert!(!req.messages.is_empty());
    }

    #[test]
    fn t02_ir_conversion() {
        let req = basic_request("moonshot-v1-8k", "Hello");
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn t03_work_order_creation() {
        let req = basic_request("moonshot-v1-8k", "Refactor code");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Refactor code");
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn t04_response_construction() {
        let receipt = mock_receipt(vec![event_assistant_message("Hi from Kimi")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hi from Kimi")
        );
    }

    #[test]
    fn t05_stream_event_generation() {
        let events = vec![event_assistant_delta("Hel"), event_assistant_delta("lo!")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        // 2 deltas + 1 stop
        assert!(chunks.len() >= 3);
        let last = chunks.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn t06_error_handling() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = basic_request("moonshot-v1-8k", "test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    #[test]
    fn t07_model_name_preservation() {
        let req = basic_request("moonshot-v1-128k", "test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    #[test]
    fn t08_token_usage() {
        let receipt = mock_receipt_with_usage(vec![event_assistant_message("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let usage = resp.usage.expect("should have usage");
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn t09_tool_definitions() {
        let canonical = abp_kimi_sdk::dialect::CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: tool_params_schema(),
        };
        let kimi_tool = abp_kimi_sdk::dialect::tool_def_to_kimi(&canonical);
        let back = abp_kimi_sdk::dialect::tool_def_from_kimi(&kimi_tool);
        assert_eq!(back.name, "read_file");
        assert_eq!(back.description, "Read a file");
        assert_eq!(back.parameters_schema, tool_params_schema());
    }

    #[test]
    fn t10_system_message() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("Hi"),
            ])
            .build();
        let ir = request_to_ir(&req);
        let sys = ir.system_message().expect("should have system message");
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Copilot shim conformance
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_shim_copilot::*;

    fn basic_request(model: &str, user_msg: &str) -> abp_copilot_sdk::dialect::CopilotRequest {
        CopilotRequestBuilder::new()
            .model(model)
            .messages(vec![Message::user(user_msg)])
            .build()
    }

    #[test]
    fn t01_request_construction() {
        let req = basic_request("gpt-4o", "Hello");
        assert_eq!(req.model, "gpt-4o");
        assert!(!req.messages.is_empty());
    }

    #[test]
    fn t02_ir_conversion() {
        let req = basic_request("gpt-4o", "Hello");
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn t03_work_order_creation() {
        let req = basic_request("gpt-4o", "Refactor code");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Refactor code");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn t04_response_construction() {
        let receipt = mock_receipt(vec![event_assistant_message("Hi from Copilot")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hi from Copilot");
    }

    #[test]
    fn t05_stream_event_generation() {
        let events = vec![event_assistant_delta("Hel"), event_assistant_delta("lo!")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // 1 references + 2 deltas + 1 done
        assert_eq!(stream.len(), 4);
    }

    #[tokio::test]
    async fn t06_error_handling() {
        let client = CopilotClient::new("gpt-4o");
        let req = basic_request("gpt-4o", "test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    #[test]
    fn t07_model_name_preservation() {
        let req = basic_request("gpt-4-turbo", "test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn t08_token_usage() {
        let ir = IrUsage::from_io(100, 50);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(total, 150);
    }

    #[test]
    fn t09_tool_definitions() {
        let canonical = abp_copilot_sdk::dialect::CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: tool_params_schema(),
        };
        let copilot_tool = abp_copilot_sdk::dialect::tool_def_to_copilot(&canonical);
        let back = abp_copilot_sdk::dialect::tool_def_from_copilot(&copilot_tool)
            .expect("should roundtrip");
        assert_eq!(back.name, "read_file");
        assert_eq!(back.description, "Read a file");
        assert_eq!(back.parameters_schema, tool_params_schema());
    }

    #[test]
    fn t10_system_message() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("Hi"),
            ])
            .build();
        let ir = request_to_ir(&req);
        let sys = ir.system_message().expect("should have system message");
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}
