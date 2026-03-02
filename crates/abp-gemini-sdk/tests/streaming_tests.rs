// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming and content-mapping tests for the Gemini SDK dialect.

use abp_core::AgentEventKind;
use abp_gemini_sdk::dialect::{
    FunctionCallingMode, GeminiCandidate, GeminiCitationMetadata, GeminiCitationSource,
    GeminiContent, GeminiFunctionCallingConfig, GeminiFunctionDeclaration, GeminiGenerationConfig,
    GeminiInlineData, GeminiPart, GeminiSafetyRating, GeminiSafetySetting, GeminiStreamChunk,
    GeminiTool, GeminiToolConfig, GeminiUsageMetadata, HarmBlockThreshold, HarmCategory,
    HarmProbability, map_stream_chunk, map_stream_event,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn text_chunk(text: &str) -> GeminiStreamChunk {
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

fn fn_call_chunk(name: &str, args: serde_json::Value) -> GeminiStreamChunk {
    GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: name.into(),
                    args,
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    }
}

fn fn_response_chunk(name: &str, response: serde_json::Value) -> GeminiStreamChunk {
    GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: name.into(),
                    response,
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    }
}

// ---------------------------------------------------------------------------
// 1. Serde roundtrips for streaming types
// ---------------------------------------------------------------------------

#[test]
fn stream_chunk_serde_roundtrip_text() {
    let chunk = text_chunk("hello");
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.candidates.len(), 1);
    assert!(matches!(
        &parsed.candidates[0].content.parts[0],
        GeminiPart::Text(t) if t == "hello"
    ));
}

#[test]
fn stream_chunk_serde_roundtrip_function_call() {
    let chunk = fn_call_chunk("read_file", serde_json::json!({"path": "main.rs"}));
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        &parsed.candidates[0].content.parts[0],
        GeminiPart::FunctionCall { name, .. } if name == "read_file"
    ));
}

#[test]
fn stream_chunk_serde_roundtrip_function_response() {
    let chunk = fn_response_chunk("search", serde_json::json!({"results": ["a", "b"]}));
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        &parsed.candidates[0].content.parts[0],
        GeminiPart::FunctionResponse { name, .. } if name == "search"
    ));
}

#[test]
fn stream_chunk_serde_roundtrip_with_usage_metadata() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        }),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    let meta = parsed.usage_metadata.unwrap();
    assert_eq!(meta.total_token_count, 150);
}

#[test]
fn inline_data_serde_roundtrip() {
    let part = GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "abc123==".into(),
    });
    let json = serde_json::to_string(&part).unwrap();
    assert!(json.contains("mimeType"));
    let parsed: GeminiPart = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, GeminiPart::InlineData(d) if d.mime_type == "image/jpeg"));
}

// ---------------------------------------------------------------------------
// 2. Content part → AgentEvent mapping
// ---------------------------------------------------------------------------

#[test]
fn text_part_maps_to_assistant_delta() {
    let events = map_stream_chunk(&text_chunk("token"));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "token"
    ));
}

#[test]
fn empty_text_part_maps_to_empty_delta() {
    let events = map_stream_chunk(&text_chunk(""));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text.is_empty()
    ));
}

#[test]
fn empty_candidates_produce_no_events() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn inline_data_produces_no_events() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/png".into(),
                    data: "AAAA".into(),
                })],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// 3. Function call / response mapping
// ---------------------------------------------------------------------------

#[test]
fn function_call_maps_to_tool_call() {
    let chunk = fn_call_chunk("get_weather", serde_json::json!({"city": "NYC"}));
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "get_weather");
            assert_eq!(input["city"], "NYC");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn function_call_with_empty_args() {
    let chunk = fn_call_chunk("list_files", serde_json::json!({}));
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert!(input.as_object().unwrap().is_empty());
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn function_response_maps_to_tool_result() {
    let chunk = fn_response_chunk("search", serde_json::json!({"found": true}));
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_name,
            output,
            is_error,
            ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(output["found"], true);
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_response_with_null_output() {
    let chunk = fn_response_chunk("cleanup", serde_json::Value::Null);
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult { output, .. } => {
            assert!(output.is_null());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. Safety setting configuration
// ---------------------------------------------------------------------------

#[test]
fn safety_setting_all_categories_roundtrip() {
    let categories = [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in &categories {
        let s = GeminiSafetySetting {
            category: *cat,
            threshold: HarmBlockThreshold::BlockNone,
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.category, *cat);
    }
}

#[test]
fn safety_setting_all_thresholds_roundtrip() {
    let thresholds = [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for thr in &thresholds {
        let s = GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: *thr,
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.threshold, *thr);
    }
}

#[test]
fn safety_rating_all_probabilities_roundtrip() {
    let probs = [
        HarmProbability::Negligible,
        HarmProbability::Low,
        HarmProbability::Medium,
        HarmProbability::High,
    ];
    for p in &probs {
        let r = GeminiSafetyRating {
            category: HarmCategory::HarmCategoryHateSpeech,
            probability: *p,
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: GeminiSafetyRating = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.probability, *p);
    }
}

// ---------------------------------------------------------------------------
// 5. Generation config
// ---------------------------------------------------------------------------

#[test]
fn generation_config_defaults_all_none() {
    let cfg = GeminiGenerationConfig::default();
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.stop_sequences.is_none());
    assert!(cfg.response_mime_type.is_none());
    assert!(cfg.response_schema.is_none());
}

#[test]
fn generation_config_stop_sequences_roundtrip() {
    let cfg = GeminiGenerationConfig {
        stop_sequences: Some(vec!["END".into(), "STOP".into()]),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("stopSequences"));
    let parsed: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    let seqs = parsed.stop_sequences.unwrap();
    assert_eq!(seqs.len(), 2);
    assert_eq!(seqs[0], "END");
}

#[test]
fn generation_config_full_roundtrip() {
    let cfg = GeminiGenerationConfig {
        max_output_tokens: Some(4096),
        temperature: Some(0.5),
        top_p: Some(0.9),
        top_k: Some(32),
        stop_sequences: Some(vec!["###".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(serde_json::json!({"type": "object"})),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.max_output_tokens, Some(4096));
    assert_eq!(parsed.top_k, Some(32));
    assert_eq!(parsed.stop_sequences.as_ref().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// 6. Multi-part content handling
// ---------------------------------------------------------------------------

#[test]
fn multi_part_text_and_function_call_chunk() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![
                    GeminiPart::Text("Let me look that up.".into()),
                    GeminiPart::FunctionCall {
                        name: "web_search".into(),
                        args: serde_json::json!({"q": "rust traits"}),
                    },
                ],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn multi_candidate_chunk_maps_all() {
    let chunk = GeminiStreamChunk {
        candidates: vec![
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("candidate 1".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            },
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("candidate 2".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            },
        ],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 2);
}

#[test]
fn mixed_parts_function_call_response_and_text() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![
                    GeminiPart::FunctionCall {
                        name: "read_file".into(),
                        args: serde_json::json!({"path": "lib.rs"}),
                    },
                    GeminiPart::FunctionResponse {
                        name: "read_file".into(),
                        response: serde_json::json!({"content": "fn main() {}"}),
                    },
                    GeminiPart::Text("I see the file.".into()),
                ],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
}

// ---------------------------------------------------------------------------
// 7. map_stream_event alias
// ---------------------------------------------------------------------------

#[test]
fn map_stream_event_is_alias_for_map_stream_chunk() {
    let chunk = text_chunk("alias test");
    let via_chunk = map_stream_chunk(&chunk);
    let via_event = map_stream_event(&chunk);
    assert_eq!(via_chunk.len(), via_event.len());
    // Both should produce equivalent events.
    for (a, b) in via_chunk.iter().zip(via_event.iter()) {
        assert_eq!(
            serde_json::to_value(&a.kind).unwrap(),
            serde_json::to_value(&b.kind).unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Tool definition types roundtrip
// ---------------------------------------------------------------------------

#[test]
fn gemini_tool_multiple_declarations_roundtrip() {
    let tool = GeminiTool {
        function_declarations: vec![
            GeminiFunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            },
            GeminiFunctionDeclaration {
                name: "read".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        ],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.function_declarations.len(), 2);
    assert_eq!(parsed.function_declarations[0].name, "search");
    assert_eq!(parsed.function_declarations[1].name, "read");
}

#[test]
fn tool_config_none_mode_roundtrip() {
    let cfg = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::None,
            allowed_function_names: None,
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("NONE"));
    let parsed: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.function_calling_config.mode,
        FunctionCallingMode::None
    );
}

// ---------------------------------------------------------------------------
// 9. Citation metadata in stream chunks
// ---------------------------------------------------------------------------

#[test]
fn stream_chunk_with_citations_still_maps_text() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("cited content".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
            citation_metadata: Some(GeminiCitationMetadata {
                citation_sources: vec![GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(5),
                    uri: Some("https://example.com".into()),
                    license: Some("MIT".into()),
                }],
            }),
        }],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "cited content"
    ));
}

// ---------------------------------------------------------------------------
// 10. Finish reason doesn't affect event generation
// ---------------------------------------------------------------------------

#[test]
fn finish_reason_stop_still_produces_events() {
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
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 1,
            total_token_count: 11,
        }),
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
}
