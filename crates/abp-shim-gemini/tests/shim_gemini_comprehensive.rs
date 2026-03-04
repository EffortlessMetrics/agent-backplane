#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-shim-gemini crate.
//!
//! Covers: request construction, response mapping, content types, streaming,
//! safety settings, generation config, tool/function calling, error mapping,
//! model names, edge cases, and serialization roundtrips.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use abp_gemini_sdk::dialect::{
    self, GeminiCandidate, GeminiContent, GeminiGenerationConfig, GeminiInlineData, GeminiPart,
    GeminiResponse, GeminiStreamChunk, GeminiUsageMetadata, HarmBlockThreshold, HarmCategory,
};
use abp_gemini_sdk::lowering;
use abp_shim_gemini::{
    Candidate, Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
    GeminiClient, GeminiError, GenerateContentRequest, GenerateContentResponse, GenerationConfig,
    Part, SafetySetting, StreamEvent, ToolConfig, ToolDeclaration, UsageMetadata,
    content_from_dialect, content_to_dialect, execute_work_order, from_dialect_response,
    from_dialect_stream_chunk, gen_config_from_dialect, gen_config_to_dialect, ir_to_response,
    ir_to_work_order, make_usage_metadata, part_from_dialect, part_to_dialect, receipt_to_ir,
    receipt_to_stream_events, request_to_ir, safety_to_dialect, to_dialect_request,
    tool_config_to_dialect, tool_decl_to_dialect, usage_from_ir, usage_to_ir,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// =========================================================================
// 1. Request construction and conversion (GenerateContent params)
// =========================================================================

#[test]
fn request_new_sets_model() {
    let req = GenerateContentRequest::new("gemini-2.5-flash");
    assert_eq!(req.model, "gemini-2.5-flash");
    assert!(req.contents.is_empty());
    assert!(req.system_instruction.is_none());
    assert!(req.generation_config.is_none());
    assert!(req.safety_settings.is_none());
    assert!(req.tools.is_none());
    assert!(req.tool_config.is_none());
}

#[test]
fn request_add_content_chaining() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("A")]))
        .add_content(Content::model(vec![Part::text("B")]))
        .add_content(Content::user(vec![Part::text("C")]));
    assert_eq!(req.contents.len(), 3);
    assert_eq!(req.contents[0].role, "user");
    assert_eq!(req.contents[1].role, "model");
    assert_eq!(req.contents[2].role, "user");
}

#[test]
fn request_system_instruction_builder() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .system_instruction(Content::user(vec![Part::text("Be concise")]))
        .add_content(Content::user(vec![Part::text("Hi")]));
    let sys = req.system_instruction.as_ref().unwrap();
    assert_eq!(sys.parts.len(), 1);
    assert!(matches!(&sys.parts[0], Part::Text(t) if t == "Be concise"));
}

#[test]
fn request_generation_config_builder() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(4096),
            ..Default::default()
        });
    let cfg = req.generation_config.as_ref().unwrap();
    assert_eq!(cfg.temperature, Some(0.7));
    assert_eq!(cfg.max_output_tokens, Some(4096));
}

#[test]
fn request_safety_settings_builder() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]);
    assert_eq!(req.safety_settings.as_ref().unwrap().len(), 1);
}

#[test]
fn request_tools_builder() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "search".into(),
                description: "Web search".into(),
                parameters: json!({"type": "object"}),
            }],
        }]);
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn request_tool_config_builder() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });
    assert!(req.tool_config.is_some());
}

#[test]
fn request_to_ir_produces_conversation() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]));
    let (ir, gen_cfg, safety) = request_to_ir(&req).unwrap();
    assert_eq!(ir.conversation.len(), 1);
    assert_eq!(ir.conversation.messages[0].role, IrRole::User);
    assert!(gen_cfg.is_none());
    assert!(safety.is_empty());
}

#[test]
fn request_to_ir_with_system_instruction() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("System prompt")]))
        .add_content(Content::user(vec![Part::text("User msg")]));
    let (ir, _, _) = request_to_ir(&req).unwrap();
    assert_eq!(ir.conversation.messages[0].role, IrRole::System);
    assert_eq!(ir.conversation.messages[0].text_content(), "System prompt");
    assert_eq!(ir.conversation.messages[1].role, IrRole::User);
}

#[test]
fn request_to_ir_preserves_gen_config() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.5),
            ..Default::default()
        });
    let (_, gen_cfg, _) = request_to_ir(&req).unwrap();
    assert_eq!(gen_cfg.unwrap().temperature, Some(0.5));
}

#[test]
fn request_to_ir_preserves_safety_settings() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        }]);
    let (_, _, safety) = request_to_ir(&req).unwrap();
    assert_eq!(safety.len(), 1);
    assert_eq!(
        safety[0].category,
        HarmCategory::HarmCategoryDangerousContent
    );
}

#[test]
fn ir_to_work_order_sets_canonical_model() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("test")]));
    let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_cfg);
    assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.5-pro"));
}

#[test]
fn ir_to_work_order_extracts_user_text_as_task() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Explain Rust")]));
    let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_cfg);
    assert_eq!(wo.task, "Explain Rust");
}

#[test]
fn ir_to_work_order_empty_user_text_fallback() {
    let req = GenerateContentRequest::new("gemini-2.5-flash");
    let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_cfg);
    assert_eq!(wo.task, "Gemini generate content");
}

#[test]
fn ir_to_work_order_with_max_tokens() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(GenerationConfig {
            max_output_tokens: Some(2048),
            ..Default::default()
        });
    let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_cfg);
    assert_eq!(wo.config.max_turns, Some(2048));
}

// =========================================================================
// 2. Response mapping (Gemini responses → ABP receipts)
// =========================================================================

#[test]
fn receipt_to_ir_maps_assistant_message() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        })
        .build();
    let ir = receipt_to_ir(&receipt);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    assert_eq!(ir.messages[0].text_content(), "Hello!");
}

#[test]
fn receipt_to_ir_maps_tool_call() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("tc_1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        })
        .build();
    let ir = receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tc_1");
            assert_eq!(name, "search");
            assert_eq!(input["q"], "rust");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_maps_tool_call_without_id() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "get_time".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        })
        .build();
    let ir = receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, .. } => {
            assert_eq!(id, "gemini_get_time");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_maps_tool_result() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "search".into(),
                tool_use_id: Some("tc_1".into()),
                output: json!("results"),
                is_error: false,
            },
            ext: None,
        })
        .build();
    let ir = receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            content,
        } => {
            assert_eq!(tool_use_id, "tc_1");
            assert!(!is_error);
            match &content[0] {
                IrContentBlock::Text { text } => assert_eq!(text, "results"),
                other => panic!("expected Text, got {other:?}"),
            }
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_maps_tool_result_json_output() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "fetch".into(),
                tool_use_id: None,
                output: json!({"status": 200}),
                is_error: false,
            },
            ext: None,
        })
        .build();
    let ir = receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => match &content[0] {
            IrContentBlock::Text { text } => {
                assert!(text.contains("200"));
            }
            other => panic!("expected Text, got {other:?}"),
        },
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_ignores_non_message_events() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();
    let ir = receipt_to_ir(&receipt);
    assert!(ir.is_empty());
}

#[test]
fn ir_to_response_with_assistant_text() {
    let ir =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hello there!")]);
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(5),
            ..Default::default()
        })
        .build();
    let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
    assert!(!resp.candidates.is_empty());
    assert!(resp.usage_metadata.is_some());
}

#[test]
fn ir_to_response_empty_ir_produces_empty_text_candidate() {
    let ir = IrConversation::new();
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

// =========================================================================
// 3. Content types (text, function_call, function_response, inline_data)
// =========================================================================

#[test]
fn part_text_constructor() {
    let p = Part::text("hello");
    assert!(matches!(p, Part::Text(ref s) if s == "hello"));
}

#[test]
fn part_inline_data_constructor() {
    let p = Part::inline_data("image/png", "base64data");
    match &p {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/png");
            assert_eq!(data, "base64data");
        }
        _ => panic!("expected InlineData"),
    }
}

#[test]
fn part_function_call_constructor() {
    let p = Part::function_call("search", json!({"q": "test"}));
    match &p {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args["q"], "test");
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn part_function_response_constructor() {
    let p = Part::function_response("search", json!("result"));
    match &p {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response, &json!("result"));
        }
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn content_user_helper() {
    let c = Content::user(vec![Part::text("hi")]);
    assert_eq!(c.role, "user");
    assert_eq!(c.parts.len(), 1);
}

#[test]
fn content_model_helper() {
    let c = Content::model(vec![Part::text("hello")]);
    assert_eq!(c.role, "model");
}

#[test]
fn content_multiple_parts() {
    let c = Content::user(vec![
        Part::text("Describe this image"),
        Part::inline_data("image/jpeg", "abc123"),
    ]);
    assert_eq!(c.parts.len(), 2);
    assert!(matches!(&c.parts[0], Part::Text(_)));
    assert!(matches!(&c.parts[1], Part::InlineData { .. }));
}

#[test]
fn part_to_dialect_text() {
    let p = Part::text("hello");
    let d = part_to_dialect(&p);
    assert!(matches!(d, GeminiPart::Text(ref s) if s == "hello"));
}

#[test]
fn part_to_dialect_inline_data() {
    let p = Part::inline_data("image/png", "data");
    let d = part_to_dialect(&p);
    match d {
        GeminiPart::InlineData(ref id) => {
            assert_eq!(id.mime_type, "image/png");
            assert_eq!(id.data, "data");
        }
        _ => panic!("expected InlineData"),
    }
}

#[test]
fn part_to_dialect_function_call() {
    let p = Part::function_call("fn_a", json!({"x": 1}));
    let d = part_to_dialect(&p);
    match d {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "fn_a");
            assert_eq!(args["x"], 1);
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn part_to_dialect_function_response() {
    let p = Part::function_response("fn_a", json!("ok"));
    let d = part_to_dialect(&p);
    match d {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "fn_a");
            assert_eq!(response, json!("ok"));
        }
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn part_from_dialect_text() {
    let d = GeminiPart::Text("hi".into());
    let p = part_from_dialect(&d);
    assert!(matches!(p, Part::Text(ref s) if s == "hi"));
}

#[test]
fn part_from_dialect_inline_data() {
    let d = GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "abc".into(),
    });
    let p = part_from_dialect(&d);
    match p {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/jpeg");
            assert_eq!(data, "abc");
        }
        _ => panic!("expected InlineData"),
    }
}

#[test]
fn part_from_dialect_function_call() {
    let d = GeminiPart::FunctionCall {
        name: "fn".into(),
        args: json!({}),
    };
    let p = part_from_dialect(&d);
    assert!(matches!(p, Part::FunctionCall { ref name, .. } if name == "fn"));
}

#[test]
fn part_from_dialect_function_response() {
    let d = GeminiPart::FunctionResponse {
        name: "fn".into(),
        response: json!(42),
    };
    let p = part_from_dialect(&d);
    match p {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "fn");
            assert_eq!(response, json!(42));
        }
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn content_to_dialect_preserves_role_and_parts() {
    let c = Content::user(vec![Part::text("test"), Part::inline_data("img/png", "d")]);
    let d = content_to_dialect(&c);
    assert_eq!(d.role, "user");
    assert_eq!(d.parts.len(), 2);
}

#[test]
fn content_from_dialect_preserves_role_and_parts() {
    let d = GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("hi".into())],
    };
    let c = content_from_dialect(&d);
    assert_eq!(c.role, "model");
    assert_eq!(c.parts.len(), 1);
}

#[test]
fn inline_data_ir_roundtrip() {
    let content = Content::user(vec![Part::inline_data("image/jpeg", "abc123")]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }
    let back = lowering::from_ir(&ir);
    let shim_content = content_from_dialect(&back[0]);
    match &shim_content.parts[0] {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/jpeg");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn function_call_ir_roundtrip() {
    let content = Content::model(vec![Part::function_call("search", json!({"q": "rust"}))]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "search");
            assert_eq!(input["q"], "rust");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn function_response_ir_roundtrip() {
    let content = Content::user(vec![Part::function_response("search", json!("results"))]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "gemini_search");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// =========================================================================
// 4. Streaming semantics (Gemini streaming → ABP events)
// =========================================================================

#[tokio::test]
async fn streaming_generates_events() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Stream test")]));
    let stream = client.generate_stream(req).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    assert!(!events.is_empty());
}

#[tokio::test]
async fn streaming_last_chunk_has_usage() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]));
    let stream = client.generate_stream(req).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    let last = events.last().unwrap();
    assert!(last.usage_metadata.is_some());
}

#[test]
fn stream_event_text_accessor() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("chunk")]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(event.text(), Some("chunk"));
}

#[test]
fn stream_event_text_returns_none_for_function_call() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::function_call("fn", json!({}))]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert!(event.text().is_none());
}

#[test]
fn stream_event_text_returns_none_for_empty_candidates() {
    let event = StreamEvent {
        candidates: vec![],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 10,
            total_token_count: 15,
        }),
    };
    assert!(event.text().is_none());
}

#[test]
fn from_dialect_stream_chunk_text() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("delta".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let event = from_dialect_stream_chunk(&chunk);
    assert_eq!(event.text(), Some("delta"));
    assert!(event.usage_metadata.is_none());
}

#[test]
fn from_dialect_stream_chunk_with_usage() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 20,
            candidates_token_count: 30,
            total_token_count: 50,
        }),
    };
    let event = from_dialect_stream_chunk(&chunk);
    assert!(event.candidates.is_empty());
    let u = event.usage_metadata.unwrap();
    assert_eq!(u.total_token_count, 50);
}

#[test]
fn from_dialect_stream_chunk_function_call() {
    let chunk = GeminiStreamChunk {
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
        usage_metadata: None,
    };
    let event = from_dialect_stream_chunk(&chunk);
    match &event.candidates[0].content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args["q"], "test");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn receipt_to_stream_events_text() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(20),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello".into(),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    assert!(events.len() >= 2); // text event + usage event
    assert_eq!(events[0].text(), Some("Hello"));
    assert!(events.last().unwrap().usage_metadata.is_some());
}

#[test]
fn receipt_to_stream_events_includes_tool_calls() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(5),
            output_tokens: Some(10),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({"q": "test"}),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    assert!(events.len() >= 2);
    match &events[0].candidates[0].content.parts[0] {
        Part::FunctionCall { name, .. } => assert_eq!(name, "search"),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn receipt_to_stream_events_delta_text() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(5),
            output_tokens: Some(5),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk1".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk2".into(),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    assert!(events.len() >= 3); // 2 deltas + usage
    assert_eq!(events[0].text(), Some("chunk1"));
    assert_eq!(events[1].text(), Some("chunk2"));
}

#[test]
fn receipt_to_stream_events_no_usage_if_zero() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "Hi".into() },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    // Only text event, no usage event since default usage is zero
    assert_eq!(events.len(), 1);
}

// =========================================================================
// 5. Safety settings and generation config
// =========================================================================

#[test]
fn safety_to_dialect_maps_fields() {
    let s = SafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let d = safety_to_dialect(&s);
    assert_eq!(d.category, HarmCategory::HarmCategoryHarassment);
    assert_eq!(d.threshold, HarmBlockThreshold::BlockMediumAndAbove);
}

#[test]
fn safety_setting_serde_roundtrip() {
    let ss = SafetySetting {
        category: HarmCategory::HarmCategorySexuallyExplicit,
        threshold: HarmBlockThreshold::BlockLowAndAbove,
    };
    let json = serde_json::to_string(&ss).unwrap();
    let back: SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ss);
}

#[test]
fn harm_category_all_variants_serde() {
    let categories = [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in &categories {
        let json = serde_json::to_string(cat).unwrap();
        let back: HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, cat);
    }
}

#[test]
fn harm_block_threshold_all_variants_serde() {
    let thresholds = [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for thr in &thresholds {
        let json = serde_json::to_string(thr).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, thr);
    }
}

#[test]
fn gen_config_to_dialect_all_fields() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(1024),
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        stop_sequences: Some(vec!["END".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let d = gen_config_to_dialect(&cfg);
    assert_eq!(d.max_output_tokens, Some(1024));
    assert_eq!(d.temperature, Some(0.7));
    assert_eq!(d.top_p, Some(0.9));
    assert_eq!(d.top_k, Some(40));
    assert_eq!(d.stop_sequences, Some(vec!["END".into()]));
    assert_eq!(d.response_mime_type, Some("application/json".into()));
    assert!(d.response_schema.is_some());
    assert!(d.candidate_count.is_none());
}

#[test]
fn gen_config_from_dialect_roundtrip() {
    let dialect_cfg = GeminiGenerationConfig {
        max_output_tokens: Some(512),
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(20),
        candidate_count: Some(2),
        stop_sequences: Some(vec!["DONE".into()]),
        response_mime_type: Some("text/plain".into()),
        response_schema: None,
    };
    let back = gen_config_from_dialect(&dialect_cfg);
    assert_eq!(back.max_output_tokens, Some(512));
    assert_eq!(back.temperature, Some(0.3));
    assert_eq!(back.top_p, Some(0.8));
    assert_eq!(back.top_k, Some(20));
    assert_eq!(back.stop_sequences, Some(vec!["DONE".into()]));
    assert_eq!(back.response_mime_type, Some("text/plain".into()));
}

#[test]
fn generation_config_default_is_all_none() {
    let cfg = GenerationConfig::default();
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.stop_sequences.is_none());
    assert!(cfg.response_mime_type.is_none());
    assert!(cfg.response_schema.is_none());
}

#[test]
fn generation_config_serde_roundtrip() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(2048),
        temperature: Some(0.8),
        top_p: Some(0.95),
        top_k: Some(40),
        stop_sequences: Some(vec!["END".into(), "STOP".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    let back: GenerationConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.max_output_tokens, Some(2048));
    assert_eq!(back.temperature, Some(0.8));
    assert_eq!(back.top_p, Some(0.95));
    assert_eq!(back.top_k, Some(40));
    assert_eq!(back.stop_sequences, Some(vec!["END".into(), "STOP".into()]));
}

#[test]
fn generation_config_skip_none_on_serialize() {
    let cfg = GenerationConfig::default();
    let json_str = serde_json::to_string(&cfg).unwrap();
    assert_eq!(json_str, "{}");
}

// =========================================================================
// 6. Tool/function calling bridging
// =========================================================================

#[test]
fn tool_decl_to_dialect_preserves_fields() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "get_weather".into(),
            description: "Get weather for a location".into(),
            parameters: json!({
                "type": "object",
                "properties": {"location": {"type": "string"}},
                "required": ["location"]
            }),
        }],
    };
    let d = tool_decl_to_dialect(&tool);
    assert_eq!(d.function_declarations.len(), 1);
    assert_eq!(d.function_declarations[0].name, "get_weather");
    assert_eq!(
        d.function_declarations[0].description,
        "Get weather for a location"
    );
    assert_eq!(
        d.function_declarations[0].parameters["required"][0],
        "location"
    );
}

#[test]
fn tool_decl_multiple_functions() {
    let tool = ToolDeclaration {
        function_declarations: vec![
            FunctionDeclaration {
                name: "read_file".into(),
                description: "Read".into(),
                parameters: json!({}),
            },
            FunctionDeclaration {
                name: "write_file".into(),
                description: "Write".into(),
                parameters: json!({}),
            },
        ],
    };
    let d = tool_decl_to_dialect(&tool);
    assert_eq!(d.function_declarations.len(), 2);
    assert_eq!(d.function_declarations[0].name, "read_file");
    assert_eq!(d.function_declarations[1].name, "write_file");
}

#[test]
fn tool_config_to_dialect_auto_mode() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    };
    let d = tool_config_to_dialect(&tc);
    assert_eq!(d.function_calling_config.mode, FunctionCallingMode::Auto);
    assert!(d.function_calling_config.allowed_function_names.is_none());
}

#[test]
fn tool_config_to_dialect_any_mode_with_allowed() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "read".into()]),
        },
    };
    let d = tool_config_to_dialect(&tc);
    assert_eq!(d.function_calling_config.mode, FunctionCallingMode::Any);
    let names = d.function_calling_config.allowed_function_names.unwrap();
    assert_eq!(names, vec!["search", "read"]);
}

#[test]
fn tool_config_to_dialect_none_mode() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::None,
            allowed_function_names: None,
        },
    };
    let d = tool_config_to_dialect(&tc);
    assert_eq!(d.function_calling_config.mode, FunctionCallingMode::None);
}

#[test]
fn function_calling_mode_serde_roundtrip() {
    for mode in [
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ] {
        let json_str = serde_json::to_string(&mode).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, mode);
    }
}

#[test]
fn canonical_tool_def_roundtrip_through_dialect() {
    let canonical = dialect::CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let gemini = dialect::tool_def_to_gemini(&canonical);
    assert_eq!(gemini.name, "read_file");
    let back = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(back, canonical);
}

#[test]
fn to_dialect_request_preserves_tools_and_tool_config() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "f".into(),
                description: "d".into(),
                parameters: json!({}),
            }],
        }])
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });
    let dialect = to_dialect_request(&req);
    assert!(dialect.tools.is_some());
    assert!(dialect.tool_config.is_some());
    assert_eq!(dialect.tools.unwrap()[0].function_declarations[0].name, "f");
}

// =========================================================================
// 7. Error mapping (Gemini errors → ABP errors)
// =========================================================================

#[test]
fn gemini_error_request_conversion_display() {
    let err = GeminiError::RequestConversion("bad input".into());
    let msg = err.to_string();
    assert!(msg.contains("request conversion"));
    assert!(msg.contains("bad input"));
}

#[test]
fn gemini_error_response_conversion_display() {
    let err = GeminiError::ResponseConversion("bad output".into());
    let msg = err.to_string();
    assert!(msg.contains("response conversion"));
    assert!(msg.contains("bad output"));
}

#[test]
fn gemini_error_backend_display() {
    let err = GeminiError::BackendError("timeout".into());
    let msg = err.to_string();
    assert!(msg.contains("backend error"));
    assert!(msg.contains("timeout"));
}

#[test]
fn gemini_error_serde_from_json_error() {
    let json_err = serde_json::from_str::<i32>("not_a_number").unwrap_err();
    let err = GeminiError::Serde(json_err);
    let msg = err.to_string();
    assert!(msg.contains("serde error"));
}

#[test]
fn gemini_error_is_debug_printable() {
    let err = GeminiError::BackendError("test".into());
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("BackendError"));
}

// =========================================================================
// 8. Model name handling
// =========================================================================

#[test]
fn client_model_accessor() {
    let client = GeminiClient::new("gemini-2.5-pro");
    assert_eq!(client.model(), "gemini-2.5-pro");
}

#[test]
fn client_model_from_string() {
    let model = String::from("gemini-2.5-flash");
    let client = GeminiClient::new(model);
    assert_eq!(client.model(), "gemini-2.5-flash");
}

#[test]
fn model_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-2.5-flash");
    assert_eq!(canonical, "google/gemini-2.5-flash");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gemini-2.5-flash");
}

#[test]
fn model_name_variations_canonical() {
    let models = [
        "gemini-2.5-flash",
        "gemini-2.5-pro",
        "gemini-2.0-flash",
        "gemini-2.0-flash-lite",
        "gemini-1.5-flash",
        "gemini-1.5-pro",
    ];
    for model in &models {
        let canonical = dialect::to_canonical_model(model);
        assert!(canonical.starts_with("google/"), "model={model}");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(&back, model);
    }
}

#[test]
fn unknown_model_canonical_mapping() {
    let canonical = dialect::to_canonical_model("gemini-99.0-future");
    assert_eq!(canonical, "google/gemini-99.0-future");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gemini-99.0-future");
}

#[test]
fn from_canonical_model_without_prefix() {
    let result = dialect::from_canonical_model("some-model");
    assert_eq!(result, "some-model");
}

#[test]
fn is_known_model_true_for_known() {
    assert!(dialect::is_known_model("gemini-2.5-flash"));
    assert!(dialect::is_known_model("gemini-2.5-pro"));
}

#[test]
fn is_known_model_false_for_unknown() {
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("gemini-99.0"));
}

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "gemini/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "gemini-2.5-flash");
}

// =========================================================================
// 9. Edge cases (empty, unicode, large payloads)
// =========================================================================

#[test]
fn empty_contents_request() {
    let req = GenerateContentRequest::new("gemini-2.5-flash");
    let dialect = to_dialect_request(&req);
    assert!(dialect.contents.is_empty());
}

#[test]
fn empty_parts_content() {
    let content = Content::user(vec![]);
    let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(content);
    let dialect = to_dialect_request(&req);
    assert!(dialect.contents[0].parts.is_empty());
}

#[test]
fn unicode_text_part() {
    let text = "こんにちは世界 🌍 مرحبا العالم";
    let p = Part::text(text);
    let d = part_to_dialect(&p);
    let back = part_from_dialect(&d);
    assert_eq!(back, Part::Text(text.into()));
}

#[test]
fn unicode_in_request_serde_roundtrip() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("日本語テスト 🇯🇵")]));
    let json_str = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json_str).unwrap();
    match &back.contents[0].parts[0] {
        Part::Text(t) => assert_eq!(t, "日本語テスト 🇯🇵"),
        _ => panic!("expected Text"),
    }
}

#[test]
fn large_text_part() {
    let text = "x".repeat(100_000);
    let p = Part::text(&text);
    let d = part_to_dialect(&p);
    let back = part_from_dialect(&d);
    assert_eq!(back, Part::Text(text));
}

#[test]
fn empty_string_text_part() {
    let p = Part::text("");
    let d = part_to_dialect(&p);
    let back = part_from_dialect(&d);
    assert_eq!(back, Part::Text("".into()));
}

#[test]
fn empty_function_call_args() {
    let p = Part::function_call("noop", json!({}));
    let d = part_to_dialect(&p);
    match d {
        GeminiPart::FunctionCall { args, .. } => {
            assert_eq!(args, json!({}));
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn deeply_nested_function_args() {
    let nested = json!({
        "a": {"b": {"c": {"d": {"e": "deep"}}}}
    });
    let p = Part::function_call("deep_fn", nested.clone());
    let d = part_to_dialect(&p);
    let back = part_from_dialect(&d);
    match back {
        Part::FunctionCall { args, .. } => {
            assert_eq!(args["a"]["b"]["c"]["d"]["e"], "deep");
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn multi_turn_four_exchanges() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Q1")]))
        .add_content(Content::model(vec![Part::text("A1")]))
        .add_content(Content::user(vec![Part::text("Q2")]))
        .add_content(Content::model(vec![Part::text("A2")]));
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, None);
    assert_eq!(ir.len(), 4);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[2].role, IrRole::User);
    assert_eq!(ir.messages[3].role, IrRole::Assistant);
}

#[test]
fn multi_modal_mixed_parts() {
    let content = Content::user(vec![
        Part::text("What's in this image?"),
        Part::inline_data("image/png", "base64data=="),
    ]);
    let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(content);
    let dialect = to_dialect_request(&req);
    assert_eq!(dialect.contents[0].parts.len(), 2);
    assert!(matches!(&dialect.contents[0].parts[0], GeminiPart::Text(_)));
    assert!(matches!(
        &dialect.contents[0].parts[1],
        GeminiPart::InlineData(_)
    ));
}

#[test]
fn response_function_calls_accessor_filters_text() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::function_call("fn_a", json!({"x": 1})),
                Part::text("some text"),
                Part::function_call("fn_b", json!({"y": 2})),
            ]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "fn_a");
    assert_eq!(calls[1].0, "fn_b");
}

#[test]
fn response_function_calls_empty_for_no_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
    };
    assert!(resp.function_calls().is_empty());
}

#[test]
fn response_text_none_when_only_function_calls() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::function_call("fn", json!({}))]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert!(resp.text().is_none());
}

// =========================================================================
// 10. Serialization roundtrips
// =========================================================================

#[test]
fn part_text_serde_roundtrip() {
    let part = Part::text("Hello, world!");
    let json_str = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, Part::Text("Hello, world!".into()));
}

#[test]
fn part_inline_data_serde_roundtrip() {
    let part = Part::inline_data("image/png", "iVBORw0KGgo=");
    let json_str = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_function_call_serde_roundtrip() {
    let part = Part::function_call("get_weather", json!({"location": "London"}));
    let json_str = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_function_response_serde_roundtrip() {
    let part = Part::function_response("get_weather", json!({"temp": 22}));
    let json_str = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

#[test]
fn content_serde_roundtrip() {
    let c = Content::user(vec![
        Part::text("test"),
        Part::inline_data("image/jpeg", "abc"),
    ]);
    let json_str = serde_json::to_string(&c).unwrap();
    let back: Content = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.parts.len(), 2);
}

#[test]
fn usage_metadata_serde_roundtrip() {
    let usage = UsageMetadata {
        prompt_token_count: 42,
        candidates_token_count: 18,
        total_token_count: 60,
    };
    let json_str = serde_json::to_string(&usage).unwrap();
    let back: UsageMetadata = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn generate_content_request_serde_roundtrip() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.5),
            ..Default::default()
        });
    let json_str = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.model, "gemini-2.5-flash");
    assert_eq!(back.contents.len(), 1);
    assert_eq!(back.generation_config.unwrap().temperature, Some(0.5));
}

#[test]
fn tool_declaration_serde_roundtrip() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }],
    };
    let json_str = serde_json::to_string(&tool).unwrap();
    let back: ToolDeclaration = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn tool_config_serde_roundtrip() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into()]),
        },
    };
    let json_str = serde_json::to_string(&tc).unwrap();
    let back: ToolConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn stream_event_serde_roundtrip() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("chunk")]),
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    let json_str = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.text(), Some("chunk"));
    assert_eq!(back.usage_metadata.unwrap().total_token_count, 15);
}

// =========================================================================
// Additional: usage conversions, dialect response, execute, pipeline
// =========================================================================

#[test]
fn usage_to_ir_conversion() {
    let usage = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    let ir = usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);
}

#[test]
fn usage_from_ir_conversion() {
    let ir = IrUsage::from_io(200, 100);
    let usage = usage_from_ir(&ir);
    assert_eq!(usage.prompt_token_count, 200);
    assert_eq!(usage.candidates_token_count, 100);
    assert_eq!(usage.total_token_count, 300);
}

#[test]
fn usage_ir_roundtrip() {
    let original = UsageMetadata {
        prompt_token_count: 150,
        candidates_token_count: 75,
        total_token_count: 225,
    };
    let ir = usage_to_ir(&original);
    let back = usage_from_ir(&ir);
    assert_eq!(back, original);
}

#[test]
fn make_usage_metadata_with_tokens() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    };
    let meta = make_usage_metadata(&usage).unwrap();
    assert_eq!(meta.prompt_token_count, 100);
    assert_eq!(meta.candidates_token_count, 50);
    assert_eq!(meta.total_token_count, 150);
}

#[test]
fn make_usage_metadata_none_when_zero() {
    let usage = UsageNormalized::default();
    assert!(make_usage_metadata(&usage).is_none());
}

#[test]
fn make_usage_metadata_partial_tokens() {
    let usage = UsageNormalized {
        input_tokens: Some(10),
        output_tokens: None,
        ..Default::default()
    };
    let meta = make_usage_metadata(&usage).unwrap();
    assert_eq!(meta.prompt_token_count, 10);
    assert_eq!(meta.candidates_token_count, 0);
    assert_eq!(meta.total_token_count, 10);
}

#[test]
fn from_dialect_response_maps_text() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let shim = from_dialect_response(&resp);
    assert_eq!(shim.text(), Some("Hello"));
    assert_eq!(shim.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn from_dialect_response_maps_usage() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hi".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    let shim = from_dialect_response(&resp);
    let u = shim.usage_metadata.unwrap();
    assert_eq!(u.prompt_token_count, 10);
    assert_eq!(u.candidates_token_count, 5);
    assert_eq!(u.total_token_count, 15);
}

#[test]
fn from_dialect_response_multiple_candidates() {
    let resp = GeminiResponse {
        candidates: vec![
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("A".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("B".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
        ],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let shim = from_dialect_response(&resp);
    assert_eq!(shim.candidates.len(), 2);
    assert_eq!(shim.text(), Some("A"));
}

#[test]
fn execute_work_order_produces_receipt() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Test")]));
    let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_cfg);
    let receipt = execute_work_order(&wo);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.usage.input_tokens.is_some());
    assert!(receipt.usage.output_tokens.is_some());
    assert!(!receipt.trace.is_empty());
}

#[test]
fn execute_work_order_trace_has_run_events() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]));
    let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_cfg);
    let receipt = execute_work_order(&wo);
    let has_run_started = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let has_run_completed = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_run_started);
    assert!(has_run_completed);
}

#[tokio::test]
async fn full_pipeline_roundtrip() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Ping")]));
    let resp = client.generate(req).await.unwrap();
    assert!(!resp.candidates.is_empty());
    let text = resp.text().unwrap();
    assert!(text.contains("Ping"));
}

#[tokio::test]
async fn full_pipeline_with_system_instruction() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be concise")]))
        .add_content(Content::user(vec![Part::text("Test")]));
    let resp = client.generate(req).await.unwrap();
    assert!(!resp.candidates.is_empty());
}

#[test]
fn to_dialect_request_preserves_all_fields() {
    let req = GenerateContentRequest::new("model-x")
        .add_content(Content::user(vec![Part::text("hi")]))
        .system_instruction(Content::user(vec![Part::text("Be helpful")]))
        .generation_config(GenerationConfig {
            temperature: Some(1.0),
            ..Default::default()
        })
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "f".into(),
                description: "d".into(),
                parameters: json!({}),
            }],
        }])
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });
    let dialect = to_dialect_request(&req);
    assert_eq!(dialect.model, "model-x");
    assert!(dialect.system_instruction.is_some());
    assert!(dialect.generation_config.is_some());
    assert!(dialect.tools.is_some());
    assert!(dialect.tool_config.is_some());
}

#[test]
fn capability_manifest_contains_expected_entries() {
    let manifest = dialect::capability_manifest();
    assert!(manifest.contains_key(&abp_core::Capability::Streaming));
    assert!(manifest.contains_key(&abp_core::Capability::ToolRead));
}

#[test]
fn dialect_map_response_text_event() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("mapped".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "mapped"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_function_call_event() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "fn".into(),
                    args: json!({"a": 1}),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "fn");
            assert_eq!(input["a"], 1);
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_chunk_produces_delta() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("token".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = dialect::map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "token"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_same_as_chunk() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("x".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let a = dialect::map_stream_chunk(&chunk);
    let b = dialect::map_stream_event(&chunk);
    assert_eq!(a.len(), b.len());
}

// GeminiRequest -> WorkOrder conversion (via From impl)
#[test]
fn gemini_request_into_work_order() {
    let dialect_req = to_dialect_request(
        &GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Explain Rust")])),
    );
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert_eq!(wo.task, "Explain Rust");
    assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-flash"));
}

// Receipt -> GeminiResponse conversion (via From impl)
#[test]
fn receipt_into_gemini_response_complete() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Response".into(),
            },
            ext: None,
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn receipt_into_gemini_response_partial() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Partial)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(
        resp.candidates[0].finish_reason.as_deref(),
        Some("MAX_TOKENS")
    );
}

#[test]
fn receipt_into_gemini_response_failed() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Failed)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
}
