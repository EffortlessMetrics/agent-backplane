// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Comprehensive tests for gemini-bridge types and translation.

use gemini_bridge::gemini_types::*;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// 1. Serde roundtrip tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn part_text_roundtrip() {
    let part = Part::text("Hello, world!");
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_inline_data_roundtrip() {
    let part = Part::inline_data("image/png", "iVBORw0KGgo=");
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_function_call_roundtrip() {
    let part = Part::function_call("get_weather", json!({"location": "Tokyo"}));
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn part_function_response_roundtrip() {
    let part = Part::function_response("get_weather", json!({"temp": 22}));
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}

#[test]
fn content_user_roundtrip() {
    let content = Content::user(vec![Part::text("Hi")]);
    let json = serde_json::to_string(&content).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(content, back);
}

#[test]
fn content_model_roundtrip() {
    let content = Content::model(vec![Part::text("Hello!")]);
    let json = serde_json::to_string(&content).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(content, back);
}

#[test]
fn content_system_no_role_roundtrip() {
    let content = Content::system(vec![Part::text("You are a helpful assistant.")]);
    let json = serde_json::to_string(&content).unwrap();
    // System content has no role field
    assert!(!json.contains("\"role\""));
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(content, back);
}

#[test]
fn generation_config_roundtrip() {
    let cfg = GenerationConfig {
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        max_output_tokens: Some(1024),
        candidate_count: Some(1),
        stop_sequences: Some(vec!["STOP".into()]),
        response_mime_type: None,
        response_schema: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn generation_config_optional_fields_skipped() {
    let cfg = GenerationConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert_eq!(json, "{}");
}

#[test]
fn safety_setting_roundtrip() {
    let setting = SafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&setting).unwrap();
    let back: SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(setting, back);
}

#[test]
fn safety_rating_roundtrip() {
    let rating = SafetyRating {
        category: HarmCategory::HarmCategoryHateSpeech,
        probability: HarmProbability::Low,
    };
    let json = serde_json::to_string(&rating).unwrap();
    let back: SafetyRating = serde_json::from_str(&json).unwrap();
    assert_eq!(rating, back);
}

#[test]
fn all_harm_categories_roundtrip() {
    let categories = vec![
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in categories {
        let json = serde_json::to_string(&cat).unwrap();
        let back: HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, back);
    }
}

#[test]
fn all_harm_probabilities_roundtrip() {
    let probs = vec![
        HarmProbability::Negligible,
        HarmProbability::Low,
        HarmProbability::Medium,
        HarmProbability::High,
    ];
    for prob in probs {
        let json = serde_json::to_string(&prob).unwrap();
        let back: HarmProbability = serde_json::from_str(&json).unwrap();
        assert_eq!(prob, back);
    }
}

#[test]
fn all_harm_block_thresholds_roundtrip() {
    let thresholds = vec![
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for t in thresholds {
        let json = serde_json::to_string(&t).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}

#[test]
fn function_declaration_roundtrip() {
    let decl = FunctionDeclaration {
        name: "search".into(),
        description: "Search the web".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        }),
    };
    let json = serde_json::to_string(&decl).unwrap();
    let back: FunctionDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(decl, back);
}

#[test]
fn gemini_tool_roundtrip() {
    let tool = GeminiTool {
        function_declarations: vec![FunctionDeclaration {
            name: "get_time".into(),
            description: "Get current time".into(),
            parameters: json!({"type": "object", "properties": {}}),
        }],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn tool_config_roundtrip() {
    let cfg = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: Some(vec!["search".into()]),
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn function_calling_mode_roundtrip() {
    for mode in [
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

#[test]
fn usage_metadata_roundtrip() {
    let usage = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn prompt_feedback_roundtrip() {
    let feedback = PromptFeedback {
        block_reason: Some("SAFETY".into()),
        safety_ratings: Some(vec![SafetyRating {
            category: HarmCategory::HarmCategoryDangerousContent,
            probability: HarmProbability::High,
        }]),
    };
    let json = serde_json::to_string(&feedback).unwrap();
    let back: PromptFeedback = serde_json::from_str(&json).unwrap();
    assert_eq!(feedback, back);
}

#[test]
fn prompt_feedback_empty_roundtrip() {
    let feedback = PromptFeedback {
        block_reason: None,
        safety_ratings: None,
    };
    let json = serde_json::to_string(&feedback).unwrap();
    let back: PromptFeedback = serde_json::from_str(&json).unwrap();
    assert_eq!(feedback, back);
}

#[test]
fn citation_metadata_roundtrip() {
    let meta = CitationMetadata {
        citation_sources: vec![CitationSource {
            start_index: Some(0),
            end_index: Some(100),
            uri: Some("https://example.com".into()),
            license: Some("MIT".into()),
        }],
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: CitationMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}

#[test]
fn citation_source_optional_fields() {
    let source = CitationSource {
        start_index: None,
        end_index: None,
        uri: None,
        license: None,
    };
    let json = serde_json::to_string(&source).unwrap();
    assert_eq!(json, "{}");
    let back: CitationSource = serde_json::from_str(&json).unwrap();
    assert_eq!(source, back);
}

#[test]
fn candidate_roundtrip() {
    let candidate = Candidate {
        content: Content::model(vec![Part::text("Hello!")]),
        finish_reason: Some("STOP".into()),
        safety_ratings: Some(vec![SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Negligible,
        }]),
        citation_metadata: None,
    };
    let json = serde_json::to_string(&candidate).unwrap();
    let back: Candidate = serde_json::from_str(&json).unwrap();
    assert_eq!(candidate, back);
}

#[test]
fn candidate_minimal_roundtrip() {
    let candidate = Candidate {
        content: Content::model(vec![Part::text("ok")]),
        finish_reason: None,
        safety_ratings: None,
        citation_metadata: None,
    };
    let json = serde_json::to_string(&candidate).unwrap();
    let back: Candidate = serde_json::from_str(&json).unwrap();
    assert_eq!(candidate, back);
}

#[test]
fn generate_content_request_roundtrip() {
    let req = GenerateContentRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![Content::user(vec![Part::text("Hello")])],
        tools: None,
        generation_config: Some(GenerationConfig {
            temperature: Some(0.5),
            top_p: None,
            top_k: None,
            max_output_tokens: Some(256),
            candidate_count: None,
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        }),
        safety_settings: None,
        system_instruction: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn generate_content_request_full_roundtrip() {
    let req = GenerateContentRequest {
        model: "gemini-2.5-pro".into(),
        contents: vec![
            Content::user(vec![Part::text("Turn 1")]),
            Content::model(vec![Part::text("Reply 1")]),
            Content::user(vec![Part::text("Turn 2")]),
        ],
        tools: Some(vec![GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({"type": "object"}),
            }],
        }]),
        generation_config: Some(GenerationConfig {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            max_output_tokens: Some(2048),
            candidate_count: Some(1),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: Some("application/json".into()),
            response_schema: Some(json!({"type": "object"})),
        }),
        safety_settings: Some(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }]),
        system_instruction: Some(Content::system(vec![Part::text("Be helpful")])),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn generate_content_response_roundtrip() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("Hello!")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
        prompt_feedback: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn stream_generate_content_response_roundtrip() {
    let chunk = StreamGenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("chunk")]),
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamGenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn error_response_roundtrip() {
    let err = GeminiErrorResponse {
        error: GeminiErrorDetail {
            code: 400,
            message: "Invalid argument".into(),
            status: Some("INVALID_ARGUMENT".into()),
        },
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: GeminiErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn error_response_parse() {
    let body = r#"{"error":{"code":401,"message":"unauthorized","status":"UNAUTHENTICATED"}}"#;
    let parsed = GeminiErrorResponse::parse(body).unwrap();
    assert_eq!(parsed.error.code, 401);
    assert_eq!(parsed.error.message, "unauthorized");
}

#[test]
fn error_response_parse_invalid() {
    assert!(GeminiErrorResponse::parse("not json").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Helper / builder tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn request_builder_pattern() {
    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]))
        .generation_config(GenerationConfig {
            temperature: Some(0.5),
            ..Default::default()
        })
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }])
        .tools(vec![GeminiTool {
            function_declarations: vec![FunctionDeclaration {
                name: "f".into(),
                description: "d".into(),
                parameters: json!({}),
            }],
        }])
        .system_instruction(Content::system(vec![Part::text("System")]));

    assert_eq!(req.model, "gemini-2.5-flash");
    assert_eq!(req.contents.len(), 1);
    assert!(req.generation_config.is_some());
    assert!(req.safety_settings.is_some());
    assert!(req.tools.is_some());
    assert!(req.system_instruction.is_some());
}

#[test]
fn response_text_extraction() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("Hello!")]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("Hello!"));
}

#[test]
fn response_text_empty_candidates() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), None);
}

#[test]
fn response_function_calls_extraction() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![
                Part::function_call("search", json!({"q": "rust"})),
                Part::function_call("fetch", json!({"url": "http://x"})),
            ]),
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "search");
    assert_eq!(calls[1].0, "fetch");
}

#[test]
fn stream_text_extraction() {
    let chunk = StreamGenerateContentResponse {
        candidates: vec![Candidate {
            content: Content::model(vec![Part::text("delta")]),
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(chunk.text(), Some("delta"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Multi-modal content parts
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multimodal_content_roundtrip() {
    let content = Content::user(vec![
        Part::text("What is in this image?"),
        Part::inline_data("image/jpeg", "base64encodeddata"),
    ]);
    let json = serde_json::to_string(&content).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(content, back);
}

#[test]
fn mixed_parts_in_model_response() {
    let content = Content::model(vec![
        Part::text("I found the answer."),
        Part::function_call("lookup", json!({"id": 42})),
    ]);
    let json = serde_json::to_string(&content).unwrap();
    let back: Content = serde_json::from_str(&json).unwrap();
    assert_eq!(content, back);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Safety rating handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn candidate_with_safety_ratings() {
    let candidate = Candidate {
        content: Content::model(vec![Part::text("safe content")]),
        finish_reason: Some("STOP".into()),
        safety_ratings: Some(vec![
            SafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            },
            SafetyRating {
                category: HarmCategory::HarmCategoryHateSpeech,
                probability: HarmProbability::Low,
            },
            SafetyRating {
                category: HarmCategory::HarmCategorySexuallyExplicit,
                probability: HarmProbability::Negligible,
            },
            SafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::Negligible,
            },
        ]),
        citation_metadata: None,
    };
    let json = serde_json::to_string(&candidate).unwrap();
    let back: Candidate = serde_json::from_str(&json).unwrap();
    assert_eq!(candidate, back);
    assert_eq!(back.safety_ratings.unwrap().len(), 4);
}

#[test]
fn response_with_prompt_feedback_blocked() {
    let resp = GenerateContentResponse {
        candidates: vec![],
        usage_metadata: None,
        prompt_feedback: Some(PromptFeedback {
            block_reason: Some("SAFETY".into()),
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::High,
            }]),
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
    assert!(back.candidates.is_empty());
    assert_eq!(
        back.prompt_feedback.unwrap().block_reason,
        Some("SAFETY".into())
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. camelCase wire format tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn generation_config_camel_case() {
    let cfg = GenerationConfig {
        max_output_tokens: Some(100),
        top_p: Some(0.95),
        top_k: None,
        temperature: None,
        candidate_count: None,
        stop_sequences: None,
        response_mime_type: None,
        response_schema: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("maxOutputTokens"));
    assert!(json.contains("topP"));
    assert!(!json.contains("max_output_tokens"));
}

#[test]
fn usage_metadata_camel_case() {
    let usage = UsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 5,
        total_token_count: 15,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(json.contains("promptTokenCount"));
    assert!(json.contains("candidatesTokenCount"));
    assert!(json.contains("totalTokenCount"));
}

#[test]
fn inline_data_camel_case() {
    let data = InlineData {
        mime_type: "image/png".into(),
        data: "abc".into(),
    };
    let json = serde_json::to_string(&data).unwrap();
    assert!(json.contains("mimeType"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Translation tests (feature-gated)
// ═══════════════════════════════════════════════════════════════════════

#[cfg(feature = "normalized")]
mod translation_tests {
    use super::*;
    use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
    use chrono::Utc;
    use gemini_bridge::translate::*;

    fn make_receipt_with_message(text: &str) -> abp_core::Receipt {
        let now = Utc::now();
        ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .started_at(now)
            .finished_at(now)
            .usage(UsageNormalized {
                input_tokens: Some(10),
                output_tokens: Some(20),
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            })
            .add_trace_event(AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantMessage {
                    text: text.to_string(),
                },
                ext: None,
            })
            .build()
    }

    fn make_receipt_with_tool_call() -> abp_core::Receipt {
        let now = Utc::now();
        ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .started_at(now)
            .finished_at(now)
            .usage(UsageNormalized::default())
            .add_trace_event(AgentEvent {
                ts: now,
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: Some("tc_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"query": "rust"}),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: now,
                kind: AgentEventKind::ToolResult {
                    tool_name: "search".into(),
                    tool_use_id: Some("tc_1".into()),
                    output: json!("found results"),
                    is_error: false,
                },
                ext: None,
            })
            .build()
    }

    // ── gemini_to_work_order tests ──────────────────────────────────────

    #[test]
    fn simple_request_to_work_order() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let wo = gemini_to_work_order(&req);
        assert_eq!(wo.task, "Hello");
        assert_eq!(wo.config.model, Some("gemini-2.5-flash".into()));
        assert_eq!(
            wo.config.vendor.get("dialect"),
            Some(&serde_json::Value::String("gemini".into()))
        );
    }

    #[test]
    fn multi_turn_request_to_work_order() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Turn 1")]))
            .add_content(Content::model(vec![Part::text("Reply 1")]))
            .add_content(Content::user(vec![Part::text("Turn 2")]));
        let wo = gemini_to_work_order(&req);
        // Should use the last user text as task
        assert_eq!(wo.task, "Turn 2");
    }

    #[test]
    fn system_instruction_becomes_context_snippet() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::system(vec![Part::text("Be concise")]))
            .add_content(Content::user(vec![Part::text("Hi")]));
        let wo = gemini_to_work_order(&req);
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].name, "system_instruction");
        assert_eq!(wo.context.snippets[0].content, "Be concise");
    }

    #[test]
    fn tools_preserved_in_vendor() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .tools(vec![GeminiTool {
                function_declarations: vec![FunctionDeclaration {
                    name: "f".into(),
                    description: "d".into(),
                    parameters: json!({}),
                }],
            }]);
        let wo = gemini_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn generation_config_preserved_in_vendor() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            });
        let wo = gemini_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("generation_config"));
    }

    #[test]
    fn safety_settings_preserved_in_vendor() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .safety_settings(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            }]);
        let wo = gemini_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("safety_settings"));
    }

    #[test]
    fn empty_request_produces_empty_task() {
        let req = GenerateContentRequest::new("gemini-2.5-flash");
        let wo = gemini_to_work_order(&req);
        assert_eq!(wo.task, "");
    }

    // ── receipt_to_gemini tests ─────────────────────────────────────────

    #[test]
    fn receipt_to_gemini_text() {
        let receipt = make_receipt_with_message("Hello from Gemini");
        let resp = receipt_to_gemini(&receipt);
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.text(), Some("Hello from Gemini"));
        assert_eq!(resp.candidates[0].finish_reason, Some("STOP".into()));
    }

    #[test]
    fn receipt_to_gemini_usage() {
        let receipt = make_receipt_with_message("Hi");
        let resp = receipt_to_gemini(&receipt);
        let usage = resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 10);
        assert_eq!(usage.candidates_token_count, 20);
        assert_eq!(usage.total_token_count, 30);
    }

    #[test]
    fn receipt_to_gemini_tool_call() {
        let receipt = make_receipt_with_tool_call();
        let resp = receipt_to_gemini(&receipt);
        let parts = &resp.candidates[0].content.parts;
        assert_eq!(parts.len(), 2);
        match &parts[0] {
            Part::FunctionCall(fc) => {
                assert_eq!(fc.name, "search");
                assert_eq!(fc.args, json!({"query": "rust"}));
            }
            _ => panic!("expected FunctionCall"),
        }
        match &parts[1] {
            Part::FunctionResponse(fr) => {
                assert_eq!(fr.name, "search");
                assert_eq!(fr.response, json!("found results"));
            }
            _ => panic!("expected FunctionResponse"),
        }
    }

    #[test]
    fn receipt_partial_outcome() {
        let now = Utc::now();
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Partial)
            .started_at(now)
            .finished_at(now)
            .build();
        let resp = receipt_to_gemini(&receipt);
        assert_eq!(
            resp.candidates[0].finish_reason,
            Some("MAX_TOKENS".into())
        );
    }

    #[test]
    fn receipt_failed_outcome() {
        let now = Utc::now();
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Failed)
            .started_at(now)
            .finished_at(now)
            .build();
        let resp = receipt_to_gemini(&receipt);
        assert_eq!(resp.candidates[0].finish_reason, Some("OTHER".into()));
    }

    #[test]
    fn receipt_no_usage() {
        let now = Utc::now();
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .started_at(now)
            .finished_at(now)
            .usage(UsageNormalized::default())
            .build();
        let resp = receipt_to_gemini(&receipt);
        assert!(resp.usage_metadata.is_none());
    }

    // ── agent_event_to_gemini_stream tests ──────────────────────────────

    #[test]
    fn stream_assistant_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "partial".into(),
            },
            ext: None,
        };
        let chunk = agent_event_to_gemini_stream(&event).unwrap();
        assert_eq!(chunk.text(), Some("partial"));
        assert!(chunk.candidates[0].finish_reason.is_none());
    }

    #[test]
    fn stream_assistant_message() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "final".into(),
            },
            ext: None,
        };
        let chunk = agent_event_to_gemini_stream(&event).unwrap();
        assert_eq!(chunk.text(), Some("final"));
        assert_eq!(chunk.candidates[0].finish_reason, Some("STOP".into()));
    }

    #[test]
    fn stream_tool_call() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("tc_1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        };
        let chunk = agent_event_to_gemini_stream(&event).unwrap();
        match &chunk.candidates[0].content.parts[0] {
            Part::FunctionCall(fc) => {
                assert_eq!(fc.name, "search");
            }
            _ => panic!("expected FunctionCall"),
        }
    }

    #[test]
    fn stream_run_completed() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let chunk = agent_event_to_gemini_stream(&event).unwrap();
        assert!(chunk.candidates[0].content.parts.is_empty());
        assert_eq!(chunk.candidates[0].finish_reason, Some("STOP".into()));
    }

    #[test]
    fn stream_run_started_returns_none() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        };
        assert!(agent_event_to_gemini_stream(&event).is_none());
    }

    #[test]
    fn stream_warning_returns_none() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "warn".into(),
            },
            ext: None,
        };
        assert!(agent_event_to_gemini_stream(&event).is_none());
    }

    #[test]
    fn stream_file_changed_returns_none() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "updated".into(),
            },
            ext: None,
        };
        assert!(agent_event_to_gemini_stream(&event).is_none());
    }

    // ── Role mapping tests ──────────────────────────────────────────────

    #[test]
    fn role_mapping_gemini_to_canonical() {
        assert_eq!(gemini_role_to_canonical("model"), "assistant");
        assert_eq!(gemini_role_to_canonical("user"), "user");
        assert_eq!(gemini_role_to_canonical("unknown"), "user");
    }

    #[test]
    fn role_mapping_canonical_to_gemini() {
        assert_eq!(canonical_role_to_gemini("assistant"), "model");
        assert_eq!(canonical_role_to_gemini("model"), "model");
        assert_eq!(canonical_role_to_gemini("user"), "user");
        assert_eq!(canonical_role_to_gemini("system"), "user");
        assert_eq!(canonical_role_to_gemini("unknown"), "user");
    }

    // ── Error translation tests ─────────────────────────────────────────

    #[test]
    fn api_error_auth() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 401,
                message: "invalid key".into(),
                status: None,
            },
        };
        let bridge_err = api_error_to_bridge(&err);
        let msg = format!("{bridge_err}");
        assert!(msg.contains("authentication"));
    }

    #[test]
    fn api_error_invalid_request() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 400,
                message: "bad request".into(),
                status: Some("INVALID_ARGUMENT".into()),
            },
        };
        let bridge_err = api_error_to_bridge(&err);
        let msg = format!("{bridge_err}");
        assert!(msg.contains("invalid request"));
    }

    #[test]
    fn api_error_rate_limited() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 429,
                message: "too many requests".into(),
                status: None,
            },
        };
        let bridge_err = api_error_to_bridge(&err);
        let msg = format!("{bridge_err}");
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn api_error_overloaded() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 503,
                message: "overloaded".into(),
                status: None,
            },
        };
        let bridge_err = api_error_to_bridge(&err);
        let msg = format!("{bridge_err}");
        assert!(msg.contains("overloaded"));
    }

    #[test]
    fn api_error_generic() {
        let err = GeminiErrorResponse {
            error: GeminiErrorDetail {
                code: 500,
                message: "internal".into(),
                status: None,
            },
        };
        let bridge_err = api_error_to_bridge(&err);
        let msg = format!("{bridge_err}");
        assert!(msg.contains("500"));
    }

    // ── task_to_request helper ──────────────────────────────────────────

    #[test]
    fn task_to_request_creates_valid_request() {
        let req = task_to_request("Do something", "gemini-2.5-flash");
        assert_eq!(req.model, "gemini-2.5-flash");
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, Some("user".into()));
        match &req.contents[0].parts[0] {
            Part::Text(t) => assert_eq!(t, "Do something"),
            _ => panic!("expected text part"),
        }
        assert!(req.tools.is_none());
        assert!(req.generation_config.is_none());
        assert!(req.safety_settings.is_none());
        assert!(req.system_instruction.is_none());
    }
}
