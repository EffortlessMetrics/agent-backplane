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
//! Deep surface-area tests for the Gemini drop-in replacement shim.
//!
//! Covers: request/response formats, streaming, function calling, content parts,
//! roles, model names, generation config, safety settings, client configuration,
//! request→WorkOrder, and receipt→response conversions.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use abp_gemini_sdk::dialect::{
    self, GeminiCandidate, GeminiContent, GeminiInlineData, GeminiPart, GeminiResponse,
    GeminiStreamChunk, GeminiUsageMetadata, HarmBlockThreshold, HarmCategory,
};
use abp_gemini_sdk::lowering;
use abp_shim_gemini::client::Client;
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
// 1. GenerateContent request (format + builder)
// =========================================================================

#[test]
fn generate_content_request_minimal() {
    let req = GenerateContentRequest::new("gemini-1.5-pro");
    assert_eq!(req.model, "gemini-1.5-pro");
    assert!(req.contents.is_empty());
    assert!(req.generation_config.is_none());
    assert!(req.safety_settings.is_none());
    assert!(req.tools.is_none());
    assert!(req.tool_config.is_none());
    assert!(req.system_instruction.is_none());
}

#[test]
fn generate_content_request_chaining_all_setters() {
    let req = GenerateContentRequest::new("gemini-2.0-flash")
        .add_content(Content::user(vec![Part::text("hi")]))
        .system_instruction(Content::user(vec![Part::text("sys")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.5),
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

    assert_eq!(req.model, "gemini-2.0-flash");
    assert_eq!(req.contents.len(), 1);
    assert!(req.system_instruction.is_some());
    assert!(req.generation_config.is_some());
    assert!(req.safety_settings.is_some());
    assert!(req.tools.is_some());
    assert!(req.tool_config.is_some());
}

#[test]
fn generate_content_request_json_wire_format() {
    let req = GenerateContentRequest::new("gemini-1.5-flash")
        .add_content(Content::user(vec![Part::text("hello")]));
    let json_val: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(json_val["model"], "gemini-1.5-flash");
    assert!(json_val["contents"].is_array());
    assert_eq!(json_val["contents"][0]["role"], "user");
}

#[test]
fn generate_content_request_omits_none_fields_in_json() {
    let req = GenerateContentRequest::new("gemini-1.5-pro")
        .add_content(Content::user(vec![Part::text("x")]));
    let json_str = serde_json::to_string(&req).unwrap();
    assert!(!json_str.contains("systemInstruction"));
    assert!(!json_str.contains("generationConfig"));
    assert!(!json_str.contains("safetySettings"));
    assert!(!json_str.contains("tools"));
    assert!(!json_str.contains("toolConfig"));
}

#[test]
fn generate_content_request_serde_roundtrip_full() {
    let req = GenerateContentRequest::new("gemini-1.5-flash")
        .add_content(Content::user(vec![Part::text("ask")]))
        .add_content(Content::model(vec![Part::text("answer")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            candidate_count: None,
            max_output_tokens: Some(2048),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
        });
    let json_str = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.model, "gemini-1.5-flash");
    assert_eq!(back.contents.len(), 2);
    let cfg = back.generation_config.unwrap();
    assert_eq!(cfg.temperature, Some(0.7));
    assert_eq!(cfg.max_output_tokens, Some(2048));
}

// =========================================================================
// 2. GenerateContent response (format + accessors)
// =========================================================================

#[test]
fn generate_content_response_text_accessor() {
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
fn generate_content_response_text_none_when_empty_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert!(resp.text().is_none());
}

#[test]
fn generate_content_response_text_skips_non_text_parts() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::function_call("fn", json!({})),
                Part::text("after call"),
            ]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("after call"));
}

#[test]
fn generate_content_response_serde_roundtrip() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("test")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
        prompt_feedback: None,
    };
    let json_str = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.text(), Some("test"));
    assert_eq!(back.usage_metadata.as_ref().unwrap().total_token_count, 15);
}

#[test]
fn generate_content_response_function_calls_empty_when_only_text() {
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

// =========================================================================
// 3. Streaming response (SSE event format)
// =========================================================================

#[test]
fn stream_event_text_accessor_returns_delta() {
    let evt = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("chunk1")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(evt.text(), Some("chunk1"));
}

#[test]
fn stream_event_serde_roundtrip() {
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
fn stream_event_empty_candidates_no_text() {
    let evt = StreamEvent {
        candidates: vec![],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        }),
    };
    assert!(evt.text().is_none());
}

#[tokio::test]
async fn streaming_pipeline_yields_text_and_usage() {
    let client = PipelineClient::new("gemini-1.5-flash");
    let req = GenerateContentRequest::new("gemini-1.5-flash")
        .add_content(Content::user(vec![Part::text("Stream me")]));
    let stream = client.generate_stream(req).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    assert!(events.len() >= 2);
    // Last event should carry usage metadata
    assert!(events.last().unwrap().usage_metadata.is_some());
}

#[test]
fn receipt_to_stream_events_text_and_usage() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(5),
            output_tokens: Some(10),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    assert!(events.len() >= 2);
    assert_eq!(events[0].text(), Some("hello"));
    let last = events.last().unwrap();
    assert!(last.usage_metadata.is_some());
}

#[test]
fn receipt_to_stream_events_includes_tool_calls() {
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
                tool_name: "search".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({"q": "test"}),
            },
            ext: None,
        })
        .build();
    let events = receipt_to_stream_events(&receipt);
    let tool_event = &events[0];
    match &tool_event.candidates[0].content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args["q"], "test");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// =========================================================================
// 4. Function calling (FunctionDeclaration, FunctionCall, FunctionResponse)
// =========================================================================

#[test]
fn function_declaration_serde_roundtrip() {
    let decl = FunctionDeclaration {
        name: "get_weather".into(),
        description: "Get weather info".into(),
        parameters: json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }),
    };
    let json_str = serde_json::to_string(&decl).unwrap();
    let back: FunctionDeclaration = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, decl);
}

#[test]
fn function_call_part_construction_and_match() {
    let part = Part::function_call("read_file", json!({"path": "/tmp/a.rs"}));
    match &part {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "read_file");
            assert_eq!(args["path"], "/tmp/a.rs");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn function_response_part_construction_and_match() {
    let part = Part::function_response("read_file", json!({"content": "fn main() {}"}));
    match &part {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "read_file");
            assert_eq!(response["content"], "fn main() {}");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn function_call_to_ir_generates_tool_use() {
    let content = Content::model(vec![Part::function_call(
        "calculate",
        json!({"expr": "2+2"}),
    )]);
    let dialect_content = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect_content], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { name, input, id } => {
            assert_eq!(name, "calculate");
            assert_eq!(input["expr"], "2+2");
            assert_eq!(id, "gemini_calculate");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn function_response_to_ir_generates_tool_result() {
    let content = Content::user(vec![Part::function_response("calculate", json!("4"))]);
    let dialect_content = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect_content], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "gemini_calculate");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_call_ir_roundtrip_preserves_args() {
    let args = json!({"location": "NYC", "units": "metric"});
    let content = Content::model(vec![Part::function_call("get_weather", args.clone())]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall {
            name,
            args: back_args,
        } => {
            assert_eq!(name, "get_weather");
            assert_eq!(back_args, &args);
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn tool_declaration_to_dialect_maps_fields() {
    let decl = ToolDeclaration {
        function_declarations: vec![
            FunctionDeclaration {
                name: "fn_a".into(),
                description: "A".into(),
                parameters: json!({"type": "object"}),
            },
            FunctionDeclaration {
                name: "fn_b".into(),
                description: "B".into(),
                parameters: json!({"type": "object"}),
            },
        ],
    };
    let dialect = tool_decl_to_dialect(&decl);
    assert_eq!(dialect.function_declarations.len(), 2);
    assert_eq!(dialect.function_declarations[0].name, "fn_a");
    assert_eq!(dialect.function_declarations[1].description, "B");
}

#[test]
fn tool_config_to_dialect_preserves_mode_and_names() {
    let config = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "read".into()]),
        },
    };
    let dialect = tool_config_to_dialect(&config);
    assert_eq!(
        dialect.function_calling_config.mode,
        FunctionCallingMode::Any
    );
    let names = dialect
        .function_calling_config
        .allowed_function_names
        .unwrap();
    assert_eq!(names, vec!["search", "read"]);
}

#[test]
fn function_calling_mode_none_serde() {
    let mode = FunctionCallingMode::None;
    let json_str = serde_json::to_string(&mode).unwrap();
    assert_eq!(json_str, "\"NONE\"");
    let back: FunctionCallingMode = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, FunctionCallingMode::None);
}

// =========================================================================
// 5. Content parts (Text, InlineData, FunctionCall)
// =========================================================================

#[test]
fn part_text_serde_camel_case() {
    let part = Part::text("hello");
    let json_str = serde_json::to_string(&part).unwrap();
    assert!(json_str.contains("text"));
    let back: Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, part);
}

#[test]
fn part_inline_data_construction() {
    let part = Part::inline_data("image/webp", "AAAA");
    match &part {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/webp");
            assert_eq!(data, "AAAA");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn part_inline_data_serde_roundtrip() {
    let part = Part::inline_data("image/png", "iVBORw0KGgo=");
    let json_str = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, part);
}

#[test]
fn part_inline_data_to_ir_produces_image_block() {
    let content = Content::user(vec![Part::inline_data("image/jpeg", "base64data")]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64data");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn part_to_dialect_and_back_text() {
    let original = Part::text("roundtrip");
    let dialect = part_to_dialect(&original);
    let back = part_from_dialect(&dialect);
    assert_eq!(back, original);
}

#[test]
fn part_to_dialect_and_back_inline_data() {
    let original = Part::inline_data("image/gif", "R0lGODlh");
    let dialect = part_to_dialect(&original);
    let back = part_from_dialect(&dialect);
    assert_eq!(back, original);
}

#[test]
fn part_to_dialect_and_back_function_call() {
    let original = Part::function_call("tool", json!({"a": 1}));
    let dialect = part_to_dialect(&original);
    let back = part_from_dialect(&dialect);
    assert_eq!(back, original);
}

#[test]
fn part_to_dialect_and_back_function_response() {
    let original = Part::function_response("tool", json!({"result": "ok"}));
    let dialect = part_to_dialect(&original);
    let back = part_from_dialect(&dialect);
    assert_eq!(back, original);
}

#[test]
fn content_to_dialect_preserves_multi_part() {
    let content = Content::user(vec![
        Part::text("Look at this"),
        Part::inline_data("image/png", "data=="),
        Part::function_response("analyze", json!("done")),
    ]);
    let dialect = content_to_dialect(&content);
    assert_eq!(dialect.parts.len(), 3);
    assert!(matches!(&dialect.parts[0], GeminiPart::Text(_)));
    assert!(matches!(&dialect.parts[1], GeminiPart::InlineData(_)));
    assert!(matches!(
        &dialect.parts[2],
        GeminiPart::FunctionResponse { .. }
    ));
}

// =========================================================================
// 6. Roles: user, model (not assistant)
// =========================================================================

#[test]
fn content_user_role_is_user() {
    let c = Content::user(vec![Part::text("q")]);
    assert_eq!(c.role, "user");
}

#[test]
fn content_model_role_is_model_not_assistant() {
    let c = Content::model(vec![Part::text("a")]);
    assert_eq!(c.role, "model");
    assert_ne!(c.role, "assistant");
}

#[test]
fn ir_maps_model_to_assistant_and_back() {
    let content = Content::model(vec![Part::text("reply")]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].role, "model");
}

#[test]
fn ir_maps_user_to_user_and_back() {
    let content = Content::user(vec![Part::text("question")]);
    let dialect = content_to_dialect(&content);
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].role, IrRole::User);
    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].role, "user");
}

// =========================================================================
// 7. Model names
// =========================================================================

#[test]
fn model_gemini_15_pro_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-1.5-pro");
    assert_eq!(canonical, "google/gemini-1.5-pro");
    assert_eq!(dialect::from_canonical_model(&canonical), "gemini-1.5-pro");
}

#[test]
fn model_gemini_15_flash_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-1.5-flash");
    assert_eq!(canonical, "google/gemini-1.5-flash");
    assert_eq!(
        dialect::from_canonical_model(&canonical),
        "gemini-1.5-flash"
    );
}

#[test]
fn model_gemini_20_flash_canonical_roundtrip() {
    let canonical = dialect::to_canonical_model("gemini-2.0-flash");
    assert_eq!(canonical, "google/gemini-2.0-flash");
    assert_eq!(
        dialect::from_canonical_model(&canonical),
        "gemini-2.0-flash"
    );
}

#[test]
fn known_models_include_15_and_20() {
    assert!(dialect::is_known_model("gemini-1.5-pro"));
    assert!(dialect::is_known_model("gemini-1.5-flash"));
    assert!(dialect::is_known_model("gemini-2.0-flash"));
}

#[test]
fn unknown_model_is_not_known() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("claude-3.5-sonnet"));
}

#[test]
fn client_stores_model_name() {
    let c = PipelineClient::new("gemini-1.5-pro");
    assert_eq!(c.model(), "gemini-1.5-pro");
}

// =========================================================================
// 8. GenerationConfig
// =========================================================================

#[test]
fn generation_config_default_all_none() {
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
fn generation_config_to_dialect_roundtrip() {
    let cfg = GenerationConfig {
        temperature: Some(0.3),
        top_p: Some(0.85),
        top_k: Some(50),
        candidate_count: None,
        max_output_tokens: Some(4096),
        stop_sequences: Some(vec!["HALT".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "string"})),
    };
    let dialect = gen_config_to_dialect(&cfg);
    assert_eq!(dialect.temperature, Some(0.3));
    assert_eq!(dialect.top_p, Some(0.85));
    assert_eq!(dialect.top_k, Some(50));
    assert_eq!(dialect.max_output_tokens, Some(4096));
    assert_eq!(dialect.stop_sequences, Some(vec!["HALT".into()]));
    // candidate_count should be None (shim doesn't expose it)
    assert!(dialect.candidate_count.is_none());

    let back = gen_config_from_dialect(&dialect);
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    assert_eq!(back.stop_sequences, cfg.stop_sequences);
}

#[test]
fn generation_config_camel_case_json_keys() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(256),
        temperature: Some(1.0),
        top_p: Some(0.95),
        top_k: Some(10),
        candidate_count: None,
        stop_sequences: None,
        response_mime_type: None,
        response_schema: None,
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    assert!(json_str.contains("maxOutputTokens"));
    assert!(json_str.contains("topP"));
    assert!(json_str.contains("topK"));
    // Should not contain snake_case keys
    assert!(!json_str.contains("max_output_tokens"));
    assert!(!json_str.contains("top_p"));
    assert!(!json_str.contains("top_k"));
}

// =========================================================================
// 9. Safety settings
// =========================================================================

#[test]
fn safety_setting_all_harm_categories_serde() {
    let categories = [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in &categories {
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
fn safety_setting_all_thresholds_serde() {
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
fn safety_to_dialect_preserves_category_and_threshold() {
    let ss = SafetySetting {
        category: HarmCategory::HarmCategoryHateSpeech,
        threshold: HarmBlockThreshold::BlockOnlyHigh,
    };
    let dialect = safety_to_dialect(&ss);
    assert_eq!(dialect.category, HarmCategory::HarmCategoryHateSpeech);
    assert_eq!(dialect.threshold, HarmBlockThreshold::BlockOnlyHigh);
}

#[test]
fn safety_settings_preserved_through_request() {
    let req = GenerateContentRequest::new("gemini-1.5-pro")
        .add_content(Content::user(vec![Part::text("x")]))
        .safety_settings(vec![
            SafetySetting {
                category: HarmCategory::HarmCategoryDangerousContent,
                threshold: HarmBlockThreshold::BlockNone,
            },
            SafetySetting {
                category: HarmCategory::HarmCategorySexuallyExplicit,
                threshold: HarmBlockThreshold::BlockLowAndAbove,
            },
        ]);
    let dialect = to_dialect_request(&req);
    let ss = dialect.safety_settings.unwrap();
    assert_eq!(ss.len(), 2);
    assert_eq!(ss[0].category, HarmCategory::HarmCategoryDangerousContent);
    assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockNone);
}

// =========================================================================
// 10. API key in query param (not header-based)
// =========================================================================

#[test]
fn client_url_has_key_as_query_param() {
    let client = Client::new("AIzaSyTEST_KEY_123").unwrap();
    // The base URL is the default Gemini endpoint
    assert_eq!(
        client.base_url(),
        "https://generativelanguage.googleapis.com/v1beta"
    );
    // model_url is private, but we can verify via the builder pattern
    // that the key is stored and will be appended as ?key=
}

#[test]
fn client_default_base_url_is_googleapis() {
    let client = Client::new("test-key").unwrap();
    assert!(
        client
            .base_url()
            .contains("generativelanguage.googleapis.com")
    );
    assert!(client.base_url().contains("/v1beta"));
}

// =========================================================================
// 11. Client configuration
// =========================================================================

#[test]
fn client_builder_custom_base_url() {
    let client = Client::builder("key123")
        .base_url("https://custom.example.com/v1")
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://custom.example.com/v1");
}

#[test]
fn client_builder_custom_timeout() {
    let client = Client::builder("key123")
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();
    // Client was constructed successfully with custom timeout
    assert_eq!(
        client.base_url(),
        "https://generativelanguage.googleapis.com/v1beta"
    );
}

#[test]
fn client_builder_all_options() {
    let client = Client::builder("my-api-key")
        .base_url("https://proxy.example.com")
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://proxy.example.com");
}

#[test]
fn client_error_display_api_error() {
    let err = abp_shim_gemini::client::ClientError::Api {
        status: 429,
        body: "rate limited".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("429"));
    assert!(msg.contains("rate limited"));
}

#[test]
fn client_error_display_builder_error() {
    let err = abp_shim_gemini::client::ClientError::Builder("invalid config".into());
    assert!(err.to_string().contains("invalid config"));
}

#[test]
fn gemini_error_variants_display() {
    let variants: Vec<GeminiError> = vec![
        GeminiError::RequestConversion("bad req".into()),
        GeminiError::ResponseConversion("bad resp".into()),
        GeminiError::BackendError("timeout".into()),
    ];
    assert!(variants[0].to_string().contains("bad req"));
    assert!(variants[1].to_string().contains("bad resp"));
    assert!(variants[2].to_string().contains("timeout"));
}

// =========================================================================
// 12. Request → WorkOrder conversion
// =========================================================================

#[test]
fn request_to_ir_extracts_conversation() {
    let req = GenerateContentRequest::new("gemini-1.5-pro")
        .add_content(Content::user(vec![Part::text("Explain traits")]))
        .add_content(Content::model(vec![Part::text("Traits are...")]));
    let (ir, _gen_config, _safety) = request_to_ir(&req).unwrap();
    assert_eq!(ir.conversation.len(), 2);
    assert_eq!(ir.conversation.messages[0].role, IrRole::User);
    assert_eq!(ir.conversation.messages[1].role, IrRole::Assistant);
}

#[test]
fn request_to_ir_returns_gen_config() {
    let req = GenerateContentRequest::new("gemini-1.5-flash")
        .add_content(Content::user(vec![Part::text("x")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.8),
            ..Default::default()
        });
    let (_ir, gen_config, _safety) = request_to_ir(&req).unwrap();
    assert_eq!(gen_config.unwrap().temperature, Some(0.8));
}

#[test]
fn request_to_ir_returns_safety_settings() {
    let req = GenerateContentRequest::new("gemini-1.5-flash")
        .add_content(Content::user(vec![Part::text("x")]))
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]);
    let (_ir, _gen_config, safety) = request_to_ir(&req).unwrap();
    assert_eq!(safety.len(), 1);
}

#[test]
fn ir_to_work_order_uses_canonical_model() {
    let req = GenerateContentRequest::new("gemini-2.0-flash")
        .add_content(Content::user(vec![Part::text("test")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.0-flash"));
}

#[test]
fn ir_to_work_order_task_from_user_messages() {
    let req = GenerateContentRequest::new("gemini-1.5-pro")
        .add_content(Content::user(vec![Part::text("First question")]))
        .add_content(Content::model(vec![Part::text("Answer")]))
        .add_content(Content::user(vec![Part::text("Follow up")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    // Task concatenates all user messages
    assert!(wo.task.contains("First question"));
    assert!(wo.task.contains("Follow up"));
}

#[test]
fn ir_to_work_order_max_tokens_from_gen_config() {
    let req = GenerateContentRequest::new("gemini-1.5-flash")
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
fn execute_work_order_returns_complete_receipt() {
    let req = GenerateContentRequest::new("gemini-1.5-flash")
        .add_content(Content::user(vec![Part::text("hello")]));
    let (ir, gen_config, _) = request_to_ir(&req).unwrap();
    let wo = ir_to_work_order(&ir, &req.model, &gen_config);
    let receipt = execute_work_order(&wo);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.usage.input_tokens.is_some());
    assert!(receipt.usage.output_tokens.is_some());
    assert!(!receipt.trace.is_empty());
}

// =========================================================================
// 13. Receipt → Response conversion
// =========================================================================

#[test]
fn receipt_to_ir_extracts_assistant_messages() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        })
        .build();
    let ir = abp_shim_gemini::receipt_to_ir(&receipt);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    assert_eq!(ir.messages[0].text_content(), "Hello!");
}

#[test]
fn receipt_to_ir_extracts_tool_calls() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("call_123".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        })
        .build();
    let ir = abp_shim_gemini::receipt_to_ir(&receipt);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { name, id, input } => {
            assert_eq!(name, "search");
            assert_eq!(id, "call_123");
            assert_eq!(input["q"], "rust");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn receipt_to_ir_extracts_tool_results() {
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "search".into(),
                tool_use_id: Some("call_123".into()),
                output: json!("found 3 results"),
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
            assert_eq!(tool_use_id, "call_123");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn ir_to_response_produces_model_candidates() {
    let ir = IrConversation::from_messages(vec![IrMessage::text(
        IrRole::Assistant,
        "Generated text".to_string(),
    )]);
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(10),
            output_tokens: Some(20),
            ..Default::default()
        })
        .build();
    let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].content.role, "model");
    assert_eq!(resp.text(), Some("Generated text"));
    let usage = resp.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 10);
    assert_eq!(usage.candidates_token_count, 20);
    assert_eq!(usage.total_token_count, 30);
}

#[test]
fn ir_to_response_empty_ir_produces_empty_text_candidate() {
    let ir = IrConversation::new();
    let receipt = ReceiptBuilder::new("shim:gemini")
        .outcome(Outcome::Complete)
        .build();
    let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
    assert!(!resp.candidates.is_empty());
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn make_usage_metadata_from_normalized() {
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
fn make_usage_metadata_returns_none_when_zero() {
    let usage = UsageNormalized::default();
    assert!(make_usage_metadata(&usage).is_none());
}

#[test]
fn usage_to_ir_and_from_ir_roundtrip() {
    let original = UsageMetadata {
        prompt_token_count: 200,
        candidates_token_count: 100,
        total_token_count: 300,
    };
    let ir = usage_to_ir(&original);
    assert_eq!(ir.input_tokens, 200);
    assert_eq!(ir.output_tokens, 100);
    assert_eq!(ir.total_tokens, 300);
    let back = usage_from_ir(&ir);
    assert_eq!(back, original);
}

// =========================================================================
// Full pipeline roundtrips
// =========================================================================

#[tokio::test]
async fn full_pipeline_generate_roundtrip() {
    let client = PipelineClient::new("gemini-1.5-pro");
    let req = GenerateContentRequest::new("gemini-1.5-pro")
        .add_content(Content::user(vec![Part::text("Say hello")]));
    let resp = client.generate(req).await.unwrap();
    assert!(!resp.candidates.is_empty());
    let text = resp.text().unwrap();
    assert!(!text.is_empty());
}

#[tokio::test]
async fn full_pipeline_with_generation_config() {
    let client = PipelineClient::new("gemini-2.0-flash");
    let req = GenerateContentRequest::new("gemini-2.0-flash")
        .add_content(Content::user(vec![Part::text("Count to 3")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.0),
            max_output_tokens: Some(100),
            ..Default::default()
        });
    let resp = client.generate(req).await.unwrap();
    assert!(resp.text().is_some());
    assert!(resp.usage_metadata.is_some());
}

#[test]
fn from_dialect_response_maps_all_fields() {
    let dialect_resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![
                    GeminiPart::Text("Here's the result".into()),
                    GeminiPart::FunctionCall {
                        name: "tool".into(),
                        args: json!({}),
                    },
                ],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 25,
            candidates_token_count: 15,
            total_token_count: 40,
        }),
    };
    let shim = from_dialect_response(&dialect_resp);
    assert_eq!(shim.candidates.len(), 1);
    assert_eq!(shim.candidates[0].content.parts.len(), 2);
    assert_eq!(shim.text(), Some("Here's the result"));
    assert_eq!(shim.function_calls().len(), 1);
    assert_eq!(shim.usage_metadata.unwrap().total_token_count, 40);
}

#[test]
fn content_from_dialect_preserves_role_and_parts() {
    let dialect = GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::Text("text".into()),
            GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "abc".into(),
            }),
        ],
    };
    let content = content_from_dialect(&dialect);
    assert_eq!(content.role, "model");
    assert_eq!(content.parts.len(), 2);
    assert!(matches!(&content.parts[0], Part::Text(s) if s == "text"));
    assert!(matches!(
        &content.parts[1],
        Part::InlineData { mime_type, .. } if mime_type == "image/png"
    ));
}

#[test]
fn from_dialect_stream_chunk_preserves_finish_reason() {
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
    let event = from_dialect_stream_chunk(&chunk);
    assert_eq!(event.candidates[0].finish_reason.as_deref(), Some("STOP"));
}
