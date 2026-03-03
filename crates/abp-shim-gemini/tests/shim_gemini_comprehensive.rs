// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for the Gemini shim crate.
//!
//! Tests cover: type fidelity, request/response translation,
//! streaming, tool use, and edge cases.

use abp_core::ir::{IrContentBlock, IrRole};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use abp_gemini_sdk::dialect::{
    self, GeminiCandidate, GeminiContent, GeminiInlineData, GeminiPart, GeminiResponse,
    GeminiStreamChunk, GeminiUsageMetadata, HarmBlockThreshold, HarmCategory,
};
use abp_gemini_sdk::lowering;
use abp_shim_gemini::{
    from_dialect_response, from_dialect_stream_chunk, gen_config_from_dialect, to_dialect_request,
    usage_from_ir, usage_to_ir, Candidate, Content, FunctionCallingConfig, FunctionCallingMode,
    FunctionDeclaration, GenerateContentRequest, GenerateContentResponse, GenerationConfig,
    GeminiClient, GeminiError, Part, SafetySetting, StreamEvent, ToolConfig, ToolDeclaration,
    UsageMetadata,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// =========================================================================
// 1. Gemini types fidelity (~15 tests)
// =========================================================================

#[test]
fn part_text_serde_roundtrip() {
    let part = Part::text("Hello, world!");
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Part::Text("Hello, world!".into()));
}

#[test]
fn part_inline_data_serde_roundtrip() {
    let part = Part::inline_data("image/png", "iVBORw0KGgo=");
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_function_call_serde_roundtrip() {
    let part = Part::function_call("get_weather", json!({"location": "London"}));
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_function_response_serde_roundtrip() {
    let part = Part::function_response("get_weather", json!({"temp": 22}));
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn content_user_role() {
    let c = Content::user(vec![Part::text("Hello")]);
    assert_eq!(c.role, "user");
    assert_eq!(c.parts.len(), 1);
}

#[test]
fn content_model_role() {
    let c = Content::model(vec![Part::text("Hi!")]);
    assert_eq!(c.role, "model");
}

#[test]
fn content_serde_roundtrip() {
    let c = Content::user(vec![Part::text("test"), Part::inline_data("image/jpeg", "abc")]);
    let json = serde_json::to_string(&c).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.parts.len(), 2);
}

#[test]
fn safety_setting_serde_roundtrip() {
    let ss = SafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockNone,
    };
    let json = serde_json::to_string(&ss).unwrap();
    let back: SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ss);
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
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_output_tokens, Some(2048));
    assert_eq!(back.temperature, Some(0.8));
    assert_eq!(back.top_p, Some(0.95));
    assert_eq!(back.top_k, Some(40));
    assert_eq!(back.stop_sequences, Some(vec!["END".into(), "STOP".into()]));
}

#[test]
fn generation_config_default_has_all_none() {
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
fn usage_metadata_serde_roundtrip() {
    let usage = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn harm_category_all_variants_serialize() {
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
fn harm_block_threshold_all_variants_serialize() {
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
fn generate_content_request_serde_roundtrip() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.5),
            ..Default::default()
        });
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gemini-2.5-flash");
    assert_eq!(back.contents.len(), 1);
    assert_eq!(back.generation_config.unwrap().temperature, Some(0.5));
}

// =========================================================================
// 2. Request translation (~15 tests)
// =========================================================================

#[test]
fn request_simple_text_to_work_order() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Explain Rust traits")]));
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert_eq!(wo.task, "Explain Rust traits");
    assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-flash"));
}

#[test]
fn request_model_preserved_in_work_order() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("test")]));
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-pro"));
}

#[test]
fn request_system_instruction_becomes_context_snippet() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be concise and helpful.")]))
        .add_content(Content::user(vec![Part::text("Hello")]));
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert!(!wo.context.snippets.is_empty());
    assert_eq!(wo.context.snippets[0].content, "Be concise and helpful.");
}

#[test]
fn request_tools_preserved_in_vendor_config() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Search")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "search".into(),
                description: "Web search".into(),
                parameters: json!({"type": "object"}),
            }],
        }]);
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert!(wo.config.vendor.contains_key("tools"));
}

#[test]
fn request_generation_config_preserved_in_vendor() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.9),
            max_output_tokens: Some(4096),
            ..Default::default()
        });
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert!(wo.config.vendor.contains_key("generation_config"));
}

#[test]
fn request_safety_settings_preserved_in_vendor() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }]);
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert!(wo.config.vendor.contains_key("safety_settings"));
}

#[test]
fn request_tool_config_preserved_in_vendor() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert!(wo.config.vendor.contains_key("tool_config"));
}

#[test]
fn request_multi_turn_extracts_last_user_text() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("First question")]))
        .add_content(Content::model(vec![Part::text("Answer")]))
        .add_content(Content::user(vec![Part::text("Follow-up")]));
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert_eq!(wo.task, "Follow-up");
}

#[test]
fn request_empty_contents_produces_empty_task() {
    let req = GenerateContentRequest::new("gemini-2.5-flash");
    let dialect_req = to_dialect_request(&req);
    let wo: abp_core::WorkOrder = dialect_req.into();
    assert!(wo.task.is_empty());
}

#[test]
fn to_dialect_preserves_contents_order() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("A")]))
        .add_content(Content::model(vec![Part::text("B")]))
        .add_content(Content::user(vec![Part::text("C")]));
    let dialect = to_dialect_request(&req);
    assert_eq!(dialect.contents.len(), 3);
    assert_eq!(dialect.contents[0].role, "user");
    assert_eq!(dialect.contents[1].role, "model");
    assert_eq!(dialect.contents[2].role, "user");
}

#[test]
fn to_dialect_preserves_system_instruction() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("System prompt")]))
        .add_content(Content::user(vec![Part::text("Hello")]));
    let dialect = to_dialect_request(&req);
    let sys = dialect.system_instruction.unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "System prompt"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn to_dialect_preserves_generation_config() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(GenerationConfig {
            temperature: Some(1.0),
            top_p: Some(0.9),
            ..Default::default()
        });
    let dialect = to_dialect_request(&req);
    let cfg = dialect.generation_config.unwrap();
    assert_eq!(cfg.temperature, Some(1.0));
    assert_eq!(cfg.top_p, Some(0.9));
}

#[test]
fn to_dialect_preserves_tools() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tools(vec![ToolDeclaration {
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
        }]);
    let dialect = to_dialect_request(&req);
    let tools = dialect.tools.unwrap();
    assert_eq!(tools[0].function_declarations.len(), 2);
    assert_eq!(tools[0].function_declarations[0].name, "fn_a");
    assert_eq!(tools[0].function_declarations[1].name, "fn_b");
}

#[test]
fn to_dialect_preserves_safety_settings() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![
            SafetySetting {
                category: HarmCategory::HarmCategoryDangerousContent,
                threshold: HarmBlockThreshold::BlockOnlyHigh,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryHateSpeech,
                threshold: HarmBlockThreshold::BlockLowAndAbove,
            },
        ]);
    let dialect = to_dialect_request(&req);
    let ss = dialect.safety_settings.unwrap();
    assert_eq!(ss.len(), 2);
    assert_eq!(ss[0].category, HarmCategory::HarmCategoryDangerousContent);
    assert_eq!(ss[1].threshold, HarmBlockThreshold::BlockLowAndAbove);
}

// =========================================================================
// 3. Response translation (~15 tests)
// =========================================================================

#[test]
fn receipt_complete_to_response_text() {
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
    let dialect_resp: GeminiResponse = receipt.into();
    assert_eq!(dialect_resp.candidates.len(), 1);
    assert_eq!(
        dialect_resp.candidates[0].finish_reason.as_deref(),
        Some("STOP")
    );
    match &dialect_resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn receipt_partial_maps_to_max_tokens() {
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
fn receipt_failed_maps_to_other() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Failed)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
}

#[test]
fn receipt_with_usage_maps_to_response_metadata() {
    let usage = UsageNormalized {
        input_tokens: Some(200),
        output_tokens: Some(100),
        ..Default::default()
    };
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .usage(usage)
        .build();
    let resp: GeminiResponse = receipt.into();
    let meta = resp.usage_metadata.unwrap();
    assert_eq!(meta.prompt_token_count, 200);
    assert_eq!(meta.candidates_token_count, 100);
    assert_eq!(meta.total_token_count, 300);
}

#[test]
fn receipt_without_usage_has_no_metadata() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert!(resp.usage_metadata.is_none());
}

#[test]
fn receipt_tool_call_maps_to_function_call() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "get_time".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({"tz": "UTC"}),
            },
            ext: None,
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "get_time");
            assert_eq!(args["tz"], "UTC");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn receipt_tool_result_maps_to_function_response() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "get_time".into(),
                tool_use_id: None,
                output: json!({"time": "12:00"}),
                is_error: false,
            },
            ext: None,
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "get_time");
            assert_eq!(response["time"], "12:00");
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn receipt_mixed_trace_maps_all_parts() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "I'll search.".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        })
        .build();
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].content.parts.len(), 2);
    assert!(matches!(
        &resp.candidates[0].content.parts[0],
        GeminiPart::Text(_)
    ));
    assert!(matches!(
        &resp.candidates[0].content.parts[1],
        GeminiPart::FunctionCall { .. }
    ));
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
fn response_text_accessor_returns_none_when_no_text() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::function_call("fn", json!({}))]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert!(resp.text().is_none());
}

#[test]
fn response_function_calls_accessor() {
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
fn receipt_no_prompt_feedback_in_response() {
    let receipt = ReceiptBuilder::new("gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp: GeminiResponse = receipt.into();
    assert!(resp.prompt_feedback.is_none());
}

// =========================================================================
// 4. Streaming (~10 tests)
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
fn from_dialect_stream_chunk_text() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("delta text".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let event = from_dialect_stream_chunk(&chunk);
    assert_eq!(event.text(), Some("delta text"));
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
    assert_eq!(event.candidates.len(), 1);
    match &event.candidates[0].content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args["q"], "test");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_chunk_produces_assistant_delta() {
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
fn dialect_map_stream_event_same_as_map_stream_chunk() {
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

// =========================================================================
// 5. Tool use (~10 tests)
// =========================================================================

#[test]
fn tool_declaration_to_dialect_and_back() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "get_weather".into(),
            description: "Get weather for a location".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        }],
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tools(vec![tool.clone()]);
    let dialect = to_dialect_request(&req);
    let dialect_tools = dialect.tools.unwrap();
    assert_eq!(dialect_tools[0].function_declarations[0].name, "get_weather");
    assert_eq!(
        dialect_tools[0].function_declarations[0].parameters["required"][0],
        "location"
    );
}

#[test]
fn function_call_ir_roundtrip() {
    let content = Content::model(vec![Part::function_call(
        "search",
        json!({"query": "rust async"}),
    )]);
    let dialect = GeminiContent {
        role: content.role.clone(),
        parts: content
            .parts
            .iter()
            .map(|p| match p {
                Part::FunctionCall { name, args } => GeminiPart::FunctionCall {
                    name: name.clone(),
                    args: args.clone(),
                },
                _ => unreachable!(),
            })
            .collect(),
    };
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "search");
            assert_eq!(input["query"], "rust async");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn function_response_ir_roundtrip() {
    let content = Content::user(vec![Part::function_response(
        "search",
        json!("results here"),
    )]);
    let dialect = GeminiContent {
        role: content.role.clone(),
        parts: content
            .parts
            .iter()
            .map(|p| match p {
                Part::FunctionResponse { name, response } => GeminiPart::FunctionResponse {
                    name: name.clone(),
                    response: response.clone(),
                },
                _ => unreachable!(),
            })
            .collect(),
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
}

#[test]
fn tool_config_auto_mode() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });
    let dialect = to_dialect_request(&req);
    let tc = dialect.tool_config.unwrap();
    assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Auto);
}

#[test]
fn tool_config_any_mode_with_allowed_functions() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Any,
                allowed_function_names: Some(vec!["search".into(), "read".into()]),
            },
        });
    let dialect = to_dialect_request(&req);
    let tc = dialect.tool_config.unwrap();
    assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Any);
    let names = tc.function_calling_config.allowed_function_names.unwrap();
    assert_eq!(names, vec!["search", "read"]);
}

#[test]
fn tool_config_none_mode() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::None,
                allowed_function_names: None,
            },
        });
    let dialect = to_dialect_request(&req);
    let tc = dialect.tool_config.unwrap();
    assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::None);
}

#[test]
fn function_calling_mode_serde_roundtrip() {
    for mode in [
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
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
    assert_eq!(gemini.description, "Read a file");
    let back = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(back, canonical);
}

#[test]
fn multiple_function_declarations_in_tool() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![
                FunctionDeclaration {
                    name: "read_file".into(),
                    description: "Read".into(),
                    parameters: json!({"type": "object"}),
                },
                FunctionDeclaration {
                    name: "write_file".into(),
                    description: "Write".into(),
                    parameters: json!({"type": "object"}),
                },
                FunctionDeclaration {
                    name: "delete_file".into(),
                    description: "Delete".into(),
                    parameters: json!({"type": "object"}),
                },
            ],
        }]);
    let dialect = to_dialect_request(&req);
    let tools = dialect.tools.unwrap();
    assert_eq!(tools[0].function_declarations.len(), 3);
}

// =========================================================================
// 6. Edge cases (~10 tests)
// =========================================================================

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
fn multi_modal_content_mixed_parts() {
    let content = Content::user(vec![
        Part::text("What's in this image?"),
        Part::inline_data("image/png", "base64data=="),
    ]);
    let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(content);
    let dialect = to_dialect_request(&req);
    assert_eq!(dialect.contents[0].parts.len(), 2);
    assert!(matches!(
        &dialect.contents[0].parts[0],
        GeminiPart::Text(_)
    ));
    assert!(matches!(
        &dialect.contents[0].parts[1],
        GeminiPart::InlineData(_)
    ));
}

#[test]
fn inline_data_ir_roundtrip_via_shim() {
    let content = Content::user(vec![Part::inline_data("image/jpeg", "abc123")]);
    let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(content);
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, None);
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }
    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "abc123");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn empty_parts_content() {
    let content = Content::user(vec![]);
    let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(content);
    let dialect = to_dialect_request(&req);
    assert!(dialect.contents[0].parts.is_empty());
}

#[test]
fn model_name_variations_canonical_mapping() {
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
        assert!(canonical.starts_with("google/"));
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
fn is_known_model_returns_true_for_known() {
    assert!(dialect::is_known_model("gemini-2.5-flash"));
    assert!(dialect::is_known_model("gemini-2.5-pro"));
}

#[test]
fn is_known_model_returns_false_for_unknown() {
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("gemini-99.0"));
}

#[test]
fn gemini_error_display_messages() {
    let err = GeminiError::RequestConversion("bad input".into());
    assert!(err.to_string().contains("bad input"));

    let err = GeminiError::ResponseConversion("bad output".into());
    assert!(err.to_string().contains("bad output"));

    let err = GeminiError::BackendError("timeout".into());
    assert!(err.to_string().contains("timeout"));
}

#[test]
fn gemini_client_model_accessor() {
    let client = GeminiClient::new("gemini-2.5-pro");
    assert_eq!(client.model(), "gemini-2.5-pro");
}

// =========================================================================
// Additional: Usage IR conversions, gen_config, roundtrips
// =========================================================================

#[test]
fn usage_to_ir_and_back() {
    let original = UsageMetadata {
        prompt_token_count: 150,
        candidates_token_count: 75,
        total_token_count: 225,
    };
    let ir = usage_to_ir(&original);
    assert_eq!(ir.input_tokens, 150);
    assert_eq!(ir.output_tokens, 75);
    assert_eq!(ir.total_tokens, 225);
    let back = usage_from_ir(&ir);
    assert_eq!(back, original);
}

#[test]
fn gen_config_dialect_roundtrip() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(512),
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(20),
        stop_sequences: Some(vec!["DONE".into()]),
        response_mime_type: Some("text/plain".into()),
        response_schema: None,
    };
    let dialect_cfg = abp_gemini_sdk::dialect::GeminiGenerationConfig {
        max_output_tokens: cfg.max_output_tokens,
        temperature: cfg.temperature,
        top_p: cfg.top_p,
        top_k: cfg.top_k,
        candidate_count: None,
        stop_sequences: cfg.stop_sequences.clone(),
        response_mime_type: cfg.response_mime_type.clone(),
        response_schema: cfg.response_schema.clone(),
    };
    let back = gen_config_from_dialect(&dialect_cfg);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.stop_sequences, cfg.stop_sequences);
    assert_eq!(back.response_mime_type, cfg.response_mime_type);
}

#[test]
fn capability_manifest_contains_expected_entries() {
    let manifest = dialect::capability_manifest();
    assert!(manifest.contains_key(&abp_core::Capability::Streaming));
    assert!(manifest.contains_key(&abp_core::Capability::ToolRead));
    assert!(manifest.contains_key(&abp_core::Capability::StructuredOutputJsonSchema));
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
    assert_eq!(events.len(), 1);
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
fn dialect_map_response_ignores_inline_data() {
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
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
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

#[test]
fn dialect_version_is_set() {
    assert_eq!(dialect::DIALECT_VERSION, "gemini/v0.1");
}

#[test]
fn default_model_is_gemini_flash() {
    assert_eq!(dialect::DEFAULT_MODEL, "gemini-2.5-flash");
}
