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
//! Comprehensive tests for the `abp-shim-gemini` crate.
//!
//! Covers initialization, request/response translation, function calling,
//! content parts, safety settings, generation config, model mapping,
//! error translation, and edge cases.

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
    self, GeminiCandidate, GeminiContent, GeminiInlineData, GeminiPart, GeminiResponse,
    GeminiStreamChunk, GeminiUsageMetadata,
};
use abp_gemini_sdk::lowering;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};

// =========================================================================
// Helpers
// =========================================================================

fn simple_user_request(text: &str) -> GenerateContentRequest {
    GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text(text)]))
}

fn dialect_response_with_text(text: &str) -> GeminiResponse {
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

// =========================================================================
// 1. Gemini shim initialization and configuration
// =========================================================================

#[test]
fn client_new_sets_model() {
    let client = GeminiClient::new("gemini-2.5-flash");
    assert_eq!(client.model(), "gemini-2.5-flash");
}

#[test]
fn client_new_accepts_string() {
    let client = GeminiClient::new(String::from("gemini-2.5-pro"));
    assert_eq!(client.model(), "gemini-2.5-pro");
}

#[test]
fn client_new_custom_model() {
    let client = GeminiClient::new("my-custom-model");
    assert_eq!(client.model(), "my-custom-model");
}

#[test]
fn client_clone() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let cloned = client.clone();
    assert_eq!(cloned.model(), "gemini-2.5-flash");
}

#[test]
fn client_debug_impl() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let debug = format!("{client:?}");
    assert!(debug.contains("gemini-2.5-flash"));
}

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
fn request_builder_chaining() {
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("hi")]))
        .add_content(Content::model(vec![Part::text("hello")]))
        .add_content(Content::user(vec![Part::text("bye")]));
    assert_eq!(req.contents.len(), 3);
}

// =========================================================================
// 2. Request translation (Gemini → IR)
// =========================================================================

#[test]
fn simple_text_to_ir() {
    let req = simple_user_request("Hello world");
    let dialect_req = to_dialect_request(&req);
    let ir = lowering::to_ir(
        &dialect_req.contents,
        dialect_req.system_instruction.as_ref(),
    );
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Hello world");
}

#[test]
fn multi_turn_to_ir() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Q1")]))
        .add_content(Content::model(vec![Part::text("A1")]))
        .add_content(Content::user(vec![Part::text("Q2")]));
    let dialect_req = to_dialect_request(&req);
    let ir = lowering::to_ir(
        &dialect_req.contents,
        dialect_req.system_instruction.as_ref(),
    );
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[2].role, IrRole::User);
}

#[test]
fn system_instruction_becomes_system_role_in_ir() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be concise")]))
        .add_content(Content::user(vec![Part::text("Hello")]));
    let dialect_req = to_dialect_request(&req);
    let ir = lowering::to_ir(
        &dialect_req.contents,
        dialect_req.system_instruction.as_ref(),
    );
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[0].text_content(), "Be concise");
    assert_eq!(ir.messages[1].role, IrRole::User);
}

#[test]
fn empty_request_to_ir() {
    let req = GenerateContentRequest::new("gemini-2.5-flash");
    let dialect_req = to_dialect_request(&req);
    let ir = lowering::to_ir(
        &dialect_req.contents,
        dialect_req.system_instruction.as_ref(),
    );
    assert!(ir.is_empty());
}

#[test]
fn user_role_maps_to_ir_user() {
    let content = Content::user(vec![Part::text("hello")]);
    let dialect = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hello".into())],
    };
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(content.role, "user");
}

#[test]
fn model_role_maps_to_ir_assistant() {
    let dialect = GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("world".into())],
    };
    let ir = lowering::to_ir(&[dialect], None);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
}

#[test]
fn request_with_all_optional_fields() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
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
    let dialect = to_dialect_request(&req);
    assert!(dialect.system_instruction.is_some());
    assert!(dialect.generation_config.is_some());
    assert!(dialect.safety_settings.is_some());
    assert!(dialect.tools.is_some());
    assert!(dialect.tool_config.is_some());
}

// =========================================================================
// 3. Response translation (IR → Gemini)
// =========================================================================

#[test]
fn from_dialect_response_text() {
    let resp = dialect_response_with_text("Hello!");
    let shim_resp = from_dialect_response(&resp);
    assert_eq!(shim_resp.text(), Some("Hello!"));
    assert_eq!(shim_resp.candidates.len(), 1);
}

#[test]
fn from_dialect_response_preserves_finish_reason() {
    let resp = dialect_response_with_text("done");
    let shim_resp = from_dialect_response(&resp);
    assert_eq!(
        shim_resp.candidates[0].finish_reason.as_deref(),
        Some("STOP")
    );
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
    let resp = dialect_response_with_text("no usage");
    let shim_resp = from_dialect_response(&resp);
    assert!(shim_resp.usage_metadata.is_none());
}

#[test]
fn from_dialect_response_multiple_candidates() {
    let resp = GeminiResponse {
        candidates: vec![
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("first".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("second".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
        ],
        prompt_feedback: None,
        usage_metadata: None,
    };
    let shim_resp = from_dialect_response(&resp);
    assert_eq!(shim_resp.candidates.len(), 2);
    // text() returns the first candidate's first text
    assert_eq!(shim_resp.text(), Some("first"));
}

#[test]
fn response_text_accessor_returns_none_for_non_text() {
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
fn response_text_accessor_empty_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
    };
    assert!(resp.text().is_none());
}

// =========================================================================
// 4. Function/tool declarations
// =========================================================================

#[test]
fn tool_declaration_single_function() {
    let tool = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "get_weather".into(),
            description: "Get weather for location".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        }],
    };
    assert_eq!(tool.function_declarations.len(), 1);
    assert_eq!(tool.function_declarations[0].name, "get_weather");
}

#[test]
fn tool_declaration_multiple_functions() {
    let tool = ToolDeclaration {
        function_declarations: vec![
            FunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object"}),
            },
            FunctionDeclaration {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        ],
    };
    assert_eq!(tool.function_declarations.len(), 2);
}

#[test]
fn tool_declaration_to_dialect_preserves_fields() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("hi")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "calc".into(),
                description: "Calculator".into(),
                parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
            }],
        }]);
    let dialect = to_dialect_request(&req);
    let tools = dialect.tools.unwrap();
    assert_eq!(tools[0].function_declarations[0].name, "calc");
    assert_eq!(tools[0].function_declarations[0].description, "Calculator");
}

#[test]
fn tool_config_auto_mode() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(tc);
    let dialect = to_dialect_request(&req);
    let tc = dialect.tool_config.unwrap();
    assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Auto);
}

#[test]
fn tool_config_any_mode_with_allowed_functions() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["fn_a".into(), "fn_b".into()]),
        },
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(tc);
    let dialect = to_dialect_request(&req);
    let tc = dialect.tool_config.unwrap();
    assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Any);
    let allowed = tc.function_calling_config.allowed_function_names.unwrap();
    assert_eq!(allowed, vec!["fn_a", "fn_b"]);
}

#[test]
fn tool_config_none_mode() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::None,
            allowed_function_names: None,
        },
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .tool_config(tc);
    let dialect = to_dialect_request(&req);
    let tc = dialect.tool_config.unwrap();
    assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::None);
}

#[test]
fn function_call_part_to_ir() {
    let content = GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        }],
    };
    let ir = lowering::to_ir(&[content], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(name, "search");
            assert_eq!(input, &json!({"query": "rust"}));
            assert_eq!(id, "gemini_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn function_response_part_to_ir() {
    let content = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results"),
        }],
    };
    let ir = lowering::to_ir(&[content], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            content,
        } => {
            assert_eq!(tool_use_id, "gemini_search");
            assert!(!is_error);
            assert!(!content.is_empty());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_call_roundtrip_through_ir() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "write_file".into(),
            args: json!({"path": "a.rs", "content": "fn main() {}"}),
        }],
    }];
    let ir = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "write_file");
            assert_eq!(args, &json!({"path": "a.rs", "content": "fn main() {}"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn function_response_roundtrip_through_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "read_file".into(),
            response: json!("file contents here"),
        }],
    }];
    let ir = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "read_file");
            assert_eq!(response, &json!("file contents here"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn response_function_calls_accessor() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::function_call("fn_a", json!({"x": 1})),
                Part::function_call("fn_b", json!({"y": 2})),
            ]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "fn_a");
    assert_eq!(calls[0].1, &json!({"x": 1}));
    assert_eq!(calls[1].0, "fn_b");
}

#[test]
fn response_function_calls_empty_when_no_calls() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("no calls")]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert!(resp.function_calls().is_empty());
}

#[test]
fn response_function_calls_empty_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
    };
    assert!(resp.function_calls().is_empty());
}

// =========================================================================
// 5. Content parts handling
// =========================================================================

#[test]
fn part_text_constructor() {
    let p = Part::text("hello");
    assert!(matches!(p, Part::Text(ref s) if s == "hello"));
}

#[test]
fn part_text_from_string() {
    let p = Part::text(String::from("world"));
    assert!(matches!(p, Part::Text(ref s) if s == "world"));
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
    let p = Part::function_call("search", json!({"q": "rust"}));
    match &p {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "rust"}));
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn part_function_response_constructor() {
    let p = Part::function_response("search", json!("results"));
    match &p {
        Part::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response, &json!("results"));
        }
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn content_user_helper() {
    let c = Content::user(vec![Part::text("hello")]);
    assert_eq!(c.role, "user");
    assert_eq!(c.parts.len(), 1);
}

#[test]
fn content_model_helper() {
    let c = Content::model(vec![Part::text("world")]);
    assert_eq!(c.role, "model");
    assert_eq!(c.parts.len(), 1);
}

#[test]
fn content_with_multiple_parts() {
    let c = Content::user(vec![
        Part::text("describe this image"),
        Part::inline_data("image/jpeg", "abc123"),
    ]);
    assert_eq!(c.parts.len(), 2);
}

#[test]
fn inline_data_to_ir_image() {
    let content = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/jpeg".into(),
            data: "abc123".into(),
        })],
    };
    let ir = lowering::to_ir(&[content], None);
    match &ir.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn inline_data_roundtrip_through_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "xyz789".into(),
        })],
    }];
    let ir = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&ir);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/png");
            assert_eq!(d.data, "xyz789");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn mixed_text_and_function_call_in_content() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::Text("Let me search.".into()),
            GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({}),
            },
        ],
    }];
    let ir = lowering::to_ir(&contents, None);
    assert_eq!(ir.messages[0].content.len(), 2);
    assert!(matches!(
        ir.messages[0].content[0],
        IrContentBlock::Text { .. }
    ));
    assert!(matches!(
        ir.messages[0].content[1],
        IrContentBlock::ToolUse { .. }
    ));
}

#[test]
fn part_serde_roundtrip_text() {
    let p = Part::text("hello");
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn part_serde_roundtrip_inline_data() {
    let p = Part::inline_data("image/png", "data");
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn part_serde_roundtrip_function_call() {
    let p = Part::function_call("fn_name", json!({"a": 1}));
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn part_serde_roundtrip_function_response() {
    let p = Part::function_response("fn_name", json!("result"));
    let json = serde_json::to_string(&p).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

// =========================================================================
// 6. Safety settings
// =========================================================================

#[test]
fn safety_setting_construction() {
    let s = SafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockNone,
    };
    assert_eq!(s.category, HarmCategory::HarmCategoryHarassment);
    assert_eq!(s.threshold, HarmBlockThreshold::BlockNone);
}

#[test]
fn safety_settings_all_categories() {
    let categories = [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in &categories {
        let s = SafetySetting {
            category: *cat,
            threshold: HarmBlockThreshold::BlockNone,
        };
        assert_eq!(s.category, *cat);
    }
}

#[test]
fn safety_settings_all_thresholds() {
    let thresholds = [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for thr in &thresholds {
        let s = SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: *thr,
        };
        assert_eq!(s.threshold, *thr);
    }
}

#[test]
fn safety_settings_preserved_in_dialect_request() {
    let settings = vec![
        SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        },
        SafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        },
    ];
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(settings);
    let dialect = to_dialect_request(&req);
    let ds = dialect.safety_settings.unwrap();
    assert_eq!(ds.len(), 2);
    assert_eq!(ds[0].category, HarmCategory::HarmCategoryHarassment);
    assert_eq!(ds[0].threshold, HarmBlockThreshold::BlockNone);
    assert_eq!(ds[1].category, HarmCategory::HarmCategoryDangerousContent);
    assert_eq!(ds[1].threshold, HarmBlockThreshold::BlockOnlyHigh);
}

#[test]
fn safety_setting_serde_roundtrip() {
    let s = SafetySetting {
        category: HarmCategory::HarmCategorySexuallyExplicit,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn safety_setting_json_format() {
    let s = SafetySetting {
        category: HarmCategory::HarmCategoryHateSpeech,
        threshold: HarmBlockThreshold::BlockLowAndAbove,
    };
    let json = serde_json::to_value(&s).unwrap();
    // camelCase serialization
    assert!(json.get("category").is_some());
    assert!(json.get("threshold").is_some());
}

#[test]
fn no_safety_settings_in_request() {
    let req = simple_user_request("hello");
    let dialect = to_dialect_request(&req);
    assert!(dialect.safety_settings.is_none());
}

// =========================================================================
// 7. Generation config
// =========================================================================

#[test]
fn generation_config_default_all_none() {
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
fn generation_config_to_dialect_roundtrip() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(1024),
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        stop_sequences: Some(vec!["END".into(), "STOP".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(cfg.clone());
    let dialect = to_dialect_request(&req);
    let dcfg = dialect.generation_config.unwrap();

    assert_eq!(dcfg.max_output_tokens, Some(1024));
    assert_eq!(dcfg.temperature, Some(0.7));
    assert_eq!(dcfg.top_p, Some(0.9));
    assert_eq!(dcfg.top_k, Some(40));
    assert_eq!(dcfg.stop_sequences, Some(vec!["END".into(), "STOP".into()]));
    assert_eq!(dcfg.response_mime_type, Some("application/json".into()));

    let back = gen_config_from_dialect(&dcfg);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.stop_sequences, cfg.stop_sequences);
    assert_eq!(back.response_mime_type, cfg.response_mime_type);
}

#[test]
fn generation_config_partial_fields() {
    let cfg = GenerationConfig {
        temperature: Some(0.5),
        max_output_tokens: Some(512),
        ..Default::default()
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(cfg);
    let dialect = to_dialect_request(&req);
    let dcfg = dialect.generation_config.unwrap();
    assert_eq!(dcfg.temperature, Some(0.5));
    assert_eq!(dcfg.max_output_tokens, Some(512));
    assert!(dcfg.top_p.is_none());
    assert!(dcfg.top_k.is_none());
}

#[test]
fn generation_config_serde_skips_none() {
    let cfg = GenerationConfig {
        temperature: Some(1.0),
        ..Default::default()
    };
    let json = serde_json::to_value(&cfg).unwrap();
    assert!(json.get("temperature").is_some());
    // None fields should be skipped
    assert!(json.get("topP").is_none());
}

#[test]
fn generation_config_with_json_schema_response() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"}
        }
    });
    let cfg = GenerationConfig {
        response_mime_type: Some("application/json".into()),
        response_schema: Some(schema.clone()),
        ..Default::default()
    };
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("structured output")]))
        .generation_config(cfg);
    let dialect = to_dialect_request(&req);
    let dcfg = dialect.generation_config.unwrap();
    assert_eq!(dcfg.response_schema, Some(schema));
}

#[test]
fn no_generation_config_in_request() {
    let req = simple_user_request("hello");
    let dialect = to_dialect_request(&req);
    assert!(dialect.generation_config.is_none());
}

// =========================================================================
// 8. Model mapping
// =========================================================================

#[test]
fn to_canonical_model_adds_prefix() {
    assert_eq!(
        dialect::to_canonical_model("gemini-2.5-flash"),
        "google/gemini-2.5-flash"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("google/gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn from_canonical_model_no_prefix_passthrough() {
    assert_eq!(
        dialect::from_canonical_model("custom-model"),
        "custom-model"
    );
}

#[test]
fn canonical_model_roundtrip() {
    let original = "gemini-2.5-pro";
    let canonical = dialect::to_canonical_model(original);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, original);
}

#[test]
fn known_model_flash() {
    assert!(dialect::is_known_model("gemini-2.5-flash"));
}

#[test]
fn known_model_pro() {
    assert!(dialect::is_known_model("gemini-2.5-pro"));
}

#[test]
fn known_model_2_0_flash() {
    assert!(dialect::is_known_model("gemini-2.0-flash"));
}

#[test]
fn known_model_1_5_flash() {
    assert!(dialect::is_known_model("gemini-1.5-flash"));
}

#[test]
fn known_model_1_5_pro() {
    assert!(dialect::is_known_model("gemini-1.5-pro"));
}

#[test]
fn unknown_model() {
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("claude-3.5-sonnet"));
}

#[test]
fn default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "gemini-2.5-flash");
}

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "gemini/v0.1");
}

#[tokio::test]
async fn generate_uses_canonical_model_in_work_order() {
    let client = GeminiClient::new("gemini-2.5-pro");
    let req = GenerateContentRequest::new("gemini-2.5-pro")
        .add_content(Content::user(vec![Part::text("test")]));
    let resp = client.generate(req).await.unwrap();
    // Verify response is produced (work order internally uses canonical model)
    assert!(!resp.candidates.is_empty());
}

// =========================================================================
// 9. Error translation
// =========================================================================

#[test]
fn gemini_error_request_conversion_display() {
    let err = GeminiError::RequestConversion("bad input".into());
    assert!(err.to_string().contains("bad input"));
    assert!(err.to_string().contains("request conversion"));
}

#[test]
fn gemini_error_response_conversion_display() {
    let err = GeminiError::ResponseConversion("bad output".into());
    assert!(err.to_string().contains("bad output"));
    assert!(err.to_string().contains("response conversion"));
}

#[test]
fn gemini_error_backend_display() {
    let err = GeminiError::BackendError("backend down".into());
    assert!(err.to_string().contains("backend down"));
    assert!(err.to_string().contains("backend error"));
}

#[test]
fn gemini_error_serde_from_json_error() {
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let err = GeminiError::Serde(json_err);
    assert!(err.to_string().contains("serde error"));
}

#[test]
fn gemini_error_debug_impl() {
    let err = GeminiError::BackendError("test".into());
    let debug = format!("{err:?}");
    assert!(debug.contains("BackendError"));
}

// =========================================================================
// 10. Edge cases
// =========================================================================

#[test]
fn empty_text_part() {
    let p = Part::text("");
    assert!(matches!(p, Part::Text(ref s) if s.is_empty()));
}

#[test]
fn empty_contents_request() {
    let req = GenerateContentRequest::new("gemini-2.5-flash");
    let dialect = to_dialect_request(&req);
    assert!(dialect.contents.is_empty());
}

#[test]
fn empty_parts_in_content() {
    let c = Content::user(vec![]);
    assert!(c.parts.is_empty());
    assert_eq!(c.role, "user");
}

#[test]
fn unicode_text_content() {
    let text = "こんにちは世界 🌍 مرحبا";
    let req = simple_user_request(text);
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, None);
    assert_eq!(ir.messages[0].text_content(), text);
}

#[test]
fn very_long_text_content() {
    let text = "a".repeat(100_000);
    let req = simple_user_request(&text);
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, None);
    assert_eq!(ir.messages[0].text_content().len(), 100_000);
}

#[test]
fn newlines_in_text() {
    let text = "line1\nline2\nline3";
    let req = simple_user_request(text);
    let dialect = to_dialect_request(&req);
    let ir = lowering::to_ir(&dialect.contents, None);
    assert_eq!(ir.messages[0].text_content(), text);
}

#[test]
fn special_chars_in_function_name() {
    let p = Part::function_call("my-fn_v2.0", json!({}));
    match &p {
        Part::FunctionCall { name, .. } => assert_eq!(name, "my-fn_v2.0"),
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn empty_function_args() {
    let p = Part::function_call("no_args", json!({}));
    match &p {
        Part::FunctionCall { args, .. } => assert_eq!(args, &json!({})),
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn complex_function_args() {
    let args = json!({
        "nested": {"key": "value"},
        "array": [1, 2, 3],
        "null_field": null,
        "bool_field": true
    });
    let p = Part::function_call("complex", args.clone());
    match &p {
        Part::FunctionCall { args: a, .. } => assert_eq!(a, &args),
        _ => panic!("expected FunctionCall"),
    }
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
    // No system message because text is empty
    assert_eq!(ir.len(), 1);
}

#[test]
fn system_messages_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn extract_system_instruction_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = lowering::extract_system_instruction(&conv).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn extract_system_instruction_none_when_absent() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    assert!(lowering::extract_system_instruction(&conv).is_none());
}

#[test]
fn function_response_with_object_payload() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "api".into(),
            response: json!({"status": 200, "body": "ok"}),
        }],
    }];
    let ir = lowering::to_ir(&contents, None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            let text = match &content[0] {
                IrContentBlock::Text { text } => text.as_str(),
                _ => panic!("expected text block"),
            };
            assert!(text.contains("200"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ── Usage conversions ───────────────────────────────────────────────────

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
    assert_eq!(ir.cache_read_tokens, 0);
    assert_eq!(ir.cache_write_tokens, 0);
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
fn usage_roundtrip() {
    let original = UsageMetadata {
        prompt_token_count: 42,
        candidates_token_count: 18,
        total_token_count: 60,
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

// ── Streaming ───────────────────────────────────────────────────────────

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
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("final".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 10,
            total_token_count: 15,
        }),
    };
    let event = from_dialect_stream_chunk(&chunk);
    let usage = event.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 5);
    assert_eq!(usage.candidates_token_count, 10);
    assert_eq!(usage.total_token_count, 15);
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
    assert!(event.text().is_none());
    match &event.candidates[0].content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "test"}));
        }
        _ => panic!("expected FunctionCall"),
    }
}

#[test]
fn stream_event_text_accessor() {
    let event = StreamEvent {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("hello")]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(event.text(), Some("hello"));
}

#[test]
fn stream_event_text_accessor_no_text() {
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
fn stream_event_text_accessor_empty_candidates() {
    let event = StreamEvent {
        candidates: vec![],
        usage_metadata: None,
    };
    assert!(event.text().is_none());
}

#[tokio::test]
async fn streaming_produces_events() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Stream test")]));
    let stream = client.generate_stream(request).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    // Should have at least one text event and one usage event
    assert!(events.len() >= 2);
}

#[tokio::test]
async fn streaming_final_chunk_has_usage() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Count")]));
    let stream = client.generate_stream(request).await.unwrap();
    let events: Vec<StreamEvent> = stream.collect().await;
    // Last event should have usage metadata
    let last = events.last().unwrap();
    assert!(last.usage_metadata.is_some());
}

// ── Full pipeline (async) ───────────────────────────────────────────────

#[tokio::test]
async fn simple_generate_returns_text() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = simple_user_request("Hello");
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
    assert!(response.text().is_some());
}

#[tokio::test]
async fn generate_returns_usage() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = simple_user_request("Count to 5");
    let response = client.generate(request).await.unwrap();
    let usage = response.usage_metadata.as_ref().unwrap();
    assert!(usage.total_token_count > 0);
    assert_eq!(
        usage.total_token_count,
        usage.prompt_token_count + usage.candidates_token_count
    );
}

#[tokio::test]
async fn generate_with_system_instruction() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be brief")]))
        .add_content(Content::user(vec![Part::text("Explain Rust")]));
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

#[tokio::test]
async fn generate_multi_turn() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hi")]))
        .add_content(Content::model(vec![Part::text("Hello!")]))
        .add_content(Content::user(vec![Part::text("How are you?")]));
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

#[tokio::test]
async fn generate_with_tools() {
    let client = GeminiClient::new("gemini-2.5-flash");
    let request = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("What's the weather?")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "get_weather".into(),
                description: "Get weather".into(),
                parameters: json!({"type": "object", "properties": {"location": {"type": "string"}}}),
            }],
        }]);
    let response = client.generate(request).await.unwrap();
    assert!(!response.candidates.is_empty());
}

// ── Dialect conversion coverage ─────────────────────────────────────────

#[test]
fn to_dialect_request_preserves_model() {
    let req = GenerateContentRequest::new("custom-model")
        .add_content(Content::user(vec![Part::text("hi")]));
    let dialect = to_dialect_request(&req);
    assert_eq!(dialect.model, "custom-model");
}

#[test]
fn to_dialect_request_preserves_system_instruction() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(Content::user(vec![Part::text("Be helpful")]))
        .add_content(Content::user(vec![Part::text("hi")]));
    let dialect = to_dialect_request(&req);
    let sys = dialect.system_instruction.unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be helpful"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn to_dialect_request_no_system_instruction() {
    let req = simple_user_request("hello");
    let dialect = to_dialect_request(&req);
    assert!(dialect.system_instruction.is_none());
}

#[test]
fn to_dialect_request_no_tools() {
    let req = simple_user_request("hello");
    let dialect = to_dialect_request(&req);
    assert!(dialect.tools.is_none());
    assert!(dialect.tool_config.is_none());
}

// ── Capability manifest ─────────────────────────────────────────────────

#[test]
fn capability_manifest_has_streaming() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_structured_output() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_tool_read_native() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

// ── IR helpers ──────────────────────────────────────────────────────────

#[test]
fn ir_conversation_builder_api() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "hi"));
    assert_eq!(conv.len(), 2);
    assert!(!conv.is_empty());
}

#[test]
fn ir_message_text_only_check() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert!(msg.is_text_only());
}

#[test]
fn ir_message_not_text_only_with_tool_use() {
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
fn ir_conversation_system_message() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hello"),
    ]);
    assert!(conv.system_message().is_some());
    assert_eq!(conv.system_message().unwrap().text_content(), "sys");
}

#[test]
fn ir_conversation_last_assistant() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "q"),
        IrMessage::text(IrRole::Assistant, "a1"),
        IrMessage::text(IrRole::User, "q2"),
        IrMessage::text(IrRole::Assistant, "a2"),
    ]);
    assert_eq!(conv.last_assistant().unwrap().text_content(), "a2");
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(10, 20);
    assert_eq!(u.input_tokens, 10);
    assert_eq!(u.output_tokens, 20);
    assert_eq!(u.total_tokens, 30);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(5, 15);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 15);
    assert_eq!(merged.output_tokens, 35);
    assert_eq!(merged.total_tokens, 50);
}

// ── Thinking content block ──────────────────────────────────────────────

#[test]
fn thinking_block_maps_to_text_in_gemini() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "thinking about it...".into(),
        }],
    )]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "thinking about it..."),
        other => panic!("expected Text, got {other:?}"),
    }
}

// ── FunctionDeclaration equality ────────────────────────────────────────

#[test]
fn function_declaration_equality() {
    let a = FunctionDeclaration {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"type": "object"}),
    };
    let b = FunctionDeclaration {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"type": "object"}),
    };
    assert_eq!(a, b);
}

#[test]
fn function_declaration_inequality() {
    let a = FunctionDeclaration {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"type": "object"}),
    };
    let b = FunctionDeclaration {
        name: "g".into(),
        description: "d".into(),
        parameters: json!({"type": "object"}),
    };
    assert_ne!(a, b);
}

// ── ToolDeclaration equality ────────────────────────────────────────────

#[test]
fn tool_declaration_equality() {
    let a = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        }],
    };
    let b = ToolDeclaration {
        function_declarations: vec![FunctionDeclaration {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        }],
    };
    assert_eq!(a, b);
}

// ── Part clone and debug ────────────────────────────────────────────────

#[test]
fn part_clone() {
    let p = Part::text("hello");
    let cloned = p.clone();
    assert_eq!(p, cloned);
}

#[test]
fn part_debug() {
    let p = Part::text("hello");
    let debug = format!("{p:?}");
    assert!(debug.contains("hello"));
}

// ── Candidate accessors ─────────────────────────────────────────────────

#[test]
fn candidate_with_finish_reason() {
    let c = Candidate {
        content: Content::model(vec![Part::text("done")]),
        finish_reason: Some("STOP".into()),
    };
    assert_eq!(c.finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn candidate_without_finish_reason() {
    let c = Candidate {
        content: Content::model(vec![Part::text("partial")]),
        finish_reason: None,
    };
    assert!(c.finish_reason.is_none());
}

// ── Dialect tool def conversion ─────────────────────────────────────────

#[test]
fn canonical_tool_def_to_gemini() {
    let def = dialect::CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let gemini = dialect::tool_def_to_gemini(&def);
    assert_eq!(gemini.name, "read_file");
    assert_eq!(gemini.description, "Read a file");
}

#[test]
fn canonical_tool_def_from_gemini() {
    let gemini = dialect::GeminiFunctionDeclaration {
        name: "write_file".into(),
        description: "Write a file".into(),
        parameters: json!({"type": "object"}),
    };
    let def = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(def.name, "write_file");
    assert_eq!(def.description, "Write a file");
    assert_eq!(def.parameters_schema, json!({"type": "object"}));
}

#[test]
fn canonical_tool_def_roundtrip() {
    let original = dialect::CanonicalToolDef {
        name: "search".into(),
        description: "Search".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let gemini = dialect::tool_def_to_gemini(&original);
    let back = dialect::tool_def_from_gemini(&gemini);
    assert_eq!(back.name, original.name);
    assert_eq!(back.description, original.description);
    assert_eq!(back.parameters_schema, original.parameters_schema);
}

// ── GeminiConfig ────────────────────────────────────────────────────────

#[test]
fn gemini_config_default() {
    let cfg = dialect::GeminiConfig::default();
    assert!(cfg.base_url.contains("googleapis.com"));
    assert!(cfg.model.contains("gemini"));
    assert!(cfg.max_output_tokens.unwrap_or(0) > 0);
    assert!(cfg.api_key.is_empty());
}

// ── Map work order ──────────────────────────────────────────────────────

#[test]
fn map_work_order_basic() {
    use abp_core::WorkOrderBuilder;
    let wo = WorkOrderBuilder::new("Explain Rust").build();
    let cfg = dialect::GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.contents.len(), 1);
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("Explain Rust")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn map_work_order_model_override() {
    use abp_core::WorkOrderBuilder;
    let wo = WorkOrderBuilder::new("task")
        .model("gemini-2.5-pro")
        .build();
    let cfg = dialect::GeminiConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-pro");
}

// ── Map response to events ──────────────────────────────────────────────

#[test]
fn map_response_produces_assistant_message() {
    use abp_core::AgentEventKind;
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("output".into())],
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
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "output"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn map_stream_chunk_produces_delta() {
    use abp_core::AgentEventKind;
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
    let events = dialect::map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "delta"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

// ── Serde roundtrips for shim types ─────────────────────────────────────

#[test]
fn tool_config_serde_roundtrip() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["fn_a".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn function_calling_mode_serde() {
    let modes = [
        (FunctionCallingMode::Auto, "\"AUTO\""),
        (FunctionCallingMode::Any, "\"ANY\""),
        (FunctionCallingMode::None, "\"NONE\""),
    ];
    for (mode, expected) in &modes {
        let json = serde_json::to_string(mode).unwrap();
        assert_eq!(&json, expected);
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, mode);
    }
}
