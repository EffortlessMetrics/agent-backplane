// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vendor-compatibility tests for the Gemini shim.

use abp_shim_gemini::types::{
    Content, GenerateContentRequest, GenerateContentResponse, Part, SafetySetting,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Wire-format JSON fidelity — request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_serialises_to_gemini_wire_format() {
    let req = GenerateContentRequest {
        model: "gemini-2.0-flash".into(),
        contents: vec![Content {
            role: "user".into(),
            parts: vec![Part::text("Hello")],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        safety_settings: None,
        generation_config: None,
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "gemini-2.0-flash");
    assert_eq!(v["contents"][0]["role"], "user");
}

#[test]
fn request_with_safety_settings_json() {
    let req = GenerateContentRequest {
        model: "gemini-2.0-flash".into(),
        contents: vec![Content {
            role: "user".into(),
            parts: vec![Part::text("test")],
        }],
        system_instruction: None,
        tools: None,
        tool_config: None,
        safety_settings: Some(vec![SafetySetting {
            category: abp_shim_gemini::HarmCategory::HarmCategoryHarassment,
            threshold: abp_shim_gemini::HarmBlockThreshold::BlockMediumAndAbove,
        }]),
        generation_config: None,
    };

    let v = serde_json::to_value(&req).unwrap();
    assert!(v["safetySettings"].is_array());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Wire-format JSON fidelity — response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_from_real_gemini_json() {
    let json_str = r#"{
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "Hello! How can I help you today?"}]
            },
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 5,
            "candidatesTokenCount": 8,
            "totalTokenCount": 13
        }
    }"#;

    let resp: GenerateContentResponse = serde_json::from_str(json_str).unwrap();
    assert!(!resp.candidates.is_empty());
    let candidate = &resp.candidates[0];
    assert_eq!(candidate.finish_reason.as_deref(), Some("STOP"));
    assert_eq!(candidate.content.role, "model");
}

#[test]
fn response_with_function_call() {
    let json_str = r#"{
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "NYC"}
                    }
                }]
            },
            "finishReason": "STOP"
        }]
    }"#;

    let resp: GenerateContentResponse = serde_json::from_str(json_str).unwrap();
    let parts = &resp.candidates[0].content.parts;
    match &parts[0] {
        Part::FunctionCall { name, args } => {
            assert_eq!(name, "get_weather");
            assert_eq!(args["location"], "NYC");
        }
        _ => panic!("expected FunctionCall part"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Part type constructors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn part_text_constructor() {
    let part = Part::text("Hello world");
    match part {
        Part::Text(t) => assert_eq!(t, "Hello world"),
        _ => panic!("expected Text"),
    }
}

#[test]
fn part_inline_data_constructor() {
    let part = Part::inline_data("image/png", "base64data");
    match part {
        Part::InlineData { mime_type, data } => {
            assert_eq!(mime_type, "image/png");
            assert_eq!(data, "base64data");
        }
        _ => panic!("expected InlineData"),
    }
}

#[test]
fn part_function_call_roundtrip() {
    let part = Part::function_call("search", json!({"q": "rust"}));
    let json = serde_json::to_string(&part).unwrap();
    let back: Part = serde_json::from_str(&json).unwrap();
    assert_eq!(part, back);
}
