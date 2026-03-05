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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep surface-area tests for the Gemini shim public API.
//!
//! Categories covered:
//!  1. GenerateContent request (basic, all parameters)
//!  2. System instruction handling
//!  3. Content parts (text, inline_data, function_call, function_response)
//!  4. Tool definitions (function declarations)
//!  5. Response handling (candidates, prompt_feedback, usage_metadata)
//!  6. Streaming
//!  7. Error responses
//!  8. Safety settings (harm categories, block thresholds)
//!  9. Generation config
//! 10. Model selection
//! 11. Conversion to ABP WorkOrder
//! 12. Conversion from ABP Receipt
//! 13. Serde roundtrip
//! 14. Edge cases (multi-turn, multi-modal, code execution)

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use abp_gemini_sdk::dialect::{
    self, GeminiCandidate, GeminiContent, GeminiPart, GeminiPromptFeedback, GeminiResponse,
    GeminiSafetyRating, GeminiStreamChunk, GeminiUsageMetadata, HarmBlockThreshold, HarmCategory,
    HarmProbability,
};
use abp_gemini_sdk::lowering;
use abp_shim_gemini::client::{Client, ClientError};
use abp_shim_gemini::{
    Candidate, Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
    GeminiError, GenerateContentRequest, GenerateContentResponse, GenerationConfig, Part,
    PipelineClient, SafetySetting, StreamEvent, ToolConfig, ToolDeclaration, UsageMetadata,
    content_from_dialect, content_to_dialect, execute_work_order, from_dialect_response,
    from_dialect_stream_chunk, gen_config_from_dialect, gen_config_to_dialect, ir_to_response,
    ir_to_work_order, make_usage_metadata, part_from_dialect, part_to_dialect,
    receipt_to_stream_events, request_to_ir, safety_to_dialect, to_dialect_request,
    tool_config_to_dialect, tool_decl_to_dialect, usage_from_ir, usage_to_ir,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

// =========================================================================
// 1. GenerateContent request — basic construction & all parameters
// =========================================================================

#[test]
fn req_new_sets_model_and_empty_fields() {
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
fn req_builder_chains_all_setters() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("q1")]))
        .add_content(Content::model(vec![Part::text("a1")]))
        .system_instruction(Content::user(vec![Part::text("sys")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.3),
            top_p: Some(0.8),
            top_k: Some(20),
            candidate_count: None,
            max_output_tokens: Some(1024),
            stop_sequences: Some(vec!["STOP".into()]),
            response_mime_type: Some("text/plain".into()),
            response_schema: None,
        })
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }])
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "f".into(),
                description: "d".into(),
                parameters: json!({}),
            }],
        }])
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Any,
                allowed_function_names: Some(vec!["f".into()]),
            },
        });

    assert_eq!(req.model, "gemini-2.5-pro");
    assert_eq!(req.contents.len(), 2);
    assert!(req.system_instruction.is_some());
    assert!(req.generation_config.is_some());
    assert!(req.safety_settings.is_some());
    assert!(req.tools.is_some());
    assert!(req.tool_config.is_some());
}

#[test]
fn req_add_content_is_order_preserving() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("first")]))
        .add_content(Content::model(vec![Part::text("second")]))
        .add_content(Content::user(vec![Part::text("third")]));
    assert_eq!(req.contents.len(), 3);
    assert_eq!(req.contents[0].role, "user");
    assert_eq!(req.contents[1].role, "model");
    assert_eq!(req.contents[2].role, "user");
}

#[test]
fn req_wire_format_omits_none_optional_fields() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("hi")]));
    let json_str = serde_json::to_string(&req).unwrap();
    assert!(!json_str.contains("systemInstruction"));
    assert!(!json_str.contains("generationConfig"));
    assert!(!json_str.contains("safetySettings"));
    assert!(!json_str.contains("tools"));
    assert!(!json_str.contains("toolConfig"));
}

// =========================================================================
// 2. System instruction handling (Gemini-specific)
// =========================================================================

#[test]
fn system_instruction_lowered_to_ir_system_role() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be terse.")]))
        .add_content(Content::user(vec![Part::text("Hello")]));
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[0].text_content(), "Be terse.");
}

#[test]
fn system_instruction_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "system msg"),
        IrMessage::text(IrRole::User, "user msg"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn system_instruction_extracted_separately() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = lowering::extract_system_instruction(&conv).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "You are helpful."),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn empty_system_instruction_not_added_to_ir() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![]))
        .add_content(Content::user(vec![Part::text("hi")]));
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
    // Empty system instruction should not produce a system message
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
}

// =========================================================================
// 3. Content parts — text, inline_data, function_call, function_response
// =========================================================================

#[test]
fn part_text_construction_and_match() {
    let p = Part::text("hello world");
    assert!(matches!(p, Part::Text(ref s) if s == "hello world"));
}

#[test]
fn part_inline_data_construction_and_match() {
    let p = Part::inline_data("image/webp", "base64==");
    match &p {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/webp");
            assert_eq!(data, "base64==");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn part_function_call_construction_and_match() {
    let p = Part::function_call("my_tool", json!({"key": "val"}));
    match &p {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "my_tool");
            assert_eq!(args["key"], "val");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn part_function_response_construction_and_match() {
    let p = Part::function_response("my_tool", json!(42));
    match &p {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "my_tool");
            assert_eq!(response, &json!(42));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn part_to_dialect_roundtrip_all_variants() {
    let parts = vec![
        Part::text("hello"),
        Part::inline_data("image/png", "abc"),
        Part::function_call("fn", json!({"a": 1})),
        Part::function_response("fn", json!("ok")),
    ];
    for original in parts {
        let dialect = part_to_dialect(&original);
        let back = part_from_dialect(&dialect);
        assert_eq!(back, original);
    }
}

#[test]
fn content_multi_part_preserved_through_dialect() {
    let content = Content::user(vec![
        Part::text("Describe this image"),
        Part::inline_data("image/jpeg", "data=="),
    ]);
    let dialect = content_to_dialect(&content);
    assert_eq!(dialect.parts.len(), 2);
    let back = content_from_dialect(&dialect);
    assert_eq!(back.parts.len(), 2);
    assert_eq!(back.role, "user");
}

#[test]
fn inline_data_lowered_to_ir_image_block() {
    let content = Content::user(vec![Part::inline_data("image/png", "iVBOR")]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBOR");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn inline_data_roundtrip_through_ir() {
    let content = Content::user(vec![Part::inline_data("image/gif", "R0lGODlh")]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    let back_dialects = lowering::from_ir(&ir);
    let back = content_from_dialect(&back_dialects[0]);
    match &back.parts[0] {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/gif");
            assert_eq!(data, "R0lGODlh");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

// =========================================================================
// 4. Tool definitions — function declarations
// =========================================================================

#[test]
fn tool_declaration_single_function() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "get_time".into(),
            description: "Returns the current time".into(),
            parameters: json!({"type": "object", "properties": {}}),
        }],
    };
    let dialect = tool_decl_to_dialect(&tool);
    assert_eq!(dialect.function_declarations.len(), 1);
    assert_eq!(dialect.function_declarations[0].name, "get_time");
}

#[test]
fn tool_declaration_multiple_functions() {
    let tool = ToolDeclaration {
        function_declarations: vec![
            FunctionDeclaration {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
            FunctionDeclaration {
                name: "write_file".into(),
                description: "Write a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            },
        ],
    };
    let dialect = tool_decl_to_dialect(&tool);
    assert_eq!(dialect.function_declarations.len(), 2);
    assert_eq!(dialect.function_declarations[1].name, "write_file");
}

#[test]
fn tool_config_auto_mode_no_restrictions() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    };
    let dialect = tool_config_to_dialect(&tc);
    assert_eq!(
        dialect.function_calling_config.mode,
        FunctionCallingMode::Auto
    );
    assert!(
        dialect
            .function_calling_config
            .allowed_function_names
            .is_none()
    );
}

#[test]
fn tool_config_any_mode_with_allowed_names() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into()]),
        },
    };
    let dialect = tool_config_to_dialect(&tc);
    assert_eq!(
        dialect.function_calling_config.mode,
        FunctionCallingMode::Any
    );
    assert_eq!(
        dialect
            .function_calling_config
            .allowed_function_names
            .as_ref()
            .unwrap(),
        &["search"]
    );
}

#[test]
fn tool_config_none_mode() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::None,
            allowed_function_names: None,
        },
    };
    let dialect = tool_config_to_dialect(&tc);
    assert_eq!(
        dialect.function_calling_config.mode,
        FunctionCallingMode::None
    );
}

#[test]
fn function_call_to_ir_synthesizes_id() {
    let content = Content::model(vec![Part::function_call("search", json!({"q": "rust"}))]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "gemini_search");
            assert_eq!(name, "search");
            assert_eq!(input["q"], "rust");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn function_response_to_ir_uses_synthesized_id() {
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
// 5. Response handling — candidates, prompt_feedback, usage_metadata
// =========================================================================

#[test]
fn response_text_accessor_first_candidate() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("Hello!")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("Hello!"));
}

#[test]
fn response_text_none_when_no_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert!(resp.text().is_none());
}

#[test]
fn response_function_calls_accessor() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::function_call("tool_a", json!({})),
                Part::function_call("tool_b", json!({"x": 1})),
            ]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "tool_a");
    assert_eq!(calls[1].0, "tool_b");
}

#[test]
fn response_function_calls_empty_for_text_only() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("just text")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert!(resp.function_calls().is_empty());
}

#[test]
fn from_dialect_response_maps_prompt_feedback_aware_response() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("ok".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
            citation_metadata: None,
        }],
        prompt_feedback: Some(GeminiPromptFeedback {
            block_reason: None,
            safety_ratings: Some(vec![GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
        }),
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    let shim = from_dialect_response(&resp);
    assert_eq!(shim.text(), Some("ok"));
    assert_eq!(shim.usage_metadata.as_ref().unwrap().total_token_count, 15);
}

#[test]
fn from_dialect_response_maps_usage_metadata() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("hi".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 25,
            total_token_count: 75,
        }),
    };
    let shim = from_dialect_response(&resp);
    let usage = shim.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 50);
    assert_eq!(usage.candidates_token_count, 25);
    assert_eq!(usage.total_token_count, 75);
}

// =========================================================================
// 6. Streaming
// =========================================================================

#[tokio::test]
async fn stream_produces_at_least_text_and_usage() {
    let client = PipelineClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("stream test")]));
    let stream = client.generate_stream(req).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    assert!(events.len() >= 2);
    assert!(events.last().unwrap().usage_metadata.is_some());
}

#[test]
fn stream_event_text_accessor() {
    let evt = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("delta")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(evt.text(), Some("delta"));
}

#[test]
fn stream_event_text_none_when_empty() {
    let evt = StreamEvent {
        candidates: vec![],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 1,
            candidates_token_count: 1,
            total_token_count: 2,
        }),
    };
    assert!(evt.text().is_none());
}

#[test]
fn from_dialect_stream_chunk_maps_text_and_usage() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("chunk".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 3,
            candidates_token_count: 7,
            total_token_count: 10,
        }),
    };
    let evt = from_dialect_stream_chunk(&chunk);
    assert_eq!(evt.text(), Some("chunk"));
    assert_eq!(evt.usage_metadata.as_ref().unwrap().total_token_count, 10);
}

#[test]
fn from_dialect_stream_chunk_preserves_finish_reason_stop() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("done".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let evt = from_dialect_stream_chunk(&chunk);
    assert_eq!(evt.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn receipt_to_stream_events_maps_text_then_usage() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(8),
            output_tokens: Some(12),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "text chunk".into(),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    assert!(events.len() >= 2);
    assert_eq!(events[0].text(), Some("text chunk"));
    let last = events.last().unwrap();
    assert!(last.usage_metadata.is_some());
}

#[test]
fn receipt_to_stream_events_maps_tool_call() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(1),
            output_tokens: Some(1),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "grep".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({"pattern": "fn main"}),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    match &events[0].candidates[0].content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "grep");
            assert_eq!(args["pattern"], "fn main");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// =========================================================================
// 7. Error responses
// =========================================================================

#[test]
fn gemini_error_request_conversion_display() {
    let err = GeminiError::RequestConversion("invalid content".into());
    assert!(err.to_string().contains("invalid content"));
}

#[test]
fn gemini_error_response_conversion_display() {
    let err = GeminiError::ResponseConversion("unexpected format".into());
    assert!(err.to_string().contains("unexpected format"));
}

#[test]
fn gemini_error_backend_display() {
    let err = GeminiError::BackendError("timeout".into());
    assert!(err.to_string().contains("timeout"));
}

#[test]
fn gemini_error_serde_from_json_error() {
    let result: Result<Part, _> = serde_json::from_str("not-json");
    let json_err = result.unwrap_err();
    let err = GeminiError::from(json_err);
    assert!(err.to_string().contains("serde error"));
}

#[test]
fn client_error_api_rate_limit() {
    let err = ClientError::Api {
        status: 429,
        body: "RESOURCE_EXHAUSTED: rate limit exceeded".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("429"));
    assert!(msg.contains("rate limit"));
}

#[test]
fn client_error_api_auth() {
    let err = ClientError::Api {
        status: 401,
        body: "API key not valid".into(),
    };
    assert!(err.to_string().contains("401"));
}

#[test]
fn client_error_api_model_not_found() {
    let err = ClientError::Api {
        status: 404,
        body: "models/gemini-nonexistent is not found".into(),
    };
    assert!(err.to_string().contains("404"));
    assert!(err.to_string().contains("not found"));
}

#[test]
fn client_error_builder_display() {
    let err = ClientError::Builder("TLS init failed".into());
    assert!(err.to_string().contains("TLS init failed"));
}

// =========================================================================
// 8. Safety settings — harm categories & block thresholds
// =========================================================================

#[test]
fn safety_all_harm_categories_serde_roundtrip() {
    let cats = [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in &cats {
        let ss = SafetySetting {
            category: *cat,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let json_str = serde_json::to_string(&ss).unwrap();
        let back: SafetySetting = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, ss);
    }
}

#[test]
fn safety_all_thresholds_serde_roundtrip() {
    let thresholds = [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for thr in &thresholds {
        let json_str = serde_json::to_string(thr).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, *thr);
    }
}

#[test]
fn safety_to_dialect_preserves_values() {
    let ss = SafetySetting {
        category: HarmCategory::HarmCategoryDangerousContent,
        threshold: HarmBlockThreshold::BlockOnlyHigh,
    };
    let d = safety_to_dialect(&ss);
    assert_eq!(d.category, HarmCategory::HarmCategoryDangerousContent);
    assert_eq!(d.threshold, HarmBlockThreshold::BlockOnlyHigh);
}

#[test]
fn safety_settings_in_request_flow_to_dialect() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![
            SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryHateSpeech,
                threshold: HarmBlockThreshold::BlockLowAndAbove,
            },
        ]);
    let dialect = to_dialect_request(&req);
    let ss = dialect.safety_settings.unwrap();
    assert_eq!(ss.len(), 2);
    assert_eq!(ss[0].category, HarmCategory::HarmCategoryHarassment);
    assert_eq!(ss[1].threshold, HarmBlockThreshold::BlockLowAndAbove);
}

#[test]
fn harm_probability_variants_serde() {
    let probs = [
        HarmProbability::Negligible,
        HarmProbability::Low,
        HarmProbability::Medium,
        HarmProbability::High,
    ];
    for p in &probs {
        let json_str = serde_json::to_string(p).unwrap();
        let back: HarmProbability = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, *p);
    }
}

// =========================================================================
// 9. Generation config
// =========================================================================

#[test]
fn gen_config_default_is_all_none() {
    let cfg = GenerationConfig::default();
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.stop_sequences.is_none());
    assert!(cfg.response_mime_type.is_none());
    assert!(cfg.response_schema.is_none());
}

#[test]
fn gen_config_to_dialect_all_fields() {
    let cfg = GenerationConfig {
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        candidate_count: None,
        max_output_tokens: Some(2048),
        stop_sequences: Some(vec!["END".into(), "HALT".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let d = gen_config_to_dialect(&cfg);
    assert_eq!(d.temperature, Some(0.7));
    assert_eq!(d.top_p, Some(0.9));
    assert_eq!(d.top_k, Some(40));
    assert_eq!(d.max_output_tokens, Some(2048));
    assert_eq!(d.stop_sequences, Some(vec!["END".into(), "HALT".into()]));
    assert!(d.candidate_count.is_none());
}

#[test]
fn gen_config_roundtrip_through_dialect() {
    let cfg = GenerationConfig {
        temperature: Some(1.5),
        top_p: Some(0.95),
        top_k: Some(64),
        candidate_count: None,
        max_output_tokens: Some(8192),
        stop_sequences: None,
        response_mime_type: None,
        response_schema: None,
    };
    let dialect = gen_config_to_dialect(&cfg);
    let back = gen_config_from_dialect(&dialect);
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
}

#[test]
fn gen_config_json_uses_camel_case() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(256),
        temperature: Some(0.5),
        ..Default::default()
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    assert!(json_str.contains("maxOutputTokens"));
    assert!(!json_str.contains("max_output_tokens"));
}

#[test]
fn gen_config_response_schema_structured_output() {
    let cfg = GenerationConfig {
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name"]
        })),
        ..Default::default()
    };
    let d = gen_config_to_dialect(&cfg);
    assert_eq!(d.response_mime_type.as_deref(), Some("application/json"));
    assert!(d.response_schema.is_some());
}

// =========================================================================
// 10. Model selection
// =========================================================================

#[test]
fn model_25_flash_canonical() {
    assert_eq!(
        dialect::to_canonical_model("gemini-2.5-flash"),
        "google/gemini-2.5-flash"
    );
}

#[test]
fn model_25_pro_canonical() {
    assert_eq!(
        dialect::to_canonical_model("gemini-2.5-pro"),
        "google/gemini-2.5-pro"
    );
}

#[test]
fn model_15_pro_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-1.5-pro");
    assert_eq!(dialect::from_canonical_model(&canonical), "gemini-1.5-pro");
}

#[test]
fn model_20_flash_lite_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-2.0-flash-lite");
    assert_eq!(
        dialect::from_canonical_model(&canonical),
        "gemini-2.0-flash-lite"
    );
}

#[test]
fn known_models_check() {
    assert!(dialect::is_known_model("gemini-2.5-flash"));
    assert!(dialect::is_known_model("gemini-2.5-pro"));
    assert!(dialect::is_known_model("gemini-2.0-flash"));
    assert!(dialect::is_known_model("gemini-1.5-flash"));
    assert!(dialect::is_known_model("gemini-1.5-pro"));
}

#[test]
fn unknown_model_not_known() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("claude-3.5-sonnet"));
    assert!(!dialect::is_known_model("gemini-ultra"));
}

#[test]
fn client_stores_and_returns_model() {
    let c = PipelineClient::new("gemini-2.5-pro");
    assert_eq!(c.model(), "gemini-2.5-pro");
}

#[test]
fn from_canonical_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("google/gemini-2.0-flash"),
        "gemini-2.0-flash"
    );
}

#[test]
fn from_canonical_passthrough_without_prefix() {
    assert_eq!(
        dialect::from_canonical_model("custom-model"),
        "custom-model"
    );
}

// =========================================================================
// 11. Conversion to ABP WorkOrder
// =========================================================================

#[test]
fn work_order_uses_canonical_model_name() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("hello")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.5-flash"));
}

#[test]
fn work_order_task_from_user_messages() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Question one")]))
        .add_content(Content::model(vec![Part::text("Answer")]))
        .add_content(Content::user(vec![Part::text("Question two")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    assert!(wo.task.contains("Question one"));
    assert!(wo.task.contains("Question two"));
}

#[test]
fn work_order_max_turns_from_gen_config() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("x")]))
        .generation_config(GenerationConfig {
            max_output_tokens: Some(512),
            ..Default::default()
        });
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    assert_eq!(wo.config.max_turns, Some(512));
}

#[test]
fn work_order_default_task_when_no_user_text() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::model(vec![Part::text("model only")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    assert_eq!(wo.task, "Gemini generate content");
}

#[test]
fn execute_work_order_produces_complete_receipt() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    let receipt = execute_work_order(&wo);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.usage.input_tokens.is_some());
    assert!(receipt.usage.output_tokens.is_some());
    assert!(!receipt.trace.is_empty());
}

// =========================================================================
// 12. Conversion from ABP Receipt
// =========================================================================

#[test]
fn receipt_to_ir_assistant_messages() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Howdy!".into(),
            },
            ext: None,
        })
        .build();
    let ir = abp_shim_gemini::receipt_to_ir(&receipt);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    assert_eq!(ir.messages[0].text_content(), "Howdy!");
}

#[test]
fn receipt_to_ir_tool_call_with_explicit_id() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"file": "main.rs"}),
            },
            ext: None,
        })
        .build();
    let ir = abp_shim_gemini::receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_abc");
            assert_eq!(name, "read");
            assert_eq!(input["file"], "main.rs");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_tool_call_synthesizes_id_when_none() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        })
        .build();
    let ir = abp_shim_gemini::receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, .. } => {
            assert_eq!(id, "gemini_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_tool_result() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("call_xyz".into()),
                output: json!("file contents"),
                is_error: false,
            },
            ext: None,
        })
        .build();
    let ir = abp_shim_gemini::receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "call_xyz");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn ir_to_response_produces_model_candidate() {
    let ir =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Generated output")]);
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(15),
            output_tokens: Some(30),
            ..Default::default()
        })
        .build();
    let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].content.role, "model");
    assert_eq!(resp.text(), Some("Generated output"));
    let u = resp.usage_metadata.unwrap();
    assert_eq!(u.prompt_token_count, 15);
    assert_eq!(u.candidates_token_count, 30);
    assert_eq!(u.total_token_count, 45);
}

#[test]
fn ir_to_response_empty_ir_produces_stub_candidate() {
    let ir = IrConversation::new();
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
    assert!(!resp.candidates.is_empty());
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

// =========================================================================
// 13. Serde roundtrip
// =========================================================================

#[test]
fn serde_roundtrip_generate_content_request() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("question")]))
        .add_content(Content::model(vec![Part::text("answer")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1000),
            ..Default::default()
        });
    let json_str = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.model, "gemini-2.5-pro");
    assert_eq!(back.contents.len(), 2);
    assert_eq!(
        back.generation_config.as_ref().unwrap().temperature,
        Some(0.7)
    );
}

#[test]
fn serde_roundtrip_generate_content_response() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("test output")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 20,
            candidates_token_count: 10,
            total_token_count: 30,
        }),
        prompt_feedback: None,
    };
    let json_str = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.text(), Some("test output"));
    assert_eq!(back.usage_metadata.unwrap().total_token_count, 30);
}

#[test]
fn serde_roundtrip_stream_event() {
    let evt = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("delta")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 3,
            total_token_count: 8,
        }),
    };
    let json_str = serde_json::to_string(&evt).unwrap();
    let back: StreamEvent = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.text(), Some("delta"));
    assert_eq!(back.usage_metadata.unwrap().total_token_count, 8);
}

#[test]
fn serde_roundtrip_safety_setting() {
    let ss = SafetySetting {
        category: HarmCategory::HarmCategorySexuallyExplicit,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json_str = serde_json::to_string(&ss).unwrap();
    let back: SafetySetting = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, ss);
}

#[test]
fn serde_roundtrip_function_declaration() {
    let decl = FunctionDeclaration {
        name: "lookup".into(),
        description: "Lookup an item".into(),
        parameters: json!({"type": "object", "properties": {"id": {"type": "string"}}}),
    };
    let json_str = serde_json::to_string(&decl).unwrap();
    let back: FunctionDeclaration = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, decl);
}

#[test]
fn serde_roundtrip_tool_declaration() {
    let td = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "fn1".into(),
            description: "desc".into(),
            parameters: json!({}),
        }],
    };
    let json_str = serde_json::to_string(&td).unwrap();
    let back: ToolDeclaration = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, td);
}

#[test]
fn serde_roundtrip_usage_metadata() {
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
fn serde_roundtrip_generation_config() {
    let cfg = GenerationConfig {
        temperature: Some(0.9),
        top_p: Some(0.95),
        top_k: Some(50),
        candidate_count: None,
        max_output_tokens: Some(4096),
        stop_sequences: Some(vec!["END".into()]),
        response_mime_type: Some("text/plain".into()),
        response_schema: None,
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    let back: GenerationConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
}

#[test]
fn serde_roundtrip_part_all_variants() {
    let parts = vec![
        Part::text("hello"),
        Part::inline_data("image/png", "abc"),
        Part::function_call("fn", json!({"x": 1})),
        Part::function_response("fn", json!("ok")),
    ];
    for p in &parts {
        let json_str = serde_json::to_string(p).unwrap();
        let back: Part = serde_json::from_str(&json_str).unwrap();
        assert_eq!(&back, p);
    }
}

// =========================================================================
// 14. Edge cases — multi-turn, multi-modal, usage, misc
// =========================================================================

#[test]
fn multi_turn_ir_roundtrip_preserves_order() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Turn 1")]))
        .add_content(Content::model(vec![Part::text("Reply 1")]))
        .add_content(Content::user(vec![Part::text("Turn 2")]))
        .add_content(Content::model(vec![Part::text("Reply 2")]));
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
    assert_eq!(ir.len(), 4);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[2].role, IrRole::User);
    assert_eq!(ir.messages[3].role, IrRole::Assistant);
}

#[test]
fn multi_modal_text_and_image_in_one_content() {
    let content = Content::user(vec![
        Part::text("What's in this image?"),
        Part::inline_data("image/jpeg", "AABB=="),
    ]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].content.len(), 2);
    assert!(matches!(
        &ir.messages[0].content[0],
        IrContentBlock::Text { .. }
    ));
    assert!(matches!(
        &ir.messages[0].content[1],
        IrContentBlock::Image { .. }
    ));
}

#[test]
fn function_call_then_response_multi_turn_flow() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Find files")]))
        .add_content(Content::model(vec![Part::function_call(
            "search",
            json!({"q": "*.rs"}),
        )]))
        .add_content(Content::user(vec![Part::function_response(
            "search",
            json!(["main.rs", "lib.rs"]),
        )]))
        .add_content(Content::model(vec![Part::text("Found 2 files")]));

    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, None);
    assert_eq!(ir.len(), 4);
    assert!(matches!(
        &ir.messages[1].content[0],
        IrContentBlock::ToolUse { .. }
    ));
    assert!(matches!(
        &ir.messages[2].content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn usage_to_ir_and_back_roundtrip() {
    let usage = UsageMetadata {
        prompt_token_count: 300,
        candidates_token_count: 150,
        total_token_count: 450,
    };
    let ir = usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 300);
    assert_eq!(ir.output_tokens, 150);
    assert_eq!(ir.total_tokens, 450);
    let back = usage_from_ir(&ir);
    assert_eq!(back, usage);
}

#[test]
fn make_usage_metadata_none_when_both_zero() {
    let u = UsageNormalized::default();
    assert!(make_usage_metadata(&u).is_none());
}

#[test]
fn make_usage_metadata_some_when_input_nonzero() {
    let u = UsageNormalized {
        input_tokens: Some(5),
        output_tokens: Some(0),
        ..Default::default()
    };
    let meta = make_usage_metadata(&u).unwrap();
    assert_eq!(meta.prompt_token_count, 5);
    assert_eq!(meta.candidates_token_count, 0);
    assert_eq!(meta.total_token_count, 5);
}

#[tokio::test]
async fn full_generate_pipeline_roundtrip() {
    let client = PipelineClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Say hi")]));
    let resp = client.generate(req).await.unwrap();
    assert!(!resp.candidates.is_empty());
    let text = resp.text().unwrap();
    assert!(!text.is_empty());
    assert!(resp.usage_metadata.is_some());
}

#[tokio::test]
async fn full_generate_with_system_instruction() {
    let client = PipelineClient::new("gemini-2.5-flash");
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Respond in JSON")]))
        .add_content(Content::user(vec![Part::text("Hello")]));
    let resp = client.generate(req).await.unwrap();
    assert!(!resp.candidates.is_empty());
}

#[test]
fn client_builder_default_base_url() {
    let c = Client::new("test-key").unwrap();
    assert!(c.base_url().contains("generativelanguage.googleapis.com"));
}

#[test]
fn client_builder_custom_url_and_timeout() {
    let c = Client::builder("key")
        .base_url("https://proxy.example.com/v1")
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    assert_eq!(c.base_url(), "https://proxy.example.com/v1");
}

#[test]
fn to_dialect_request_preserves_every_field() {
    let req = GenerateContentRequest::new("m")
        .add_content(Content::user(vec![Part::text("q")]))
        .system_instruction(Content::user(vec![Part::text("sys")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.1),
            ..Default::default()
        })
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }])
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

    let d = to_dialect_request(&req);
    assert_eq!(d.model, "m");
    assert_eq!(d.contents.len(), 1);
    assert!(d.system_instruction.is_some());
    assert!(d.generation_config.is_some());
    assert!(d.safety_settings.is_some());
    assert!(d.tools.is_some());
    assert!(d.tool_config.is_some());
}

#[test]
fn function_calling_mode_serde_auto() {
    let mode = FunctionCallingMode::Auto;
    let json_str = serde_json::to_string(&mode).unwrap();
    assert_eq!(json_str, "\"AUTO\"");
    let back: FunctionCallingMode = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, FunctionCallingMode::Auto);
}

#[test]
fn function_calling_mode_serde_none() {
    let mode = FunctionCallingMode::None;
    let json_str = serde_json::to_string(&mode).unwrap();
    assert_eq!(json_str, "\"NONE\"");
}

#[test]
fn function_calling_mode_serde_any() {
    let mode = FunctionCallingMode::Any;
    let json_str = serde_json::to_string(&mode).unwrap();
    assert_eq!(json_str, "\"ANY\"");
}

#[test]
fn multiple_candidates_response() {
    let resp = GenerateContentResponse {
        candidates: vec![
            Candidate {
                content: Content::model(vec![Part::text("candidate 1")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            },
            Candidate {
                content: Content::model(vec![Part::text("candidate 2")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            },
        ],
        usage_metadata: None,
        prompt_feedback: None,
    };
    // text() returns first candidate
    assert_eq!(resp.text(), Some("candidate 1"));
    // function_calls() also only looks at first candidate
    assert!(resp.function_calls().is_empty());
}

#[test]
fn candidate_with_mixed_parts_text_and_function_call() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::text("Let me search."),
                Part::function_call("search", json!({"q": "test"})),
            ]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("Let me search."));
    assert_eq!(resp.function_calls().len(), 1);
    assert_eq!(resp.function_calls()[0].0, "search");
}
