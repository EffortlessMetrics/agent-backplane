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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `abp_gemini_sdk::types` — Gemini GenerateContent API types.

use abp_gemini_sdk::types::*;
use serde_json::json;

// ===== helpers =====

fn user_text(s: &str) -> Content {
    Content {
        role: Some("user".into()),
        parts: vec![Part::Text(s.into())],
    }
}

fn model_text(s: &str) -> Content {
    Content {
        role: Some("model".into()),
        parts: vec![Part::Text(s.into())],
    }
}

fn minimal_request() -> GenerateContentRequest {
    GenerateContentRequest {
        contents: vec![user_text("hello")],
        system_instruction: None,
        tools: None,
        tool_config: None,
        generation_config: None,
        safety_settings: None,
    }
}

fn minimal_response() -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: vec![Candidate {
            content: model_text("hi"),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    }
}

// ===== Part serialization =====

#[test]
fn part_text_roundtrip() {
    let part = Part::Text("hello world".into());
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(json, json!({"text": "hello world"}));
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(back, part);
}

#[test]
fn part_inline_data_roundtrip() {
    let part = Part::InlineData {
        mime_type: "image/png".into(),
        data: "iVBOR...".into(),
    };
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(
        json,
        json!({"inlineData": {"mimeType": "image/png", "data": "iVBOR..."}})
    );
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(back, part);
}

#[test]
fn part_function_call_roundtrip() {
    let part = Part::FunctionCall {
        name: "get_weather".into(),
        args: json!({"city": "London"}),
    };
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(
        json,
        json!({"functionCall": {"name": "get_weather", "args": {"city": "London"}}})
    );
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(back, part);
}

#[test]
fn part_function_response_roundtrip() {
    let part = Part::FunctionResponse {
        name: "get_weather".into(),
        response: json!({"temp": 15}),
    };
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(
        json,
        json!({"functionResponse": {"name": "get_weather", "response": {"temp": 15}}})
    );
    let back: Part = serde_json::from_value(json).unwrap();
    assert_eq!(back, part);
}

// ===== Content serialization =====

#[test]
fn content_with_role_roundtrip() {
    let c = user_text("test");
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["parts"][0]["text"], "test");
    let back: Content = serde_json::from_value(json).unwrap();
    assert_eq!(back, c);
}

#[test]
fn content_without_role_roundtrip() {
    let c = Content {
        role: None,
        parts: vec![Part::Text("system prompt".into())],
    };
    let json = serde_json::to_value(&c).unwrap();
    assert!(json.get("role").is_none(), "role should be omitted");
    let back: Content = serde_json::from_value(json).unwrap();
    assert_eq!(back, c);
}

#[test]
fn content_multiple_parts() {
    let c = Content {
        role: Some("model".into()),
        parts: vec![
            Part::Text("Here's the image:".into()),
            Part::InlineData {
                mime_type: "image/jpeg".into(),
                data: "abc123".into(),
            },
        ],
    };
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["parts"].as_array().unwrap().len(), 2);
    let back: Content = serde_json::from_value(json).unwrap();
    assert_eq!(back, c);
}

// ===== GenerateContentRequest serialization =====

#[test]
fn minimal_request_roundtrip() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.contents.len(), 1);
    assert!(back.system_instruction.is_none());
    assert!(back.tools.is_none());
    assert!(back.tool_config.is_none());
    assert!(back.generation_config.is_none());
    assert!(back.safety_settings.is_none());
}

#[test]
fn request_optional_fields_omitted_in_json() {
    let req = minimal_request();
    let val = serde_json::to_value(&req).unwrap();
    assert!(val.get("systemInstruction").is_none());
    assert!(val.get("tools").is_none());
    assert!(val.get("toolConfig").is_none());
    assert!(val.get("generationConfig").is_none());
    assert!(val.get("safetySettings").is_none());
}

#[test]
fn request_with_system_instruction() {
    let mut req = minimal_request();
    req.system_instruction = Some(Content {
        role: None,
        parts: vec![Part::Text("You are helpful.".into())],
    });
    let val = serde_json::to_value(&req).unwrap();
    assert!(val.get("systemInstruction").is_some());
    let back: GenerateContentRequest = serde_json::from_value(val).unwrap();
    assert!(back.system_instruction.is_some());
}

#[test]
fn request_with_tools() {
    let tool = GeminiTool {
        function_declarations: vec![FunctionDeclaration {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }],
    };
    let mut req = minimal_request();
    req.tools = Some(vec![tool.clone()]);
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tools.unwrap(), vec![tool]);
}

#[test]
fn request_with_tool_config() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into()]),
        },
    };
    let mut req = minimal_request();
    req.tool_config = Some(tc.clone());
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_config.unwrap(), tc);
}

#[test]
fn request_with_generation_config() {
    let gc = GenerationConfig {
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        max_output_tokens: Some(1024),
        candidate_count: Some(1),
        stop_sequences: Some(vec!["END".into()]),
    };
    let mut req = minimal_request();
    req.generation_config = Some(gc.clone());
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["generationConfig"]["temperature"], 0.7);
    assert_eq!(val["generationConfig"]["topP"], 0.9);
    assert_eq!(val["generationConfig"]["topK"], 40);
    assert_eq!(val["generationConfig"]["maxOutputTokens"], 1024);
    assert_eq!(val["generationConfig"]["candidateCount"], 1);
    assert_eq!(val["generationConfig"]["stopSequences"][0], "END");
    let back: GenerateContentRequest = serde_json::from_value(val).unwrap();
    assert_eq!(back.generation_config.unwrap(), gc);
}

#[test]
fn request_with_safety_settings() {
    let ss = vec![SafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    }];
    let mut req = minimal_request();
    req.safety_settings = Some(ss);
    let json = serde_json::to_string(&req).unwrap();
    let back: GenerateContentRequest = serde_json::from_str(&json).unwrap();
    let settings = back.safety_settings.unwrap();
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].category, HarmCategory::HarmCategoryHarassment);
    assert_eq!(
        settings[0].threshold,
        HarmBlockThreshold::BlockMediumAndAbove
    );
}

// ===== GenerateContentResponse serialization =====

#[test]
fn minimal_response_roundtrip() {
    let resp = minimal_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.candidates.len(), 1);
    assert_eq!(back.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn response_with_usage_metadata() {
    let mut resp = minimal_response();
    resp.usage_metadata = Some(UsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 25,
        total_token_count: 35,
    });
    let val = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["usageMetadata"]["promptTokenCount"], 10);
    assert_eq!(val["usageMetadata"]["candidatesTokenCount"], 25);
    assert_eq!(val["usageMetadata"]["totalTokenCount"], 35);
    let back: GenerateContentResponse = serde_json::from_value(val).unwrap();
    let um = back.usage_metadata.unwrap();
    assert_eq!(um.prompt_token_count, 10);
    assert_eq!(um.candidates_token_count, 25);
    assert_eq!(um.total_token_count, 35);
}

#[test]
fn response_with_prompt_feedback() {
    let mut resp = minimal_response();
    resp.prompt_feedback = Some(PromptFeedback {
        block_reason: Some("SAFETY".into()),
        safety_ratings: Some(vec![SafetyRating {
            category: HarmCategory::HarmCategoryHateSpeech,
            probability: HarmProbability::High,
        }]),
    });
    let val = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["promptFeedback"]["blockReason"], "SAFETY");
    let back: GenerateContentResponse = serde_json::from_value(val).unwrap();
    let pf = back.prompt_feedback.unwrap();
    assert_eq!(pf.block_reason.as_deref(), Some("SAFETY"));
    let ratings = pf.safety_ratings.unwrap();
    assert_eq!(ratings[0].category, HarmCategory::HarmCategoryHateSpeech);
    assert_eq!(ratings[0].probability, HarmProbability::High);
}

#[test]
fn response_candidate_with_safety_ratings() {
    let resp = GenerateContentResponse {
        candidates: vec![Candidate {
            content: model_text("ok"),
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::Negligible,
            }]),
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GenerateContentResponse = serde_json::from_str(&json).unwrap();
    let sr = back.candidates[0].safety_ratings.as_ref().unwrap();
    assert_eq!(sr[0].category, HarmCategory::HarmCategoryDangerousContent);
    assert_eq!(sr[0].probability, HarmProbability::Negligible);
}

// ===== Enum variant serde =====

#[test]
fn harm_category_serde() {
    let cat = HarmCategory::HarmCategorySexuallyExplicit;
    let json = serde_json::to_value(cat).unwrap();
    assert_eq!(json, "HARM_CATEGORY_SEXUALLY_EXPLICIT");
    let back: HarmCategory = serde_json::from_value(json).unwrap();
    assert_eq!(back, cat);
}

#[test]
fn harm_block_threshold_serde() {
    let t = HarmBlockThreshold::BlockNone;
    let json = serde_json::to_value(t).unwrap();
    assert_eq!(json, "BLOCK_NONE");
    let back: HarmBlockThreshold = serde_json::from_value(json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn harm_probability_serde() {
    for (variant, expected) in [
        (HarmProbability::Negligible, "NEGLIGIBLE"),
        (HarmProbability::Low, "LOW"),
        (HarmProbability::Medium, "MEDIUM"),
        (HarmProbability::High, "HIGH"),
    ] {
        let json = serde_json::to_value(variant).unwrap();
        assert_eq!(json, expected);
    }
}

#[test]
fn function_calling_mode_serde() {
    for (variant, expected) in [
        (FunctionCallingMode::Auto, "AUTO"),
        (FunctionCallingMode::Any, "ANY"),
        (FunctionCallingMode::None, "NONE"),
    ] {
        let json = serde_json::to_value(variant).unwrap();
        assert_eq!(json, expected);
    }
}

// ===== GenerationConfig =====

#[test]
fn generation_config_default_is_all_none() {
    let gc = GenerationConfig::default();
    let val = serde_json::to_value(&gc).unwrap();
    let obj = val.as_object().unwrap();
    assert!(obj.is_empty(), "default config should serialize empty");
}

#[test]
fn generation_config_partial_fields() {
    let gc = GenerationConfig {
        temperature: Some(1.0),
        max_output_tokens: Some(256),
        ..Default::default()
    };
    let val = serde_json::to_value(&gc).unwrap();
    assert_eq!(val["temperature"], 1.0);
    assert_eq!(val["maxOutputTokens"], 256);
    assert!(val.get("topP").is_none());
    assert!(val.get("topK").is_none());
}

// ===== Deserialization from realistic API JSON =====

#[test]
fn deserialize_real_api_response() {
    let json_str = r#"{
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "The answer is 42."}]
            },
            "finishReason": "STOP",
            "safetyRatings": [
                {"category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE"},
                {"category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE"}
            ]
        }],
        "usageMetadata": {
            "promptTokenCount": 5,
            "candidatesTokenCount": 10,
            "totalTokenCount": 15
        }
    }"#;
    let resp: GenerateContentResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(resp.candidates.len(), 1);
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    let um = resp.usage_metadata.unwrap();
    assert_eq!(um.total_token_count, 15);
}

#[test]
fn deserialize_function_call_response() {
    let json_str = r#"{
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "Paris", "unit": "celsius"}
                    }
                }]
            },
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 20,
            "candidatesTokenCount": 8,
            "totalTokenCount": 28
        }
    }"#;
    let resp: GenerateContentResponse = serde_json::from_str(json_str).unwrap();
    match &resp.candidates[0].content.parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "get_weather");
            assert_eq!(args["location"], "Paris");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn deserialize_blocked_prompt() {
    let json_str = r#"{
        "candidates": [],
        "promptFeedback": {
            "blockReason": "SAFETY",
            "safetyRatings": [
                {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "HIGH"}
            ]
        }
    }"#;
    let resp: GenerateContentResponse = serde_json::from_str(json_str).unwrap();
    assert!(resp.candidates.is_empty());
    let pf = resp.prompt_feedback.unwrap();
    assert_eq!(pf.block_reason.as_deref(), Some("SAFETY"));
}

// ===== FunctionDeclaration / GeminiTool =====

#[test]
fn function_declaration_roundtrip() {
    let fd = FunctionDeclaration {
        name: "calculate".into(),
        description: "Evaluate a math expression".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "expression": {"type": "string"}
            },
            "required": ["expression"]
        }),
    };
    let json = serde_json::to_string(&fd).unwrap();
    let back: FunctionDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(back, fd);
}

#[test]
fn tool_config_roundtrip() {
    let tc = ToolConfig {
        function_calling_config: FunctionCallingConfig {
            mode: FunctionCallingMode::None,
            allowed_function_names: None,
        },
    };
    let val = serde_json::to_value(&tc).unwrap();
    assert_eq!(val["functionCallingConfig"]["mode"], "NONE");
    assert!(val["functionCallingConfig"]
        .get("allowedFunctionNames")
        .is_none());
    let back: ToolConfig = serde_json::from_value(val).unwrap();
    assert_eq!(back, tc);
}
