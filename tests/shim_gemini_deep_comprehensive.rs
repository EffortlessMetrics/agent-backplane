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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the `abp-shim-gemini` crate.
//!
//! 150+ tests covering: request/response construction, IR ↔ Gemini mapping,
//! streaming, function calling, safety settings, generation config,
//! model handling, usage metadata, error handling, grounding, citations,
//! multi-turn conversations, content parts, and edge cases.

use serde_json::json;
use tokio_stream::StreamExt;

use abp_shim_gemini::{
    Candidate, Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
    GeminiClient, GeminiError, GenerateContentRequest, GenerateContentResponse, GenerationConfig,
    HarmBlockThreshold, HarmCategory, Part, SafetySetting, StreamEvent, ToolConfig,
    ToolDeclaration, UsageMetadata, from_dialect_response, from_dialect_stream_chunk,
    gen_config_from_dialect, to_dialect_request, usage_from_ir, usage_to_ir,
};

use abp_gemini_sdk::dialect::{
    self, CanonicalToolDef, DynamicRetrievalConfig, GeminiCandidate, GeminiCitationMetadata,
    GeminiCitationSource, GeminiConfig, GeminiContent, GeminiFunctionDeclaration,
    GeminiGroundingConfig, GeminiInlineData, GeminiPart, GeminiRequest, GeminiResponse,
    GeminiSafetyRating, GeminiSafetySetting, GeminiStreamChunk, GeminiTool, GeminiToolConfig,
    GeminiUsageMetadata, GoogleSearchRetrieval, HarmProbability,
};
use abp_gemini_sdk::lowering;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEventKind, Capability, SupportLevel, WorkOrderBuilder};

fn is_native(s: &SupportLevel) -> bool {
    matches!(s, SupportLevel::Native)
}
fn is_emulated(s: &SupportLevel) -> bool {
    matches!(s, SupportLevel::Emulated)
}
fn is_unsupported(s: &SupportLevel) -> bool {
    matches!(s, SupportLevel::Unsupported)
}

// =========================================================================
// Helpers
// =========================================================================

fn simple_user_request(text: &str) -> GenerateContentRequest {
    GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text(text)]))
}

fn dialect_text_response(text: &str) -> GeminiResponse {
    GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text(text.into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    }
}

fn dialect_response_with_usage(prompt: u64, candidates: u64) -> GeminiResponse {
    GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("ok".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: prompt,
            candidates_token_count: candidates,
            total_token_count: prompt + candidates,
        }),
    }
}

fn make_stream_chunk(text: &str) -> GeminiStreamChunk {
    GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text(text.into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    }
}

// =========================================================================
// 1. GenerateContentRequest construction
// =========================================================================

#[test]
fn request_new_creates_empty_request() {
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
fn request_accepts_string_model() {
    let req = GenerateContentRequest::new(String::from("gemini-2.5-pro"));
    assert_eq!(req.model, "gemini-2.5-pro");
}

#[test]
fn request_add_content_chaining() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("a")]))
        .add_content(Content::model(vec![Part::text("b")]))
        .add_content(Content::user(vec![Part::text("c")]));
    assert_eq!(req.contents.len(), 3);
}

#[test]
fn request_system_instruction_setter() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be helpful")]));
    assert!(req.system_instruction.is_some());
}

#[test]
fn request_generation_config_setter() {
    let req = GenerateContentRequest::new("gemini-2.5-flash").generation_config(GenerationConfig {
        temperature: Some(0.9),
        ..Default::default()
    });
    assert_eq!(
        req.generation_config.as_ref().unwrap().temperature,
        Some(0.9)
    );
}

#[test]
fn request_safety_settings_setter() {
    let req =
        GenerateContentRequest::new("gemini-2.5-flash").safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]);
    assert_eq!(req.safety_settings.as_ref().unwrap().len(), 1);
}

#[test]
fn request_tools_setter() {
    let req = GenerateContentRequest::new("gemini-2.5-flash").tools(vec![ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        }],
    }]);
    assert!(req.tools.is_some());
}

#[test]
fn request_tool_config_setter() {
    let req = GenerateContentRequest::new("gemini-2.5-flash").tool_config(ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    });
    assert!(req.tool_config.is_some());
}

#[test]
fn request_serde_roundtrip_minimal() {
    let req = simple_user_request("Hello");
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.contents.len(), 1);
}

#[test]
fn request_serde_roundtrip_full() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("test")]))
        .system_instruction(Content::user(vec![Part::text("sys")]))
        .generation_config(GenerationConfig {
            max_output_tokens: Some(100),
            temperature: Some(0.5),
            top_p: Some(0.8),
            top_k: Some(20),
            candidate_count: None,
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "string"})),
        })
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHateSpeech,
            threshold: HarmBlockThreshold::BlockLowAndAbove,
        }]);
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gemini-2.5-pro");
    assert!(back.system_instruction.is_some());
    assert!(back.generation_config.is_some());
    assert!(back.safety_settings.is_some());
}

// =========================================================================
// 2. GenerateContentResponse parsing
// =========================================================================

#[test]
fn response_text_accessor_returns_first_text() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("hello")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("hello"));
}

#[test]
fn response_text_accessor_skips_non_text() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::function_call("fn", json!({})),
                Part::text("after"),
            ]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("after"));
}

#[test]
fn response_text_accessor_returns_none_on_empty() {
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
                Part::function_call("a", json!({"x": 1})),
                Part::text("text"),
                Part::function_call("b", json!({"y": 2})),
            ]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "a");
    assert_eq!(calls[1].0, "b");
}

#[test]
fn response_function_calls_empty_when_no_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert!(resp.function_calls().is_empty());
}

#[test]
fn response_serde_roundtrip() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("hello")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 3,
            total_token_count: 8,
        }),
        prompt_feedback: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text(), Some("hello"));
    assert_eq!(back.usage_metadata.unwrap().total_token_count, 8);
}

#[test]
fn response_with_multiple_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![
            Candidate {
                content: Content::model(vec![Part::text("opt1")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            },
            Candidate {
                content: Content::model(vec![Part::text("opt2")]),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            },
        ],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("opt1"));
    assert_eq!(resp.candidates.len(), 2);
}

// =========================================================================
// 3. Content / Part types and conversions
// =========================================================================

#[test]
fn content_user_helper() {
    let c = Content::user(vec![Part::text("hi")]);
    assert_eq!(c.role, "user");
    assert_eq!(c.parts.len(), 1);
}

#[test]
fn content_model_helper() {
    let c = Content::model(vec![Part::text("world")]);
    assert_eq!(c.role, "model");
}

#[test]
fn part_text_constructor() {
    let p = Part::text("hello");
    assert!(matches!(p, Part::Text(ref s) if s == "hello"));
}

#[test]
fn part_inline_data_constructor() {
    let p = Part::inline_data("image/png", "abc");
    match p {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/png");
            assert_eq!(data, "abc");
        }
        _ => panic!("expected InlineData"),
    }
}

#[test]
fn part_function_call_constructor() {
    let p = Part::function_call("search", json!({"q": "rust"}));
    match p {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, json!({"q": "rust"}));
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn part_function_response_constructor() {
    let p = Part::function_response("search", json!("results"));
    match p {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response, json!("results"));
        }
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn part_serde_text_roundtrip() {
    let p = Part::text("hello");
    let json = serde_json::to_value(&p).unwrap();
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(back, Part::Text("hello".into()));
}

#[test]
fn part_serde_inline_data_roundtrip() {
    let p = Part::inline_data("image/jpeg", "data123");
    let json = serde_json::to_value(&p).unwrap();
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn part_serde_function_call_roundtrip() {
    let p = Part::function_call("fn", json!({"a": 1}));
    let json = serde_json::to_value(&p).unwrap();
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn part_serde_function_response_roundtrip() {
    let p = Part::function_response("fn", json!({"ok": true}));
    let json = serde_json::to_value(&p).unwrap();
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn content_with_multiple_parts() {
    let c = Content::model(vec![
        Part::text("Let me search."),
        Part::function_call("search", json!({})),
    ]);
    assert_eq!(c.parts.len(), 2);
}

#[test]
fn content_serde_roundtrip() {
    let c = Content::user(vec![
        Part::text("hi"),
        Part::inline_data("image/png", "abc"),
    ]);
    let json = serde_json::to_value(&c).unwrap();
    let back: Content = serde_json::from_value(json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.parts.len(), 2);
}

// =========================================================================
// 4. Gemini streaming
// =========================================================================

#[tokio::test]
async fn streaming_produces_events() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = simple_user_request("Stream test");
    let stream = client.generate_stream(request).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    assert!(events.len() >= 2);
}

#[test]
fn stream_event_text_accessor() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("delta")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(event.text(), Some("delta"));
}

#[test]
fn stream_event_text_returns_none_for_no_text() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::function_call("f", json!({}))]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert!(event.text().is_none());
}

#[test]
fn stream_event_text_returns_none_for_empty_candidates() {
    let event = StreamEvent {
        candidates: vec![],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert!(event.text().is_none());
}

#[test]
fn from_dialect_stream_chunk_text() {
    let chunk = make_stream_chunk("hello");
    let event = from_dialect_stream_chunk(&chunk);
    assert_eq!(event.text(), Some("hello"));
}

#[test]
fn from_dialect_stream_chunk_with_usage() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        }),
    };
    let event = from_dialect_stream_chunk(&chunk);
    let usage = event.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 10);
    assert_eq!(usage.candidates_token_count, 20);
    assert_eq!(usage.total_token_count, 30);
}

#[test]
fn from_dialect_stream_chunk_function_call() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "tool".into(),
                    args: json!({"a": 1}),
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
            assert_eq!(name, "tool");
            assert_eq!(args, &json!({"a": 1}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn stream_event_serde_roundtrip() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("chunk")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 1,
            candidates_token_count: 2,
            total_token_count: 3,
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text(), Some("chunk"));
    assert_eq!(back.usage_metadata.unwrap().total_token_count, 3);
}

// =========================================================================
// 5. Request → WorkOrder conversion
// =========================================================================

#[tokio::test]
async fn simple_request_produces_work_order_response() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = simple_user_request("Hello world");
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
    let text = response.text().unwrap();
    assert!(text.contains("Hello world"));
}

#[tokio::test]
async fn request_with_model_override() {
    let client = GeminiClient::new("gemini-2.5-pro");
    let request = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("test model override")]));
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

#[tokio::test]
async fn request_returns_usage_metadata() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = simple_user_request("Count tokens");
    let response = client.generate(request).await.unwrap();
    let usage = response.usage_metadata.unwrap();
    assert!(usage.total_token_count > 0);
    assert_eq!(
        usage.total_token_count,
        usage.prompt_token_count + usage.candidates_token_count
    );
}

#[tokio::test]
async fn empty_task_fallback_text() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::model(vec![Part::text("model-only")]));
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

// =========================================================================
// 6. Receipt → Response conversion
// =========================================================================

#[test]
fn from_dialect_response_text() {
    let resp = dialect_text_response("Hello from Gemini");
    let shim_resp = from_dialect_response(&resp);
    assert_eq!(shim_resp.text(), Some("Hello from Gemini"));
}

#[test]
fn from_dialect_response_with_usage() {
    let resp = dialect_response_with_usage(100, 50);
    let shim_resp = from_dialect_response(&resp);
    let usage = shim_resp.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 100);
    assert_eq!(usage.candidates_token_count, 50);
    assert_eq!(usage.total_token_count, 150);
}

#[test]
fn from_dialect_response_no_usage() {
    let resp = dialect_text_response("text");
    let shim_resp = from_dialect_response(&resp);
    assert!(shim_resp.usage_metadata.is_none());
}

#[test]
fn from_dialect_response_preserves_finish_reason() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("done".into())],
            },
            finish_reason: Some("MAX_TOKENS".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let shim_resp = from_dialect_response(&resp);
    assert_eq!(
        shim_resp.candidates[0].finish_reason.as_deref(),
        Some("MAX_TOKENS")
    );
}

#[test]
fn from_dialect_response_function_call() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "get_weather".into(),
                    args: json!({"location": "NYC"}),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let shim_resp = from_dialect_response(&resp);
    let calls = shim_resp.function_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "get_weather");
}

// =========================================================================
// 7. IR ↔ Gemini message bidirectional mapping
// =========================================================================

#[test]
fn user_text_to_ir_and_back() {
    let req = simple_user_request("Hello");
    let dialect_req = to_dialect_request(&req);
    let ir = lowering::to_ir(
        &dialect_req.contents,
        dialect_req.system_instruction.as_ref(),
    );
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Hello");

    let back = lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn model_text_to_ir_and_back() {
    let content = Content::model(vec![Part::text("Hi there!")]);
    let dialect = GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Hi there!".into())],
    };
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);

    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].role, "model");
    let _ = content; // verify Content::model works
}

#[test]
fn system_instruction_maps_to_ir_system() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("You are helpful")]))
        .add_content(Content::user(vec![Part::text("Hi")]));
    let d = to_dialect_request(&req);
    let ir = lowering::to_ir(&d.contents, d.system_instruction.as_ref());
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[0].text_content(), "You are helpful");
}

#[test]
fn system_messages_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hello"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn extract_system_instruction_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = lowering::extract_system_instruction(&conv).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be brief"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn extract_system_instruction_returns_none_when_absent() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
    assert!(lowering::extract_system_instruction(&conv).is_none());
}

#[test]
fn function_call_ir_roundtrip() {
    let content = Content::model(vec![Part::function_call("search", json!({"q": "rust"}))]);
    let dialect = GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"q": "rust"}),
        }],
    };
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, id } => {
            assert_eq!(name, "search");
            assert_eq!(input, &json!({"q": "rust"}));
            assert_eq!(id, "gemini_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }

    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
    let _ = content;
}

#[test]
fn function_response_ir_roundtrip() {
    let dialect = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results here"),
        }],
    };
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

    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response, &json!("results here"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn inline_data_ir_roundtrip() {
    let dialect = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "abc123".into(),
        })],
    };
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }

    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/png");
            assert_eq!(d.data, "abc123");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn multi_part_message_ir_roundtrip() {
    let dialect = GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::Text("Let me search.".into()),
            GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({}),
            },
        ],
    };
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].content.len(), 2);

    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].parts.len(), 2);
}

#[test]
fn empty_conversation_ir_roundtrip() {
    let ir = lowering::to_ir(&[], None);
    assert!(ir.is_empty());
    let back = lowering::from_ir(&ir);
    assert!(back.is_empty());
}

#[test]
fn function_response_object_payload_ir() {
    let dialect = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "api".into(),
            response: json!({"status": 200, "body": "ok"}),
        }],
    };
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            let text = match &content[0] {
                IrContentBlock::Text { text } => text.as_str(),
                _ => panic!("expected text"),
            };
            assert!(text.contains("200"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// =========================================================================
// 8. Gemini-specific capabilities (grounding, code execution)
// =========================================================================

#[test]
fn capability_manifest_has_streaming() {
    let manifest = dialect::capability_manifest();
    assert!(is_native(manifest.get(&Capability::Streaming).unwrap()));
}

#[test]
fn capability_manifest_has_tool_read() {
    let manifest = dialect::capability_manifest();
    assert!(is_native(manifest.get(&Capability::ToolRead).unwrap()));
}

#[test]
fn capability_manifest_tool_write_emulated() {
    let manifest = dialect::capability_manifest();
    assert!(is_emulated(manifest.get(&Capability::ToolWrite).unwrap()));
}

#[test]
fn capability_manifest_structured_output_native() {
    let manifest = dialect::capability_manifest();
    assert!(is_native(
        manifest
            .get(&Capability::StructuredOutputJsonSchema)
            .unwrap()
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let manifest = dialect::capability_manifest();
    assert!(is_unsupported(
        manifest.get(&Capability::McpClient).unwrap()
    ));
    assert!(is_unsupported(
        manifest.get(&Capability::McpServer).unwrap()
    ));
}

#[test]
fn grounding_config_serde_roundtrip() {
    let cfg = GeminiGroundingConfig {
        google_search_retrieval: Some(GoogleSearchRetrieval {
            dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                mode: "MODE_DYNAMIC".into(),
                dynamic_threshold: Some(0.3),
            }),
        }),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cfg);
}

#[test]
fn grounding_config_no_retrieval() {
    let cfg = GeminiGroundingConfig {
        google_search_retrieval: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert!(back.google_search_retrieval.is_none());
}

#[test]
fn grounding_config_no_threshold() {
    let cfg = GeminiGroundingConfig {
        google_search_retrieval: Some(GoogleSearchRetrieval {
            dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                mode: "MODE_UNSPECIFIED".into(),
                dynamic_threshold: None,
            }),
        }),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    let drc = back
        .google_search_retrieval
        .unwrap()
        .dynamic_retrieval_config
        .unwrap();
    assert!(drc.dynamic_threshold.is_none());
    assert_eq!(drc.mode, "MODE_UNSPECIFIED");
}

// =========================================================================
// 9. Safety settings and harm categories
// =========================================================================

#[test]
fn all_harm_categories_serialize() {
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
fn all_harm_block_thresholds_serialize() {
    let thresholds = [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for t in &thresholds {
        let json = serde_json::to_string(t).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, t);
    }
}

#[test]
fn safety_setting_serde_roundtrip() {
    let setting = SafetySetting {
        category: HarmCategory::HarmCategoryDangerousContent,
        threshold: HarmBlockThreshold::BlockOnlyHigh,
    };
    let json = serde_json::to_string(&setting).unwrap();
    let back: SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, setting);
}

#[test]
fn safety_settings_to_dialect() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![
            SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryHateSpeech,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            },
        ]);
    let d = to_dialect_request(&req);
    let ss = d.safety_settings.unwrap();
    assert_eq!(ss.len(), 2);
    assert_eq!(ss[0].category, HarmCategory::HarmCategoryHarassment);
    assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockNone);
}

#[test]
fn harm_probability_serde_roundtrip() {
    let probs = [
        HarmProbability::Negligible,
        HarmProbability::Low,
        HarmProbability::Medium,
        HarmProbability::High,
    ];
    for p in &probs {
        let json = serde_json::to_string(p).unwrap();
        let back: HarmProbability = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, p);
    }
}

#[test]
fn safety_rating_serde_roundtrip() {
    let rating = GeminiSafetyRating {
        category: HarmCategory::HarmCategorySexuallyExplicit,
        probability: HarmProbability::Low,
    };
    let json = serde_json::to_string(&rating).unwrap();
    let back: GeminiSafetyRating = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rating);
}

#[test]
fn dialect_safety_setting_serde() {
    let ds = GeminiSafetySetting {
        category: HarmCategory::HarmCategoryCivicIntegrity,
        threshold: HarmBlockThreshold::BlockLowAndAbove,
    };
    let json = serde_json::to_string(&ds).unwrap();
    let back: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ds);
}

// =========================================================================
// 10. Tool / function declaration and calling
// =========================================================================

#[test]
fn tool_declaration_serde_roundtrip() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "get_weather".into(),
            description: "Get weather".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        }],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn tool_declaration_to_dialect() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "calc".into(),
            description: "Calculate".into(),
            parameters: json!({"type": "object"}),
        }],
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("calc 1+1")]))
        .tools(vec![tool]);
    let d = to_dialect_request(&req);
    let tools = d.tools.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_declarations[0].name, "calc");
}

#[test]
fn multiple_function_declarations() {
    let tool = ToolDeclaration {
        function_declarations: vec![
            FunctionDeclaration {
                name: "fn_a".into(),
                description: "Function A".into(),
                parameters: json!({}),
            },
            FunctionDeclaration {
                name: "fn_b".into(),
                description: "Function B".into(),
                parameters: json!({}),
            },
        ],
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tools(vec![tool]);
    let d = to_dialect_request(&req);
    assert_eq!(d.tools.unwrap()[0].function_declarations.len(), 2);
}

#[test]
fn canonical_tool_def_to_gemini_roundtrip() {
    let def = CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let gemini = dialect::tool_def_to_gemini(&def);
    assert_eq!(gemini.name, "search");
    assert_eq!(gemini.description, "Search the web");
    assert_eq!(gemini.parameters, def.parameters_schema);

    let back = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(back, def);
}

#[test]
fn function_calling_mode_auto_serde() {
    let mode = FunctionCallingMode::Auto;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"AUTO\"");
    let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, FunctionCallingMode::Auto);
}

#[test]
fn function_calling_mode_any_serde() {
    let mode = FunctionCallingMode::Any;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"ANY\"");
}

#[test]
fn function_calling_mode_none_serde() {
    let mode = FunctionCallingMode::None;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"NONE\"");
}

#[test]
fn tool_config_to_dialect() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["fn_a".into(), "fn_b".into()]),
        },
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(tc);
    let d = to_dialect_request(&req);
    let dtc = d.tool_config.unwrap();
    assert_eq!(
        dtc.function_calling_config.mode,
        dialect::FunctionCallingMode::Any
    );
    assert_eq!(
        dtc.function_calling_config.allowed_function_names,
        Some(vec!["fn_a".into(), "fn_b".into()])
    );
}

#[test]
fn tool_config_serde_roundtrip() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn gemini_tool_serde_roundtrip() {
    let tool = GeminiTool {
        function_declarations: vec![GeminiFunctionDeclaration {
            name: "test".into(),
            description: "A test function".into(),
            parameters: json!({"type": "object"}),
        }],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

// =========================================================================
// 11. System instruction handling
// =========================================================================

#[test]
fn system_instruction_preserved_in_dialect() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Respond in JSON")]))
        .add_content(Content::user(vec![Part::text("List colors")]));
    let d = to_dialect_request(&req);
    assert!(d.system_instruction.is_some());
    match &d.system_instruction.unwrap().parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Respond in JSON"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn system_instruction_multipart() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![
            Part::text("Rule 1: be brief. "),
            Part::text("Rule 2: be accurate."),
        ]))
        .add_content(Content::user(vec![Part::text("hi")]));
    let d = to_dialect_request(&req);
    let sys = d.system_instruction.unwrap();
    assert_eq!(sys.parts.len(), 2);
}

#[test]
fn empty_system_instruction_not_added_to_ir() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hi".into())],
    }];
    let ir = lowering::to_ir(&contents, Some(&sys));
    assert_eq!(ir.len(), 1);
}

#[test]
fn system_instruction_with_non_text_parts_ignored_in_ir() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "abc".into(),
        })],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hi".into())],
    }];
    let ir = lowering::to_ir(&contents, Some(&sys));
    // non-text system parts produce empty text → no system message
    assert_eq!(ir.len(), 1);
}

// =========================================================================
// 12. Multi-turn conversation tracking
// =========================================================================

#[test]
fn three_turn_conversation_to_ir() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Q1")]))
        .add_content(Content::model(vec![Part::text("A1")]))
        .add_content(Content::user(vec![Part::text("Q2")]));
    let d = to_dialect_request(&req);
    let ir = lowering::to_ir(&d.contents, d.system_instruction.as_ref());
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[2].role, IrRole::User);
}

#[test]
fn five_turn_conversation_roundtrip() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Q1")]))
        .add_content(Content::model(vec![Part::text("A1")]))
        .add_content(Content::user(vec![Part::text("Q2")]))
        .add_content(Content::model(vec![Part::text("A2")]))
        .add_content(Content::user(vec![Part::text("Q3")]));
    let d = to_dialect_request(&req);
    let ir = lowering::to_ir(&d.contents, d.system_instruction.as_ref());
    assert_eq!(ir.len(), 5);

    let back = lowering::from_ir(&ir);
    assert_eq!(back.len(), 5);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "model");
    assert_eq!(back[4].role, "user");
}

#[test]
fn conversation_with_tool_call_and_response() {
    let contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Search for rust".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
            }],
        },
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("results"),
            }],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Here are the results.".into())],
        },
    ];
    let ir = lowering::to_ir(&contents, None);
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
fn conversation_with_system_and_tool_use() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("You are a search assistant.".into())],
    };
    let contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Find info".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "web_search".into(),
                args: json!({"q": "ABP"}),
            }],
        },
    ];
    let ir = lowering::to_ir(&contents, Some(&sys));
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[1].role, IrRole::User);
    assert_eq!(ir.messages[2].role, IrRole::Assistant);
}

#[test]
fn ir_messages_by_role() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Q1"),
        IrMessage::text(IrRole::Assistant, "A1"),
        IrMessage::text(IrRole::User, "Q2"),
        IrMessage::text(IrRole::Assistant, "A2"),
    ]);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
}

#[test]
fn ir_last_assistant() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Q1"),
        IrMessage::text(IrRole::Assistant, "A1"),
        IrMessage::text(IrRole::User, "Q2"),
        IrMessage::text(IrRole::Assistant, "A2"),
    ]);
    assert_eq!(conv.last_assistant().unwrap().text_content(), "A2");
}

// =========================================================================
// 13. Error handling
// =========================================================================

#[test]
fn gemini_error_request_conversion_display() {
    let err = GeminiError::RequestConversion("bad input".into());
    assert!(err.to_string().contains("bad input"));
    assert!(err.to_string().contains("request conversion"));
}

#[test]
fn gemini_error_response_conversion_display() {
    let err = GeminiError::ResponseConversion("missing field".into());
    assert!(err.to_string().contains("missing field"));
    assert!(err.to_string().contains("response conversion"));
}

#[test]
fn gemini_error_backend_display() {
    let err = GeminiError::BackendError("quota exceeded".into());
    assert!(err.to_string().contains("quota exceeded"));
    assert!(err.to_string().contains("backend error"));
}

#[test]
fn gemini_error_serde_display() {
    let err: GeminiError = serde_json::from_str::<Part>("not json").unwrap_err().into();
    assert!(err.to_string().contains("serde error"));
}

#[test]
fn gemini_error_debug_impl() {
    let err = GeminiError::BackendError("auth failed".into());
    let debug = format!("{err:?}");
    assert!(debug.contains("BackendError"));
}

#[test]
fn gemini_error_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<GeminiError>();
    assert_sync::<GeminiError>();
}

// =========================================================================
// 14. Model configuration and generation config
// =========================================================================

#[test]
fn generation_config_default() {
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
fn generation_config_all_fields_roundtrip() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(2048),
        temperature: Some(0.7),
        top_p: Some(0.95),
        top_k: Some(40),
        candidate_count: None,
        stop_sequences: Some(vec!["DONE".into(), "END".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object", "properties": {"x": {"type": "number"}}})),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_output_tokens, Some(2048));
    assert_eq!(back.temperature, Some(0.7));
    assert_eq!(back.top_p, Some(0.95));
    assert_eq!(back.top_k, Some(40));
    assert_eq!(back.stop_sequences, Some(vec!["DONE".into(), "END".into()]));
    assert_eq!(
        back.response_mime_type,
        Some("application/json".to_string())
    );
}

#[test]
fn gen_config_dialect_roundtrip() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(512),
        temperature: Some(1.0),
        top_p: Some(0.8),
        top_k: Some(10),
        candidate_count: None,
        stop_sequences: Some(vec!["STOP".into()]),
        response_mime_type: Some("text/plain".into()),
        response_schema: None,
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(cfg.clone());
    let d = to_dialect_request(&req);
    let dcfg = d.generation_config.unwrap();
    assert_eq!(dcfg.max_output_tokens, Some(512));
    assert_eq!(dcfg.temperature, Some(1.0));

    let back = gen_config_from_dialect(&dcfg);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.stop_sequences, cfg.stop_sequences);
}

#[test]
fn gemini_config_default() {
    let cfg = GeminiConfig::default();
    assert!(cfg.base_url.contains("googleapis.com"));
    assert_eq!(cfg.model, "gemini-2.5-flash");
    assert!(cfg.max_output_tokens.unwrap_or(0) > 0);
    assert!(cfg.api_key.is_empty());
    assert!(cfg.temperature.is_none());
}

#[test]
fn model_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-2.5-flash");
    assert_eq!(canonical, "google/gemini-2.5-flash");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gemini-2.5-flash");
}

#[test]
fn model_canonical_without_prefix() {
    let back = dialect::from_canonical_model("gemini-2.5-pro");
    assert_eq!(back, "gemini-2.5-pro");
}

#[test]
fn is_known_model_true() {
    assert!(dialect::is_known_model("gemini-2.5-flash"));
    assert!(dialect::is_known_model("gemini-2.5-pro"));
    assert!(dialect::is_known_model("gemini-2.0-flash"));
    assert!(dialect::is_known_model("gemini-1.5-flash"));
    assert!(dialect::is_known_model("gemini-1.5-pro"));
}

#[test]
fn is_known_model_false() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("claude-3-opus"));
    assert!(!dialect::is_known_model("unknown-model"));
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
// 15. Candidate content handling
// =========================================================================

#[test]
fn candidate_with_text_content() {
    let candidate = Candidate {
        content: Content::model(vec![Part::text("Generated text")]),
        finish_reason: Some("STOP".into()),
        safety_ratings: None,
    };
    assert_eq!(candidate.finish_reason.as_deref(), Some("STOP"));
    match &candidate.content.parts[0] {
        Part::Text(t) => assert_eq!(t, "Generated text"),
        _ => panic!("expected text"),
    }
}

#[test]
fn candidate_with_no_finish_reason() {
    let candidate = Candidate {
        content: Content::model(vec![Part::text("partial")]),
        finish_reason: None,
        safety_ratings: None,
    };
    assert!(candidate.finish_reason.is_none());
}

#[test]
fn candidate_with_function_call_content() {
    let candidate = Candidate {
        content: Content::model(vec![Part::function_call("get_time", json!({"tz": "UTC"}))]),
        finish_reason: Some("STOP".into()),
        safety_ratings: None,
    };
    match &candidate.content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "get_time");
            assert_eq!(args, &json!({"tz": "UTC"}));
        }
        _ => panic!("expected function call"),
    }
}

#[test]
fn candidate_with_mixed_content() {
    let candidate = Candidate {
        content: Content::model(vec![
            Part::text("Let me help."),
            Part::function_call("assist", json!({})),
            Part::text("Done."),
        ]),
        finish_reason: Some("STOP".into()),
        safety_ratings: None,
    };
    assert_eq!(candidate.content.parts.len(), 3);
}

#[test]
fn dialect_candidate_with_safety_ratings() {
    let candidate = GeminiCandidate {
        content: GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("safe text".into())],
        },
        finish_reason: Some("STOP".into()),
        safety_ratings: Some(vec![
            GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            },
            GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHateSpeech,
                probability: HarmProbability::Low,
            },
        ]),
        citation_metadata: None,
    };
    let ratings = candidate.safety_ratings.unwrap();
    assert_eq!(ratings.len(), 2);
    assert_eq!(ratings[0].probability, HarmProbability::Negligible);
}

#[test]
fn dialect_candidate_with_citation_metadata() {
    let candidate = GeminiCandidate {
        content: GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("cited text".into())],
        },
        finish_reason: Some("STOP".into()),
        safety_ratings: None,
        citation_metadata: Some(GeminiCitationMetadata {
            citation_sources: vec![GeminiCitationSource {
                start_index: Some(0),
                end_index: Some(10),
                uri: Some("https://example.com".into()),
                license: Some("MIT".into()),
            }],
        }),
    };
    let cm = candidate.citation_metadata.unwrap();
    assert_eq!(cm.citation_sources.len(), 1);
    assert_eq!(
        cm.citation_sources[0].uri.as_deref(),
        Some("https://example.com")
    );
}

// =========================================================================
// 16. Usage metadata
// =========================================================================

#[test]
fn usage_metadata_serde_roundtrip() {
    let usage = UsageMetadata {
        prompt_token_count: 42,
        candidates_token_count: 18,
        total_token_count: 60,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

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
        prompt_token_count: 50,
        candidates_token_count: 25,
        total_token_count: 75,
    };
    let ir = usage_to_ir(&original);
    let back = usage_from_ir(&ir);
    assert_eq!(back.prompt_token_count, original.prompt_token_count);
    assert_eq!(back.candidates_token_count, original.candidates_token_count);
    assert_eq!(back.total_token_count, original.total_token_count);
}

#[test]
fn usage_zero_values() {
    let usage = UsageMetadata {
        prompt_token_count: 0,
        candidates_token_count: 0,
        total_token_count: 0,
    };
    let ir = usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 0);
    assert_eq!(ir.output_tokens, 0);
    assert_eq!(ir.total_tokens, 0);
}

#[test]
fn dialect_usage_metadata_serde() {
    let usage = GeminiUsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 20,
        total_token_count: 30,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: GeminiUsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.prompt_token_count, 10);
    assert_eq!(back.total_token_count, 30);
}

// =========================================================================
// 17. to_dialect_request full field preservation
// =========================================================================

#[test]
fn to_dialect_request_preserves_model() {
    let req = GenerateContentRequest::new("gemini-custom-model")
        .add_content(Content::user(vec![Part::text("hi")]));
    let d = to_dialect_request(&req);
    assert_eq!(d.model, "gemini-custom-model");
}

#[test]
fn to_dialect_request_preserves_contents() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("msg1")]))
        .add_content(Content::model(vec![Part::text("msg2")]));
    let d = to_dialect_request(&req);
    assert_eq!(d.contents.len(), 2);
    assert_eq!(d.contents[0].role, "user");
    assert_eq!(d.contents[1].role, "model");
}

#[test]
fn to_dialect_request_no_optional_fields() {
    let req = simple_user_request("hi");
    let d = to_dialect_request(&req);
    assert!(d.system_instruction.is_none());
    assert!(d.generation_config.is_none());
    assert!(d.safety_settings.is_none());
    assert!(d.tools.is_none());
    assert!(d.tool_config.is_none());
}

#[test]
fn to_dialect_request_preserves_all_optional_fields() {
    let req = GenerateContentRequest::new("model-x")
        .add_content(Content::user(vec![Part::text("hi")]))
        .system_instruction(Content::user(vec![Part::text("Be helpful")]))
        .generation_config(GenerationConfig {
            temperature: Some(1.0),
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
    assert!(d.system_instruction.is_some());
    assert!(d.generation_config.is_some());
    assert!(d.safety_settings.is_some());
    assert!(d.tools.is_some());
    assert!(d.tool_config.is_some());
}

// =========================================================================
// 18. from_dialect_response
// =========================================================================

#[test]
fn from_dialect_response_multiple_candidates() {
    let resp = GeminiResponse {
        candidates: vec![
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("opt1".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("opt2".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
        ],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let shim_resp = from_dialect_response(&resp);
    assert_eq!(shim_resp.candidates.len(), 2);
}

#[test]
fn from_dialect_response_inline_data() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/gif".into(),
                    data: "R0lG".into(),
                })],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let shim_resp = from_dialect_response(&resp);
    match &shim_resp.candidates[0].content.parts[0] {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/gif");
            assert_eq!(data, "R0lG");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

// =========================================================================
// 19. map_work_order (dialect)
// =========================================================================

#[test]
fn map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Write tests").build();
    let cfg = GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.contents.len(), 1);
    assert_eq!(req.contents[0].role, "user");
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("Write tests")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn map_work_order_uses_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig {
        model: "gemini-2.0-flash".into(),
        ..GeminiConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.0-flash");
}

#[test]
fn map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("gemini-2.5-pro")
        .build();
    let cfg = GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-pro");
}

#[test]
fn map_work_order_generation_config_from_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig {
        max_output_tokens: Some(1024),
        temperature: Some(0.5),
        ..GeminiConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    let gc = req.generation_config.unwrap();
    assert_eq!(gc.max_output_tokens, Some(1024));
    assert_eq!(gc.temperature, Some(0.5));
}

#[test]
fn map_work_order_no_generation_config_when_defaults() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig {
        max_output_tokens: None,
        temperature: None,
        ..GeminiConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.generation_config.is_none());
}

// =========================================================================
// 20. map_response / map_stream_chunk (dialect)
// =========================================================================

#[test]
fn map_response_text() {
    let resp = dialect_text_response("Hello");
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn map_response_function_call() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "rust"}),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "search"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_function_response() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "search".into(),
                    response: json!("results"),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult { tool_name, .. } => assert_eq!(tool_name, "search"),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_response_inline_data_ignored() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/png".into(),
                    data: "abc".into(),
                })],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_stream_chunk_text_delta() {
    let chunk = make_stream_chunk("delta text");
    let events = dialect::map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "delta text"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn map_stream_chunk_function_call() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "tool".into(),
                    args: json!({}),
                }],
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
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "tool"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_stream_event_alias() {
    let chunk = make_stream_chunk("via alias");
    let events_a = dialect::map_stream_chunk(&chunk);
    let events_b = dialect::map_stream_event(&chunk);
    assert_eq!(events_a.len(), events_b.len());
}

#[test]
fn map_response_multiple_parts() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![
                    GeminiPart::Text("text part".into()),
                    GeminiPart::FunctionCall {
                        name: "fn".into(),
                        args: json!({}),
                    },
                ],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        prompt_feedback: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
}

// =========================================================================
// 21. Citation metadata
// =========================================================================

#[test]
fn citation_metadata_serde_roundtrip() {
    let cm = GeminiCitationMetadata {
        citation_sources: vec![
            GeminiCitationSource {
                start_index: Some(0),
                end_index: Some(50),
                uri: Some("https://example.com/doc".into()),
                license: None,
            },
            GeminiCitationSource {
                start_index: Some(60),
                end_index: Some(100),
                uri: None,
                license: Some("Apache-2.0".into()),
            },
        ],
    };
    let json = serde_json::to_string(&cm).unwrap();
    let back: GeminiCitationMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cm);
}

#[test]
fn citation_source_all_none_fields() {
    let src = GeminiCitationSource {
        start_index: None,
        end_index: None,
        uri: None,
        license: None,
    };
    let json = serde_json::to_string(&src).unwrap();
    let back: GeminiCitationSource = serde_json::from_str(&json).unwrap();
    assert_eq!(back, src);
}

// =========================================================================
// 22. GeminiRequest / GeminiResponse dialect serde
// =========================================================================

#[test]
fn gemini_request_serde_roundtrip() {
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
    let back: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gemini-2.5-flash");
}

#[test]
fn gemini_response_serde_roundtrip() {
    let resp = dialect_text_response("world");
    let json = serde_json::to_string(&resp).unwrap();
    let back: GeminiResponse = serde_json::from_str(&json).unwrap();
    match &back.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "world"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_stream_chunk_serde_roundtrip() {
    let chunk = make_stream_chunk("streaming");
    let json = serde_json::to_string(&chunk).unwrap();
    let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    match &back.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "streaming"),
        other => panic!("expected Text, got {other:?}"),
    }
}

// =========================================================================
// 23. GeminiClient
// =========================================================================

#[test]
fn client_model_accessor() {
    let client = GeminiClient::new("gemini-2.5-pro");
    assert_eq!(client.model(), "gemini-2.5-pro");
}

#[test]
fn client_clone_preserves_model() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let cloned = client.clone();
    assert_eq!(cloned.model(), client.model());
}

#[test]
fn client_debug_contains_model() {
    let client = GeminiClient::new("gemini-2.0-flash");
    let debug = format!("{client:?}");
    assert!(debug.contains("gemini-2.0-flash"));
}

#[tokio::test]
async fn client_generate_with_system_instruction() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be concise")]))
        .add_content(Content::user(vec![Part::text("Hello")]));
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

#[tokio::test]
async fn client_generate_multi_turn() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hi")]))
        .add_content(Content::model(vec![Part::text("Hello!")]))
        .add_content(Content::user(vec![Part::text("How are you?")]));
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

#[tokio::test]
async fn client_stream_multi_turn() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Start")]))
        .add_content(Content::model(vec![Part::text("Ok")]))
        .add_content(Content::user(vec![Part::text("Continue")]));
    let stream = client.generate_stream(request).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    assert!(!events.is_empty());
}

// =========================================================================
// 24. GeminiToolConfig dialect types
// =========================================================================

#[test]
fn gemini_tool_config_serde_roundtrip() {
    let tc = GeminiToolConfig {
        function_calling_config: abp_gemini_sdk::dialect::GeminiFunctionCallingConfig {
            mode: dialect::FunctionCallingMode::Auto,
            allowed_function_names: Some(vec!["fn_a".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn gemini_function_calling_config_no_allowed() {
    let cfg = abp_gemini_sdk::dialect::GeminiFunctionCallingConfig {
        mode: dialect::FunctionCallingMode::None,
        allowed_function_names: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("allowedFunctionNames"));
}

// =========================================================================
// 25. Edge cases and boundary tests
// =========================================================================

#[test]
fn empty_text_part() {
    let p = Part::text("");
    match p {
        Part::Text(t) => assert!(t.is_empty()),
        _ => panic!("expected Text"),
    }
}

#[test]
fn very_long_text_part() {
    let long_text = "a".repeat(100_000);
    let p = Part::text(&long_text);
    match p {
        Part::Text(t) => assert_eq!(t.len(), 100_000),
        _ => panic!("expected Text"),
    }
}

#[test]
fn unicode_text_roundtrip() {
    let text = "Hello 世界! 🌍 مرحبا";
    let req = simple_user_request(text);
    let d = to_dialect_request(&req);
    let ir = lowering::to_ir(&d.contents, d.system_instruction.as_ref());
    assert_eq!(ir.messages[0].text_content(), text);
}

#[test]
fn empty_function_args() {
    let p = Part::function_call("fn", json!({}));
    match p {
        Part::FunctionCall { args, .. } => assert_eq!(args, json!({})),
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn null_function_response() {
    let p = Part::function_response("fn", serde_json::Value::Null);
    match p {
        Part::FunctionResponse { response, .. } => assert!(response.is_null()),
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn nested_json_function_args() {
    let args = json!({
        "query": {
            "filters": [{"field": "status", "op": "eq", "value": "active"}],
            "limit": 10
        }
    });
    let p = Part::function_call("complex_search", args.clone());
    match p {
        Part::FunctionCall {
            args: actual_args, ..
        } => assert_eq!(actual_args, args),
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn content_empty_parts() {
    let c = Content::user(vec![]);
    assert!(c.parts.is_empty());
}

#[test]
fn ir_conversation_push_chaining() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q1"))
        .push(IrMessage::text(IrRole::Assistant, "a1"));
    assert_eq!(conv.len(), 2);
    assert!(!conv.is_empty());
}

#[test]
fn ir_message_is_text_only_true() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert!(msg.is_text_only());
}

#[test]
fn ir_message_is_text_only_false() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "id".into(),
            name: "fn".into(),
            input: json!({}),
        }],
    );
    assert!(!msg.is_text_only());
}

#[test]
fn ir_conversation_tool_calls() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "q"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "id1".into(),
                name: "fn1".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(conv.tool_calls().len(), 1);
}

#[test]
fn generation_config_skip_serializing_nones() {
    let cfg = GenerationConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("maxOutputTokens"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("topP"));
}

#[test]
fn response_no_finish_reason_deserializes() {
    let json = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hi"}]}}]}"#;
    let resp: GenerateContentResponse = serde_json::from_str(json).unwrap();
    assert!(resp.candidates[0].finish_reason.is_none());
}

#[test]
fn response_no_usage_metadata_deserializes() {
    let json = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hi"}]}}]}"#;
    let resp: GenerateContentResponse = serde_json::from_str(json).unwrap();
    assert!(resp.usage_metadata.is_none());
}

#[test]
fn gemini_inline_data_equality() {
    let a = GeminiInlineData {
        mime_type: "image/png".into(),
        data: "abc".into(),
    };
    let b = GeminiInlineData {
        mime_type: "image/png".into(),
        data: "abc".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn gemini_inline_data_inequality() {
    let a = GeminiInlineData {
        mime_type: "image/png".into(),
        data: "abc".into(),
    };
    let b = GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "abc".into(),
    };
    assert_ne!(a, b);
}
