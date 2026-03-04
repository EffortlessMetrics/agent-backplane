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
//! Comprehensive tests for Google Gemini SDK dialect mapping.
//!
//! Validates the full lifecycle: Gemini request format → ABP IR → WorkOrder → Receipt → response,
//! including function calling, streaming, safety settings, multi-modal content, token counting,
//! finish reasons, grounding/citations, and model capability mapping.

use std::collections::BTreeMap;

use serde_json::json;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEventKind, CONTRACT_VERSION, Capability, ContextPacket, ContextSnippet, SupportLevel,
    WorkOrderBuilder,
};
use abp_gemini_sdk::dialect::{
    self, CanonicalToolDef, DynamicRetrievalConfig, FunctionCallingMode, GeminiCandidate,
    GeminiCitationMetadata, GeminiCitationSource, GeminiConfig, GeminiContent,
    GeminiFunctionCallingConfig, GeminiFunctionDeclaration, GeminiGenerationConfig,
    GeminiGroundingConfig, GeminiInlineData, GeminiPart, GeminiRequest, GeminiResponse,
    GeminiSafetyRating, GeminiSafetySetting, GeminiStreamChunk, GeminiTool, GeminiToolConfig,
    GeminiUsageMetadata, GoogleSearchRetrieval, HarmBlockThreshold, HarmCategory, HarmProbability,
};
use abp_gemini_sdk::lowering;
use abp_shim_gemini::{
    Candidate, Content, FunctionCallingConfig, FunctionDeclaration, GeminiClient,
    GenerateContentRequest, GenerateContentResponse, GenerationConfig, Part, SafetySetting,
    StreamEvent, ToolConfig, ToolDeclaration, UsageMetadata, from_dialect_response,
    from_dialect_stream_chunk, gen_config_from_dialect, to_dialect_request, usage_from_ir,
    usage_to_ir,
};

// =========================================================================
// Helpers
// =========================================================================

fn user_text(text: &str) -> GeminiContent {
    GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

fn model_text(text: &str) -> GeminiContent {
    GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

fn make_response(parts: Vec<GeminiPart>, finish_reason: Option<&str>) -> GeminiResponse {
    GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts,
            },
            finish_reason: finish_reason.map(String::from),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    }
}

fn make_response_with_usage(text: &str, prompt: u64, candidates: u64) -> GeminiResponse {
    GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: model_text(text).clone(),
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

fn make_stream_chunk(parts: Vec<GeminiPart>) -> GeminiStreamChunk {
    GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts,
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    }
}

fn simple_request(model: &str, text: &str) -> GenerateContentRequest {
    GenerateContentRequest::new(model).add_content(Content::user(vec![Part::text(text)]))
}

fn default_config() -> GeminiConfig {
    GeminiConfig::default()
}

// =========================================================================
// Module 1: generateContent request format mapping to WorkOrder
// =========================================================================

mod request_mapping {
    use super::*;

    #[test]
    fn simple_text_request_to_dialect() {
        let req = simple_request("gemini-2.5-flash", "Hello world");
        let dialect_req = to_dialect_request(&req);
        assert_eq!(dialect_req.model, "gemini-2.5-flash");
        assert_eq!(dialect_req.contents.len(), 1);
        assert_eq!(dialect_req.contents[0].role, "user");
    }

    #[test]
    fn request_preserves_model_name() {
        let req = simple_request("gemini-1.5-pro", "test");
        let dialect_req = to_dialect_request(&req);
        assert_eq!(dialect_req.model, "gemini-1.5-pro");
    }

    #[test]
    fn request_maps_text_parts() {
        let req = simple_request("gemini-2.5-flash", "describe rust");
        let dialect_req = to_dialect_request(&req);
        match &dialect_req.contents[0].parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "describe rust"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn request_with_system_instruction() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be concise")]))
            .add_content(Content::user(vec![Part::text("Hello")]));
        let dialect_req = to_dialect_request(&req);
        assert!(dialect_req.system_instruction.is_some());
        let sys = dialect_req.system_instruction.unwrap();
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn request_without_system_instruction_is_none() {
        let req = simple_request("gemini-2.5-flash", "Hi");
        let dialect_req = to_dialect_request(&req);
        assert!(dialect_req.system_instruction.is_none());
    }

    #[test]
    fn request_with_generation_config() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.7),
                max_output_tokens: Some(1024),
                top_p: Some(0.9),
                top_k: Some(40),
                ..Default::default()
            });
        let dialect_req = to_dialect_request(&req);
        let cfg = dialect_req.generation_config.unwrap();
        assert_eq!(cfg.temperature, Some(0.7));
        assert_eq!(cfg.max_output_tokens, Some(1024));
        assert_eq!(cfg.top_p, Some(0.9));
        assert_eq!(cfg.top_k, Some(40));
    }

    #[test]
    fn request_generation_config_stop_sequences() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(GenerationConfig {
                stop_sequences: Some(vec!["END".into(), "STOP".into()]),
                ..Default::default()
            });
        let dialect_req = to_dialect_request(&req);
        let cfg = dialect_req.generation_config.unwrap();
        assert_eq!(cfg.stop_sequences.unwrap(), vec!["END", "STOP"]);
    }

    #[test]
    fn request_generation_config_response_mime_type() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(GenerationConfig {
                response_mime_type: Some("application/json".into()),
                ..Default::default()
            });
        let dialect_req = to_dialect_request(&req);
        let cfg = dialect_req.generation_config.unwrap();
        assert_eq!(cfg.response_mime_type.unwrap(), "application/json");
    }

    #[test]
    fn request_generation_config_response_schema() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(GenerationConfig {
                response_schema: Some(schema.clone()),
                ..Default::default()
            });
        let dialect_req = to_dialect_request(&req);
        let cfg = dialect_req.generation_config.unwrap();
        assert_eq!(cfg.response_schema.unwrap(), schema);
    }

    #[test]
    fn request_to_ir_extracts_conversation() {
        let req = simple_request("gemini-2.5-flash", "Hello Gemini");
        let dialect_req = to_dialect_request(&req);
        let ir = lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello Gemini");
    }

    #[test]
    fn multi_turn_request_to_ir() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Q1")]))
            .add_content(Content::model(vec![Part::text("A1")]))
            .add_content(Content::user(vec![Part::text("Q2")]));
        let dialect_req = to_dialect_request(&req);
        let ir = lowering::to_ir(&dialect_req.contents, None);
        assert_eq!(ir.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].role, IrRole::User);
    }

    #[test]
    fn request_system_instruction_in_ir() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text(
                "You are a helpful assistant",
            )]))
            .add_content(Content::user(vec![Part::text("Hi")]));
        let dialect_req = to_dialect_request(&req);
        let ir = lowering::to_ir(
            &dialect_req.contents,
            dialect_req.system_instruction.as_ref(),
        );
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "You are a helpful assistant");
    }

    #[test]
    fn map_work_order_uses_task_from_user_text() {
        let wo = WorkOrderBuilder::new("Hello Gemini")
            .model("google/gemini-2.5-flash")
            .build();
        assert_eq!(wo.task, "Hello Gemini");
        assert_eq!(wo.config.model.as_deref(), Some("google/gemini-2.5-flash"));
    }

    #[test]
    fn map_work_order_uses_config_for_gemini_request() {
        let config = GeminiConfig {
            model: "gemini-2.5-pro".into(),
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("test task").build();
        let req = dialect::map_work_order(&wo, &config);
        assert_eq!(req.model, "gemini-2.5-pro");
        let gen_cfg = req.generation_config.unwrap();
        assert_eq!(gen_cfg.max_output_tokens, Some(2048));
        assert_eq!(gen_cfg.temperature, Some(0.5));
    }

    #[test]
    fn map_work_order_includes_context_snippets() {
        let wo = WorkOrderBuilder::new("refactor")
            .context(ContextPacket {
                files: vec![],
                snippets: vec![ContextSnippet {
                    name: "main.rs".into(),
                    content: "fn main() {}".into(),
                }],
            })
            .build();
        let config = default_config();
        let req = dialect::map_work_order(&wo, &config);
        let text = match &req.contents[0].parts[0] {
            GeminiPart::Text(t) => t.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("main.rs"));
        assert!(text.contains("fn main() {}"));
    }

    #[test]
    fn empty_request_produces_valid_dialect_request() {
        let req = GenerateContentRequest::new("gemini-2.5-flash");
        let dialect_req = to_dialect_request(&req);
        assert!(dialect_req.contents.is_empty());
        assert!(dialect_req.generation_config.is_none());
        assert!(dialect_req.safety_settings.is_none());
        assert!(dialect_req.tools.is_none());
    }
}

// =========================================================================
// Module 2: functionCall / functionResponse mapping to ABP tool events
// =========================================================================

mod function_call_mapping {
    use super::*;

    #[test]
    fn function_call_part_to_ir_tool_use() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "get_weather".into(),
                args: json!({"location": "London"}),
            }],
        };
        let ir = lowering::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "gemini_get_weather");
                assert_eq!(name, "get_weather");
                assert_eq!(input, &json!({"location": "London"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_response_part_to_ir_tool_result() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "get_weather".into(),
                response: json!("sunny, 72F"),
            }],
        };
        let ir = lowering::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                content,
            } => {
                assert_eq!(tool_use_id, "gemini_get_weather");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn function_call_roundtrip_through_ir() {
        let original = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"query": "Rust lang"}),
            }],
        }];
        let ir = lowering::to_ir(&original, None);
        let back = lowering::from_ir(&ir);
        match &back[0].parts[0] {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args, &json!({"query": "Rust lang"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn function_response_roundtrip_through_ir() {
        let original = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("found 42 results"),
            }],
        }];
        let ir = lowering::to_ir(&original, None);
        let back = lowering::from_ir(&ir);
        match &back[0].parts[0] {
            GeminiPart::FunctionResponse { name, response } => {
                assert_eq!(name, "search");
                assert_eq!(response, &json!("found 42 results"));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    #[test]
    fn function_response_with_json_object_payload() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "api_call".into(),
                response: json!({"status": 200, "data": {"items": [1, 2, 3]}}),
            }],
        };
        let ir = lowering::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                let text = match &content[0] {
                    IrContentBlock::Text { text } => text,
                    _ => panic!("expected text block"),
                };
                assert!(text.contains("200"));
                assert!(text.contains("items"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn multiple_function_calls_in_one_content() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![
                GeminiPart::FunctionCall {
                    name: "fn_a".into(),
                    args: json!({"x": 1}),
                },
                GeminiPart::FunctionCall {
                    name: "fn_b".into(),
                    args: json!({"y": 2}),
                },
            ],
        };
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].content.len(), 2);
        assert!(
            matches!(&ir.messages[0].content[0], IrContentBlock::ToolUse { name, .. } if name == "fn_a")
        );
        assert!(
            matches!(&ir.messages[0].content[1], IrContentBlock::ToolUse { name, .. } if name == "fn_b")
        );
    }

    #[test]
    fn function_call_then_response_multi_turn() {
        let contents = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Search for Rust".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "Rust"}),
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
        assert_eq!(ir.messages[3].text_content(), "Here are the results.");
    }

    #[test]
    fn tool_declaration_to_dialect() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "calc".into(),
                    description: "Calculate expression".into(),
                    parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
                }],
            }]);
        let dialect_req = to_dialect_request(&req);
        let tools = dialect_req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function_declarations[0].name, "calc");
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
        let dialect_req = to_dialect_request(&req);
        let tc = dialect_req.tool_config.unwrap();
        assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Auto);
    }

    #[test]
    fn tool_config_any_mode_with_allowed_names() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Any,
                    allowed_function_names: Some(vec!["search".into(), "calc".into()]),
                },
            });
        let dialect_req = to_dialect_request(&req);
        let tc = dialect_req.tool_config.unwrap();
        assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::Any);
        assert_eq!(
            tc.function_calling_config.allowed_function_names.unwrap(),
            vec!["search", "calc"]
        );
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
        let dialect_req = to_dialect_request(&req);
        let tc = dialect_req.tool_config.unwrap();
        assert_eq!(tc.function_calling_config.mode, FunctionCallingMode::None);
    }

    #[test]
    fn canonical_tool_def_to_gemini_function_declaration() {
        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let gemini_fn = dialect::tool_def_to_gemini(&def);
        assert_eq!(gemini_fn.name, "read_file");
        assert_eq!(gemini_fn.description, "Read a file");
        assert_eq!(gemini_fn.parameters, def.parameters_schema);
    }

    #[test]
    fn gemini_function_declaration_to_canonical_tool_def() {
        let decl = GeminiFunctionDeclaration {
            name: "write_file".into(),
            description: "Write to a file".into(),
            parameters: json!({"type": "object"}),
        };
        let canonical = dialect::tool_def_from_gemini(&decl);
        assert_eq!(canonical.name, "write_file");
        assert_eq!(canonical.description, "Write to a file");
        assert_eq!(canonical.parameters_schema, json!({"type": "object"}));
    }

    #[test]
    fn function_call_in_map_response_produces_tool_call_event() {
        let resp = make_response(
            vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "test"}),
            }],
            Some("STOP"),
        );
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(input, &json!({"q": "test"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn function_response_in_map_response_produces_tool_result_event() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::FunctionResponse {
                        name: "search".into(),
                        response: json!("results"),
                    }],
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
            AgentEventKind::ToolResult {
                tool_name,
                output,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(output, &json!("results"));
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}

// =========================================================================
// Module 3: Streaming (streamGenerateContent) response mapping
// =========================================================================

mod streaming_mapping {
    use super::*;

    #[test]
    fn stream_chunk_text_maps_to_assistant_delta() {
        let chunk = make_stream_chunk(vec![GeminiPart::Text("Hello".into())]);
        let events = dialect::map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    fn stream_chunk_function_call_maps_to_tool_call() {
        let chunk = make_stream_chunk(vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"q": "test"}),
        }]);
        let events = dialect::map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(input, &json!({"q": "test"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn stream_chunk_inline_data_ignored() {
        let chunk = make_stream_chunk(vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "abc123".into(),
        })]);
        let events = dialect::map_stream_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn stream_chunk_multiple_text_parts() {
        let chunk = GeminiStreamChunk {
            candidates: vec![
                GeminiCandidate {
                    content: GeminiContent {
                        role: "model".into(),
                        parts: vec![GeminiPart::Text("part1".into())],
                    },
                    finish_reason: None,
                    safety_ratings: None,
                    citation_metadata: None,
                },
                GeminiCandidate {
                    content: GeminiContent {
                        role: "model".into(),
                        parts: vec![GeminiPart::Text("part2".into())],
                    },
                    finish_reason: None,
                    safety_ratings: None,
                    citation_metadata: None,
                },
            ],
            usage_metadata: None,
        };
        let events = dialect::map_stream_chunk(&chunk);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn stream_chunk_to_shim_stream_event() {
        let chunk = make_stream_chunk(vec![GeminiPart::Text("delta".into())]);
        let event = from_dialect_stream_chunk(&chunk);
        assert_eq!(event.candidates.len(), 1);
        assert_eq!(event.text().unwrap(), "delta");
    }

    #[test]
    fn stream_event_with_usage_metadata() {
        let chunk = GeminiStreamChunk {
            candidates: vec![],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 50,
                candidates_token_count: 100,
                total_token_count: 150,
            }),
        };
        let event = from_dialect_stream_chunk(&chunk);
        let usage = event.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 50);
        assert_eq!(usage.candidates_token_count, 100);
        assert_eq!(usage.total_token_count, 150);
    }

    #[test]
    fn stream_event_without_usage() {
        let chunk = make_stream_chunk(vec![GeminiPart::Text("hi".into())]);
        let event = from_dialect_stream_chunk(&chunk);
        assert!(event.usage_metadata.is_none());
    }

    #[test]
    fn map_stream_event_alias() {
        let chunk = make_stream_chunk(vec![GeminiPart::Text("alias".into())]);
        let events_a = dialect::map_stream_chunk(&chunk);
        let events_b = dialect::map_stream_event(&chunk);
        assert_eq!(events_a.len(), events_b.len());
    }

    #[tokio::test]
    async fn shim_client_generate_stream() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = simple_request("gemini-2.5-flash", "Stream test");
        let stream = client.generate_stream(req).await.unwrap();
        use tokio_stream::StreamExt;
        let events: Vec<StreamEvent> = stream.collect().await;
        // Should have at least a text event and a usage event
        assert!(!events.is_empty());
    }

    #[test]
    fn stream_chunk_function_response_maps_to_tool_result() {
        let chunk = GeminiStreamChunk {
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
            usage_metadata: None,
        };
        let events = dialect::map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolResult { tool_name, .. } => {
                assert_eq!(tool_name, "api");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn empty_stream_chunk_produces_no_events() {
        let chunk = GeminiStreamChunk {
            candidates: vec![],
            usage_metadata: None,
        };
        let events = dialect::map_stream_chunk(&chunk);
        assert!(events.is_empty());
    }
}

// =========================================================================
// Module 4: Model names and capability mapping
// =========================================================================

mod model_capability_mapping {
    use super::*;

    #[test]
    fn canonical_model_name_gemini_25_flash() {
        assert_eq!(
            dialect::to_canonical_model("gemini-2.5-flash"),
            "google/gemini-2.5-flash"
        );
    }

    #[test]
    fn canonical_model_name_gemini_25_pro() {
        assert_eq!(
            dialect::to_canonical_model("gemini-2.5-pro"),
            "google/gemini-2.5-pro"
        );
    }

    #[test]
    fn canonical_model_name_gemini_20_flash() {
        assert_eq!(
            dialect::to_canonical_model("gemini-2.0-flash"),
            "google/gemini-2.0-flash"
        );
    }

    #[test]
    fn canonical_model_name_gemini_20_flash_lite() {
        assert_eq!(
            dialect::to_canonical_model("gemini-2.0-flash-lite"),
            "google/gemini-2.0-flash-lite"
        );
    }

    #[test]
    fn canonical_model_name_gemini_15_flash() {
        assert_eq!(
            dialect::to_canonical_model("gemini-1.5-flash"),
            "google/gemini-1.5-flash"
        );
    }

    #[test]
    fn canonical_model_name_gemini_15_pro() {
        assert_eq!(
            dialect::to_canonical_model("gemini-1.5-pro"),
            "google/gemini-1.5-pro"
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
    fn from_canonical_model_passthrough_unknown() {
        assert_eq!(
            dialect::from_canonical_model("custom-model"),
            "custom-model"
        );
    }

    #[test]
    fn known_model_gemini_25_flash() {
        assert!(dialect::is_known_model("gemini-2.5-flash"));
    }

    #[test]
    fn known_model_gemini_25_pro() {
        assert!(dialect::is_known_model("gemini-2.5-pro"));
    }

    #[test]
    fn known_model_gemini_15_pro() {
        assert!(dialect::is_known_model("gemini-1.5-pro"));
    }

    #[test]
    fn known_model_gemini_15_flash() {
        assert!(dialect::is_known_model("gemini-1.5-flash"));
    }

    #[test]
    fn unknown_model_returns_false() {
        assert!(!dialect::is_known_model("gpt-4"));
        assert!(!dialect::is_known_model("claude-3-opus"));
        assert!(!dialect::is_known_model("gemini-pro"));
    }

    #[test]
    fn capability_manifest_streaming_native() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_tool_read_native() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolRead),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_tool_write_emulated() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolWrite),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn capability_manifest_tool_edit_emulated() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolEdit),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn capability_manifest_tool_bash_emulated() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolBash),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn capability_manifest_structured_output_native() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_glob_unsupported() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolGlob),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn capability_manifest_grep_unsupported() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolGrep),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::McpClient),
            Some(SupportLevel::Unsupported)
        ));
        assert!(matches!(
            caps.get(&Capability::McpServer),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "gemini-2.5-flash");
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "gemini/v0.1");
    }

    #[test]
    fn canonical_roundtrip_all_known_models() {
        for model in &[
            "gemini-2.5-flash",
            "gemini-2.5-pro",
            "gemini-2.0-flash",
            "gemini-2.0-flash-lite",
            "gemini-1.5-flash",
            "gemini-1.5-pro",
        ] {
            let canonical = dialect::to_canonical_model(model);
            let back = dialect::from_canonical_model(&canonical);
            assert_eq!(&back, model, "roundtrip failed for {model}");
        }
    }
}

// =========================================================================
// Module 5: Safety settings mapping
// =========================================================================

mod safety_settings_mapping {
    use super::*;

    #[test]
    fn safety_setting_harassment_block_none() {
        let setting = SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![setting]);
        let dialect_req = to_dialect_request(&req);
        let ss = dialect_req.safety_settings.unwrap();
        assert_eq!(ss.len(), 1);
        assert_eq!(ss[0].category, HarmCategory::HarmCategoryHarassment);
        assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockNone);
    }

    #[test]
    fn safety_setting_hate_speech_block_low_and_above() {
        let setting = SafetySetting {
            category: HarmCategory::HarmCategoryHateSpeech,
            threshold: HarmBlockThreshold::BlockLowAndAbove,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![setting]);
        let dialect_req = to_dialect_request(&req);
        let ss = dialect_req.safety_settings.unwrap();
        assert_eq!(ss[0].category, HarmCategory::HarmCategoryHateSpeech);
        assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockLowAndAbove);
    }

    #[test]
    fn safety_setting_sexually_explicit_block_medium_and_above() {
        let setting = SafetySetting {
            category: HarmCategory::HarmCategorySexuallyExplicit,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![setting]);
        let dialect_req = to_dialect_request(&req);
        let ss = dialect_req.safety_settings.unwrap();
        assert_eq!(ss[0].category, HarmCategory::HarmCategorySexuallyExplicit);
        assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockMediumAndAbove);
    }

    #[test]
    fn safety_setting_dangerous_content_block_only_high() {
        let setting = SafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![setting]);
        let dialect_req = to_dialect_request(&req);
        let ss = dialect_req.safety_settings.unwrap();
        assert_eq!(ss[0].category, HarmCategory::HarmCategoryDangerousContent);
        assert_eq!(ss[0].threshold, HarmBlockThreshold::BlockOnlyHigh);
    }

    #[test]
    fn safety_setting_civic_integrity() {
        let setting = SafetySetting {
            category: HarmCategory::HarmCategoryCivicIntegrity,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![setting]);
        let dialect_req = to_dialect_request(&req);
        let ss = dialect_req.safety_settings.unwrap();
        assert_eq!(ss[0].category, HarmCategory::HarmCategoryCivicIntegrity);
    }

    #[test]
    fn multiple_safety_settings() {
        let settings = vec![
            SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryHateSpeech,
                threshold: HarmBlockThreshold::BlockLowAndAbove,
            },
            SafetySetting {
                category: HarmCategory::HarmCategorySexuallyExplicit,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            },
            SafetySetting {
                category: HarmCategory::HarmCategoryDangerousContent,
                threshold: HarmBlockThreshold::BlockOnlyHigh,
            },
        ];
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(settings);
        let dialect_req = to_dialect_request(&req);
        assert_eq!(dialect_req.safety_settings.unwrap().len(), 4);
    }

    #[test]
    fn no_safety_settings_maps_to_none() {
        let req = simple_request("gemini-2.5-flash", "test");
        let dialect_req = to_dialect_request(&req);
        assert!(dialect_req.safety_settings.is_none());
    }

    #[test]
    fn safety_rating_serde_roundtrip() {
        let rating = GeminiSafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Low,
        };
        let json = serde_json::to_value(&rating).unwrap();
        let back: GeminiSafetyRating = serde_json::from_value(json).unwrap();
        assert_eq!(back.category, HarmCategory::HarmCategoryHarassment);
        assert_eq!(back.probability, HarmProbability::Low);
    }

    #[test]
    fn safety_setting_serde_roundtrip() {
        let setting = GeminiSafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        };
        let json = serde_json::to_string(&setting).unwrap();
        let back: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(back, setting);
    }

    #[test]
    fn harm_probability_all_variants() {
        let variants = [
            HarmProbability::Negligible,
            HarmProbability::Low,
            HarmProbability::Medium,
            HarmProbability::High,
        ];
        for variant in &variants {
            let json = serde_json::to_value(variant).unwrap();
            let back: HarmProbability = serde_json::from_value(json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn harm_block_threshold_all_variants() {
        let variants = [
            HarmBlockThreshold::BlockNone,
            HarmBlockThreshold::BlockLowAndAbove,
            HarmBlockThreshold::BlockMediumAndAbove,
            HarmBlockThreshold::BlockOnlyHigh,
        ];
        for variant in &variants {
            let json = serde_json::to_value(variant).unwrap();
            let back: HarmBlockThreshold = serde_json::from_value(json).unwrap();
            assert_eq!(&back, variant);
        }
    }
}

// =========================================================================
// Module 6: Multi-modal content (text + image parts) handling
// =========================================================================

mod multi_modal_content {
    use super::*;

    #[test]
    fn inline_data_part_to_ir_image() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            })],
        };
        let ir = lowering::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "iVBORw0KGgo=");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn inline_data_roundtrip() {
        let original = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/jpeg".into(),
                data: "base64jpeg".into(),
            })],
        }];
        let ir = lowering::to_ir(&original, None);
        let back = lowering::from_ir(&ir);
        match &back[0].parts[0] {
            GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/jpeg");
                assert_eq!(d.data, "base64jpeg");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    #[test]
    fn text_and_image_mixed_content() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![
                GeminiPart::Text("Describe this image:".into()),
                GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/png".into(),
                    data: "base64data".into(),
                }),
            ],
        };
        let ir = lowering::to_ir(&[content], None);
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
    fn shim_part_text_construction() {
        let part = Part::text("hello");
        match &part {
            Part::Text(t) => assert_eq!(t, "hello"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn shim_part_inline_data_construction() {
        let part = Part::inline_data("image/webp", "d2VicA==");
        match &part {
            Part::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/webp");
                assert_eq!(data, "d2VicA==");
            }
            _ => panic!("expected InlineData"),
        }
    }

    #[test]
    fn shim_part_function_call_construction() {
        let part = Part::function_call("test_fn", json!({"a": 1}));
        match &part {
            Part::FunctionCall { name, args } => {
                assert_eq!(name, "test_fn");
                assert_eq!(args, &json!({"a": 1}));
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn shim_part_function_response_construction() {
        let part = Part::function_response("test_fn", json!("result"));
        match &part {
            Part::FunctionResponse { name, response } => {
                assert_eq!(name, "test_fn");
                assert_eq!(response, &json!("result"));
            }
            _ => panic!("expected FunctionResponse"),
        }
    }

    #[test]
    fn shim_content_user_role() {
        let content = Content::user(vec![Part::text("hi")]);
        assert_eq!(content.role, "user");
    }

    #[test]
    fn shim_content_model_role() {
        let content = Content::model(vec![Part::text("hello")]);
        assert_eq!(content.role, "model");
    }

    #[test]
    fn multi_image_input() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![
                GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/png".into(),
                    data: "img1".into(),
                }),
                GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/jpeg".into(),
                    data: "img2".into(),
                }),
                GeminiPart::Text("Compare these images".into()),
            ],
        };
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].content.len(), 3);
        assert!(
            matches!(&ir.messages[0].content[0], IrContentBlock::Image { media_type, .. } if media_type == "image/png")
        );
        assert!(
            matches!(&ir.messages[0].content[1], IrContentBlock::Image { media_type, .. } if media_type == "image/jpeg")
        );
        assert!(matches!(
            &ir.messages[0].content[2],
            IrContentBlock::Text { .. }
        ));
    }

    #[test]
    fn inline_data_in_map_response_is_ignored() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::InlineData(GeminiInlineData {
                        mime_type: "image/png".into(),
                        data: "base64".into(),
                    })],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let events = dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn inline_data_serde_roundtrip() {
        let data = GeminiInlineData {
            mime_type: "image/gif".into(),
            data: "R0lGODlh".into(),
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: GeminiInlineData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, data);
    }
}

// =========================================================================
// Module 7: candidateCount and candidate selection
// =========================================================================

mod candidate_selection {
    use super::*;

    #[test]
    fn single_candidate_response() {
        let resp = make_response(vec![GeminiPart::Text("answer".into())], Some("STOP"));
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn multiple_candidates_response() {
        let resp = GeminiResponse {
            candidates: vec![
                GeminiCandidate {
                    content: model_text("answer A"),
                    finish_reason: Some("STOP".into()),
                    safety_ratings: None,
                    citation_metadata: None,
                },
                GeminiCandidate {
                    content: model_text("answer B"),
                    finish_reason: Some("STOP".into()),
                    safety_ratings: None,
                    citation_metadata: None,
                },
                GeminiCandidate {
                    content: model_text("answer C"),
                    finish_reason: Some("STOP".into()),
                    safety_ratings: None,
                    citation_metadata: None,
                },
            ],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn from_dialect_response_preserves_all_candidates() {
        let resp = GeminiResponse {
            candidates: vec![
                GeminiCandidate {
                    content: model_text("first"),
                    finish_reason: Some("STOP".into()),
                    safety_ratings: None,
                    citation_metadata: None,
                },
                GeminiCandidate {
                    content: model_text("second"),
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
    }

    #[test]
    fn text_accessor_returns_first_candidate() {
        let resp = GenerateContentResponse {
            candidates: vec![
                Candidate {
                    content: Content::model(vec![Part::text("first")]),
                    finish_reason: Some("STOP".into()),
                },
                Candidate {
                    content: Content::model(vec![Part::text("second")]),
                    finish_reason: Some("STOP".into()),
                },
            ],
            usage_metadata: None,
        };
        assert_eq!(resp.text().unwrap(), "first");
    }

    #[test]
    fn empty_candidates_text_returns_none() {
        let resp = GenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
        };
        assert!(resp.text().is_none());
    }

    #[test]
    fn function_calls_accessor_returns_from_first_candidate() {
        let resp = GenerateContentResponse {
            candidates: vec![
                Candidate {
                    content: Content::model(vec![
                        Part::function_call("fn_a", json!({})),
                        Part::function_call("fn_b", json!({})),
                    ]),
                    finish_reason: None,
                },
                Candidate {
                    content: Content::model(vec![Part::function_call("fn_c", json!({}))]),
                    finish_reason: None,
                },
            ],
            usage_metadata: None,
        };
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "fn_a");
        assert_eq!(calls[1].0, "fn_b");
    }

    #[test]
    fn candidate_finish_reason_preserved() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: model_text("done"),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(
            shim_resp.candidates[0].finish_reason.as_deref(),
            Some("STOP")
        );
    }

    #[test]
    fn candidate_no_finish_reason() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: model_text("partial"),
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert!(shim_resp.candidates[0].finish_reason.is_none());
    }
}

// =========================================================================
// Module 8: Token count mapping
// =========================================================================

mod token_count_mapping {
    use super::*;

    #[test]
    fn usage_metadata_mapped_from_dialect_response() {
        let resp = make_response_with_usage("text", 100, 50);
        let shim_resp = from_dialect_response(&resp);
        let usage = shim_resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 100);
        assert_eq!(usage.candidates_token_count, 50);
        assert_eq!(usage.total_token_count, 150);
    }

    #[test]
    fn usage_metadata_none_when_absent() {
        let resp = make_response(vec![GeminiPart::Text("hi".into())], Some("STOP"));
        let shim_resp = from_dialect_response(&resp);
        assert!(shim_resp.usage_metadata.is_none());
    }

    #[test]
    fn usage_to_ir_conversion() {
        let usage = UsageMetadata {
            prompt_token_count: 200,
            candidates_token_count: 100,
            total_token_count: 300,
        };
        let ir_usage = usage_to_ir(&usage);
        assert_eq!(ir_usage.input_tokens, 200);
        assert_eq!(ir_usage.output_tokens, 100);
        assert_eq!(ir_usage.total_tokens, 300);
    }

    #[test]
    fn usage_from_ir_conversion() {
        let ir_usage = IrUsage::from_io(150, 75);
        let usage = usage_from_ir(&ir_usage);
        assert_eq!(usage.prompt_token_count, 150);
        assert_eq!(usage.candidates_token_count, 75);
        assert_eq!(usage.total_token_count, 225);
    }

    #[test]
    fn usage_roundtrip_ir() {
        let original = UsageMetadata {
            prompt_token_count: 500,
            candidates_token_count: 250,
            total_token_count: 750,
        };
        let ir = usage_to_ir(&original);
        let back = usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, original.prompt_token_count);
        assert_eq!(back.candidates_token_count, original.candidates_token_count);
        assert_eq!(back.total_token_count, original.total_token_count);
    }

    #[test]
    fn usage_zero_tokens() {
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
    fn usage_large_token_counts() {
        let usage = UsageMetadata {
            prompt_token_count: 1_000_000,
            candidates_token_count: 500_000,
            total_token_count: 1_500_000,
        };
        let ir = usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 1_000_000);
        assert_eq!(ir.output_tokens, 500_000);
    }

    #[test]
    fn gemini_usage_metadata_serde_roundtrip() {
        let usage = GeminiUsageMetadata {
            prompt_token_count: 42,
            candidates_token_count: 18,
            total_token_count: 60,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("promptTokenCount"));
        assert!(json.contains("candidatesTokenCount"));
        assert!(json.contains("totalTokenCount"));
        let back: GeminiUsageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.prompt_token_count, 42);
        assert_eq!(back.candidates_token_count, 18);
        assert_eq!(back.total_token_count, 60);
    }

    #[test]
    fn shim_usage_metadata_serde_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: UsageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back, usage);
    }

    #[tokio::test]
    async fn generate_returns_usage_with_correct_sum() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = simple_request("gemini-2.5-flash", "count to 5");
        let resp = client.generate(req).await.unwrap();
        if let Some(usage) = &resp.usage_metadata {
            assert_eq!(
                usage.total_token_count,
                usage.prompt_token_count + usage.candidates_token_count
            );
        }
    }
}

// =========================================================================
// Module 9: finishReason mapping (STOP, MAX_TOKENS, SAFETY, RECITATION)
// =========================================================================

mod finish_reason_mapping {
    use super::*;

    #[test]
    fn finish_reason_stop() {
        let resp = make_response(vec![GeminiPart::Text("done".into())], Some("STOP"));
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(
            shim_resp.candidates[0].finish_reason.as_deref(),
            Some("STOP")
        );
    }

    #[test]
    fn finish_reason_max_tokens() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: model_text("truncated"),
                finish_reason: Some("MAX_TOKENS".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(
            shim_resp.candidates[0].finish_reason.as_deref(),
            Some("MAX_TOKENS")
        );
    }

    #[test]
    fn finish_reason_safety() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: model_text(""),
                finish_reason: Some("SAFETY".into()),
                safety_ratings: Some(vec![GeminiSafetyRating {
                    category: HarmCategory::HarmCategoryHarassment,
                    probability: HarmProbability::High,
                }]),
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(
            shim_resp.candidates[0].finish_reason.as_deref(),
            Some("SAFETY")
        );
    }

    #[test]
    fn finish_reason_recitation() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: model_text("cited content"),
                finish_reason: Some("RECITATION".into()),
                safety_ratings: None,
                citation_metadata: Some(GeminiCitationMetadata {
                    citation_sources: vec![GeminiCitationSource {
                        start_index: Some(0),
                        end_index: Some(10),
                        uri: Some("https://example.com".into()),
                        license: None,
                    }],
                }),
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(
            shim_resp.candidates[0].finish_reason.as_deref(),
            Some("RECITATION")
        );
    }

    #[test]
    fn finish_reason_none_when_absent() {
        let resp = make_response(vec![GeminiPart::Text("partial".into())], None);
        let shim_resp = from_dialect_response(&resp);
        assert!(shim_resp.candidates[0].finish_reason.is_none());
    }

    #[test]
    fn finish_reason_unknown_value_preserved() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: model_text("text"),
                finish_reason: Some("OTHER_REASON".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let shim_resp = from_dialect_response(&resp);
        assert_eq!(
            shim_resp.candidates[0].finish_reason.as_deref(),
            Some("OTHER_REASON")
        );
    }

    #[test]
    fn finish_reason_serde_roundtrip() {
        let candidate = GeminiCandidate {
            content: model_text("test"),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        };
        let json = serde_json::to_string(&candidate).unwrap();
        let back: GeminiCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.finish_reason.as_deref(), Some("STOP"));
    }

    #[test]
    fn finish_reason_null_deserialized_as_none() {
        let json = r#"{"content":{"role":"model","parts":[{"text":"hi"}]},"finish_reason":null}"#;
        let candidate: GeminiCandidate = serde_json::from_str(json).unwrap();
        assert!(candidate.finish_reason.is_none());
    }

    #[test]
    fn stream_chunk_finish_reason_preserved() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: model_text("final"),
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let event = from_dialect_stream_chunk(&chunk);
        assert_eq!(event.candidates[0].finish_reason.as_deref(), Some("STOP"));
    }
}

// =========================================================================
// Module 10: Grounding and citation metadata
// =========================================================================

mod grounding_citation_metadata {
    use super::*;

    #[test]
    fn grounding_config_construction() {
        let config = GeminiGroundingConfig {
            google_search_retrieval: Some(GoogleSearchRetrieval {
                dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".into(),
                    dynamic_threshold: Some(0.7),
                }),
            }),
        };
        let retrieval = config.google_search_retrieval.unwrap();
        let drc = retrieval.dynamic_retrieval_config.unwrap();
        assert_eq!(drc.mode, "MODE_DYNAMIC");
        assert_eq!(drc.dynamic_threshold, Some(0.7));
    }

    #[test]
    fn grounding_config_serde_roundtrip() {
        let config = GeminiGroundingConfig {
            google_search_retrieval: Some(GoogleSearchRetrieval {
                dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".into(),
                    dynamic_threshold: Some(0.5),
                }),
            }),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn grounding_config_without_dynamic_retrieval() {
        let config = GeminiGroundingConfig {
            google_search_retrieval: Some(GoogleSearchRetrieval {
                dynamic_retrieval_config: None,
            }),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
        assert!(
            back.google_search_retrieval
                .unwrap()
                .dynamic_retrieval_config
                .is_none()
        );
    }

    #[test]
    fn grounding_config_empty() {
        let config = GeminiGroundingConfig {
            google_search_retrieval: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
        assert!(back.google_search_retrieval.is_none());
    }

    #[test]
    fn citation_metadata_single_source() {
        let meta = GeminiCitationMetadata {
            citation_sources: vec![GeminiCitationSource {
                start_index: Some(0),
                end_index: Some(50),
                uri: Some("https://example.com/article".into()),
                license: Some("MIT".into()),
            }],
        };
        assert_eq!(meta.citation_sources.len(), 1);
        assert_eq!(
            meta.citation_sources[0].uri.as_deref(),
            Some("https://example.com/article")
        );
        assert_eq!(meta.citation_sources[0].license.as_deref(), Some("MIT"));
    }

    #[test]
    fn citation_metadata_multiple_sources() {
        let meta = GeminiCitationMetadata {
            citation_sources: vec![
                GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(20),
                    uri: Some("https://a.com".into()),
                    license: None,
                },
                GeminiCitationSource {
                    start_index: Some(25),
                    end_index: Some(50),
                    uri: Some("https://b.com".into()),
                    license: Some("Apache-2.0".into()),
                },
            ],
        };
        assert_eq!(meta.citation_sources.len(), 2);
    }

    #[test]
    fn citation_source_serde_roundtrip() {
        let source = GeminiCitationSource {
            start_index: Some(10),
            end_index: Some(30),
            uri: Some("https://example.com".into()),
            license: Some("CC-BY-4.0".into()),
        };
        let json = serde_json::to_string(&source).unwrap();
        let back: GeminiCitationSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }

    #[test]
    fn citation_source_all_optional_fields_none() {
        let source = GeminiCitationSource {
            start_index: None,
            end_index: None,
            uri: None,
            license: None,
        };
        let json = serde_json::to_string(&source).unwrap();
        let back: GeminiCitationSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }

    #[test]
    fn citation_metadata_serde_roundtrip() {
        let meta = GeminiCitationMetadata {
            citation_sources: vec![GeminiCitationSource {
                start_index: Some(0),
                end_index: Some(100),
                uri: Some("https://docs.rs".into()),
                license: None,
            }],
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: GeminiCitationMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back, meta);
    }

    #[test]
    fn candidate_with_citation_metadata() {
        let candidate = GeminiCandidate {
            content: model_text("Rust is a systems language."),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: Some(GeminiCitationMetadata {
                citation_sources: vec![GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(26),
                    uri: Some("https://www.rust-lang.org".into()),
                    license: None,
                }],
            }),
        };
        let json = serde_json::to_string(&candidate).unwrap();
        assert!(json.contains("citation_metadata"));
        let back: GeminiCandidate = serde_json::from_str(&json).unwrap();
        assert!(back.citation_metadata.is_some());
    }

    #[test]
    fn candidate_with_safety_ratings_and_citations() {
        let candidate = GeminiCandidate {
            content: model_text("safe content"),
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
            citation_metadata: Some(GeminiCitationMetadata {
                citation_sources: vec![GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(12),
                    uri: Some("https://source.com".into()),
                    license: None,
                }],
            }),
        };
        let json = serde_json::to_string(&candidate).unwrap();
        let back: GeminiCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.safety_ratings.unwrap().len(), 2);
        assert_eq!(back.citation_metadata.unwrap().citation_sources.len(), 1);
    }

    #[test]
    fn dynamic_retrieval_config_serde_roundtrip() {
        let drc = DynamicRetrievalConfig {
            mode: "MODE_UNSPECIFIED".into(),
            dynamic_threshold: None,
        };
        let json = serde_json::to_string(&drc).unwrap();
        let back: DynamicRetrievalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode, "MODE_UNSPECIFIED");
        assert!(back.dynamic_threshold.is_none());
    }
}

// =========================================================================
// Module 11: Generation config and response format mapping
// =========================================================================

mod generation_config_mapping {
    use super::*;

    #[test]
    fn generation_config_default() {
        let cfg = GenerationConfig::default();
        assert!(cfg.temperature.is_none());
        assert!(cfg.max_output_tokens.is_none());
        assert!(cfg.top_p.is_none());
        assert!(cfg.top_k.is_none());
        assert!(cfg.stop_sequences.is_none());
        assert!(cfg.response_mime_type.is_none());
        assert!(cfg.response_schema.is_none());
    }

    #[test]
    fn generation_config_to_dialect_and_back() {
        let original = GenerationConfig {
            temperature: Some(0.9),
            max_output_tokens: Some(4096),
            top_p: Some(0.95),
            top_k: Some(64),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("text/plain".into()),
            response_schema: None,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(original.clone());
        let dialect_req = to_dialect_request(&req);
        let dialect_cfg = dialect_req.generation_config.unwrap();
        let back = gen_config_from_dialect(&dialect_cfg);
        assert_eq!(back.temperature, original.temperature);
        assert_eq!(back.max_output_tokens, original.max_output_tokens);
        assert_eq!(back.top_p, original.top_p);
        assert_eq!(back.top_k, original.top_k);
        assert_eq!(back.stop_sequences, original.stop_sequences);
        assert_eq!(back.response_mime_type, original.response_mime_type);
    }

    #[test]
    fn generation_config_serde_roundtrip() {
        let cfg = GenerationConfig {
            temperature: Some(0.5),
            max_output_tokens: Some(2048),
            top_p: Some(0.8),
            top_k: Some(32),
            stop_sequences: Some(vec!["STOP".into(), "END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: GenerationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.temperature, cfg.temperature);
        assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    }

    #[test]
    fn gemini_generation_config_default() {
        let cfg = GeminiGenerationConfig::default();
        assert!(cfg.temperature.is_none());
        assert!(cfg.max_output_tokens.is_none());
    }

    #[test]
    fn gemini_config_default_values() {
        let cfg = GeminiConfig::default();
        assert_eq!(cfg.model, "gemini-2.5-flash");
        assert_eq!(
            cfg.base_url,
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(cfg.max_output_tokens, Some(4096));
        assert!(cfg.api_key.is_empty());
    }
}

// =========================================================================
// Module 12: Full pipeline roundtrip tests
// =========================================================================

mod full_pipeline {
    use super::*;

    #[tokio::test]
    async fn simple_text_generation_roundtrip() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = simple_request("gemini-2.5-flash", "Hello");
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
        assert!(resp.text().is_some());
    }

    #[tokio::test]
    async fn multi_turn_generation() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hi")]))
            .add_content(Content::model(vec![Part::text("Hello!")]))
            .add_content(Content::user(vec![Part::text("How are you?")]));
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[tokio::test]
    async fn generation_with_tools() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What's the weather?")]))
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: "Get weather".into(),
                    parameters: json!({"type": "object", "properties": {"loc": {"type": "string"}}}),
                }],
            }]);
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[tokio::test]
    async fn generation_with_system_instruction() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Respond briefly")]))
            .add_content(Content::user(vec![Part::text("What is Rust?")]));
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[test]
    fn client_model_accessor() {
        let client = GeminiClient::new("gemini-1.5-pro");
        assert_eq!(client.model(), "gemini-1.5-pro");
    }

    #[test]
    fn contract_version_available() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn request_builder_chaining() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Q1")]))
            .add_content(Content::model(vec![Part::text("A1")]))
            .add_content(Content::user(vec![Part::text("Q2")]))
            .system_instruction(Content::user(vec![Part::text("Be helpful")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            })
            .safety_settings(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            }])
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "test".into(),
                    description: "a test fn".into(),
                    parameters: json!({}),
                }],
            }])
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            });
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 3);
        assert!(req.system_instruction.is_some());
        assert!(req.generation_config.is_some());
        assert!(req.safety_settings.is_some());
        assert!(req.tools.is_some());
        assert!(req.tool_config.is_some());
    }
}

// =========================================================================
// Module 13: Serde canonical / deterministic JSON
// =========================================================================

mod serde_determinism {
    use super::*;

    #[test]
    fn request_json_deterministic_with_btreemap() {
        let mut vendor: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        vendor.insert("alpha".into(), json!(1));
        vendor.insert("beta".into(), json!(2));
        vendor.insert("gamma".into(), json!(3));
        let json1 = serde_json::to_string(&vendor).unwrap();
        let json2 = serde_json::to_string(&vendor).unwrap();
        assert_eq!(json1, json2);
        // Keys should be alphabetically ordered
        let alpha_pos = json1.find("alpha").unwrap();
        let beta_pos = json1.find("beta").unwrap();
        let gamma_pos = json1.find("gamma").unwrap();
        assert!(alpha_pos < beta_pos);
        assert!(beta_pos < gamma_pos);
    }

    #[test]
    fn gemini_part_text_serde_roundtrip() {
        let part = GeminiPart::Text("hello world".into());
        let json = serde_json::to_string(&part).unwrap();
        let back: GeminiPart = serde_json::from_str(&json).unwrap();
        match back {
            GeminiPart::Text(t) => assert_eq!(t, "hello world"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn gemini_request_serde_roundtrip() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![user_text("hi")],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: GeminiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gemini-2.5-flash");
        assert_eq!(back.contents.len(), 1);
    }

    #[test]
    fn gemini_response_serde_roundtrip() {
        let resp = make_response_with_usage("answer", 10, 20);
        let json = serde_json::to_string(&resp).unwrap();
        let back: GeminiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.candidates.len(), 1);
        assert!(back.usage_metadata.is_some());
    }

    #[test]
    fn gemini_tool_serde_roundtrip() {
        let tool = GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            }],
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: GeminiTool = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tool);
    }

    #[test]
    fn gemini_tool_config_serde_roundtrip() {
        let tc = GeminiToolConfig {
            function_calling_config: GeminiFunctionCallingConfig {
                mode: FunctionCallingMode::Any,
                allowed_function_names: Some(vec!["fn1".into()]),
            },
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tc);
    }

    #[test]
    fn function_calling_mode_serde_all_variants() {
        for mode in [
            FunctionCallingMode::Auto,
            FunctionCallingMode::Any,
            FunctionCallingMode::None,
        ] {
            let json = serde_json::to_value(mode).unwrap();
            let back: FunctionCallingMode = serde_json::from_value(json).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn harm_category_serde_screaming_snake_case() {
        let json = serde_json::to_value(HarmCategory::HarmCategoryHarassment).unwrap();
        assert_eq!(json.as_str().unwrap(), "HARM_CATEGORY_HARASSMENT");
    }

    #[test]
    fn harm_block_threshold_serde_screaming_snake_case() {
        let json = serde_json::to_value(HarmBlockThreshold::BlockMediumAndAbove).unwrap();
        assert_eq!(json.as_str().unwrap(), "BLOCK_MEDIUM_AND_ABOVE");
    }
}

// =========================================================================
// Module 14: Edge cases and error scenarios
// =========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_contents_to_ir() {
        let ir = lowering::to_ir(&[], None);
        assert!(ir.is_empty());
    }

    #[test]
    fn empty_ir_to_contents() {
        let ir = IrConversation::new();
        let back = lowering::from_ir(&ir);
        assert!(back.is_empty());
    }

    #[test]
    fn empty_text_part() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text(String::new())],
        };
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].text_content(), "");
    }

    #[test]
    fn content_with_no_parts() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![],
        };
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].content.len(), 0);
    }

    #[test]
    fn empty_system_instruction_not_added() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![],
        };
        let contents = vec![user_text("hi")];
        let ir = lowering::to_ir(&contents, Some(&sys));
        assert_eq!(ir.len(), 1); // Only the user message
    }

    #[test]
    fn unicode_text_preserved() {
        let text = "こんにちは世界 🌍 émojis and ñ";
        let content = user_text(text);
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].text_content(), text);
        let back = lowering::from_ir(&ir);
        match &back[0].parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, text),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn very_long_text_preserved() {
        let text = "x".repeat(100_000);
        let content = user_text(&text);
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].text_content().len(), 100_000);
    }

    #[test]
    fn function_call_empty_args() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "no_args_fn".into(),
                args: json!({}),
            }],
        };
        let ir = lowering::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert_eq!(input, &json!({}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_response_null_payload() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "void_fn".into(),
                response: json!(null),
            }],
        };
        let ir = lowering::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn text_plus_function_call_mixed() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![
                GeminiPart::Text("Let me search for that.".into()),
                GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "test"}),
                },
            ],
        };
        let ir = lowering::to_ir(&[content], None);
        assert_eq!(ir.messages[0].content.len(), 2);
        assert!(matches!(
            &ir.messages[0].content[0],
            IrContentBlock::Text { .. }
        ));
        assert!(matches!(
            &ir.messages[0].content[1],
            IrContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn ir_system_messages_skipped_in_from_ir() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "system prompt"),
            IrMessage::text(IrRole::User, "user message"),
        ]);
        let back = lowering::from_ir(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn extract_system_instruction() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = lowering::extract_system_instruction(&ir).unwrap();
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn extract_system_instruction_none_when_absent() {
        let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        assert!(lowering::extract_system_instruction(&ir).is_none());
    }

    #[test]
    fn thinking_block_mapped_to_text() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "Let me think...".into(),
            }],
        )]);
        let back = lowering::from_ir(&ir);
        match &back[0].parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Let me think..."),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn ir_tool_role_maps_to_user_in_gemini() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "gemini_search".into(),
                content: vec![IrContentBlock::Text {
                    text: "results".into(),
                }],
                is_error: false,
            }],
        )]);
        let back = lowering::from_ir(&ir);
        assert_eq!(back[0].role, "user");
    }
}
