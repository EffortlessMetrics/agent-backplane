// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vendor-compatibility tests for the Copilot shim.

use abp_shim_copilot::types::{
    CopilotChatRequest, CopilotChatResponse, CopilotDocContext, CopilotIntent,
    CopilotLocalStreamEvent, CopilotSkill, SelectionRange,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Intent type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn intent_display_matches_slash_commands() {
    assert_eq!(CopilotIntent::Explain.to_string(), "explain");
    assert_eq!(CopilotIntent::Generate.to_string(), "generate");
    assert_eq!(CopilotIntent::Fix.to_string(), "fix");
    assert_eq!(CopilotIntent::Test.to_string(), "test");
    assert_eq!(
        CopilotIntent::Custom("refactor".into()).to_string(),
        "refactor"
    );
}

#[test]
fn intent_serde_roundtrip() {
    let intents = vec![
        CopilotIntent::Explain,
        CopilotIntent::Generate,
        CopilotIntent::Fix,
        CopilotIntent::Test,
        CopilotIntent::Custom("docs".into()),
    ];
    for intent in intents {
        let json = serde_json::to_string(&intent).unwrap();
        let back: CopilotIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(intent, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Doc context wire format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn doc_context_serialises_correctly() {
    let ctx = CopilotDocContext {
        uri: "file:///src/main.rs".into(),
        language: Some("rust".into()),
        cursor_line: Some(42),
        cursor_column: Some(10),
        selection: Some(SelectionRange {
            start_line: 40,
            start_column: 0,
            end_line: 45,
            end_column: 0,
        }),
        content: Some("fn main() {}".into()),
    };

    let v = serde_json::to_value(&ctx).unwrap();
    assert_eq!(v["uri"], "file:///src/main.rs");
    assert_eq!(v["language"], "rust");
    assert_eq!(v["cursor_line"], 42);
    assert!(v["selection"].is_object());
}

#[test]
fn doc_context_omits_optional_fields() {
    let ctx = CopilotDocContext {
        uri: "file:///test.py".into(),
        language: None,
        cursor_line: None,
        cursor_column: None,
        selection: None,
        content: None,
    };

    let v = serde_json::to_value(&ctx).unwrap();
    assert_eq!(v["uri"], "file:///test.py");
    assert!(v.get("language").is_none());
    assert!(v.get("selection").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Skill type roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn skill_serde_roundtrip() {
    let skill = CopilotSkill {
        id: "code-review".into(),
        name: "Code Review".into(),
        description: Some("Reviews code for bugs".into()),
        version: Some("1.0.0".into()),
    };

    let json = serde_json::to_string(&skill).unwrap();
    let back: CopilotSkill = serde_json::from_str(&json).unwrap();
    assert_eq!(skill, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Chat request/response wire format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chat_request_serialises_with_copilot_extensions() {
    use abp_copilot_sdk::dialect::CopilotMessage;

    let req = CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "Explain this code".into(),
        }],
        intent: Some(CopilotIntent::Explain),
        doc_context: Some(CopilotDocContext {
            uri: "file:///main.rs".into(),
            language: Some("rust".into()),
            cursor_line: None,
            cursor_column: None,
            selection: None,
            content: None,
        }),
        skills: None,
        temperature: Some(0.3),
        max_tokens: Some(2048),
        stream: None,
        metadata: Default::default(),
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "gpt-4o");
    assert!(v.get("intent").is_some());
    assert!(v.get("doc_context").is_some());
}
