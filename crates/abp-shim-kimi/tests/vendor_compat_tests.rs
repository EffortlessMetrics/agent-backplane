// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vendor-compatibility tests for the Kimi shim.

use abp_shim_kimi::types::{
    KimiChatRequest, KimiFileReference, KimiPluginConfig, KimiRequestBuilder, Message, Usage,
};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Message constructors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_constructors_produce_correct_roles() {
    let sys = Message::system("You are helpful");
    assert_eq!(sys.role, "system");
    assert_eq!(sys.content.as_deref(), Some("You are helpful"));

    let user = Message::user("Hello");
    assert_eq!(user.role, "user");

    let asst = Message::assistant("Hi there");
    assert_eq!(asst.role, "assistant");
}

#[test]
fn message_tool_constructor() {
    let msg = Message::tool("call_123", "result value");
    assert_eq!(msg.role, "tool");
    assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
    assert_eq!(msg.content.as_deref(), Some("result value"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request builder pattern
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_builder_produces_valid_request() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![
            Message::system("Be helpful"),
            Message::user("What is Rust?"),
        ])
        .temperature(0.7)
        .max_tokens(2048)
        .build();

    assert_eq!(req.model, "moonshot-v1-8k");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.temperature, Some(0.7));
    assert_eq!(req.max_tokens, Some(2048));
}

#[test]
fn request_builder_with_search() {
    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-32k")
        .messages(vec![Message::user("Search for Rust")])
        .use_search(true)
        .build();

    assert_eq!(req.use_search, Some(true));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Wire-format JSON fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn chat_request_serialises_to_kimi_wire_format() {
    let req = KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![Message::user("Hello")],
        temperature: Some(0.5),
        max_tokens: Some(1024),
        ..Default::default()
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "moonshot-v1-8k");
    assert_eq!(v["messages"][0]["role"], "user");
    assert_eq!(v["temperature"], 0.5);
    // Stream should be omitted when None
    assert!(v.get("stream").is_none());
}

#[test]
fn message_serde_roundtrip() {
    let msg = Message::user("Test message");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Usage statistics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_from_json() {
    let json_str = r#"{
        "prompt_tokens": 50,
        "completion_tokens": 100,
        "total_tokens": 150
    }"#;

    let usage: Usage = serde_json::from_str(json_str).unwrap();
    assert_eq!(usage.prompt_tokens, 50);
    assert_eq!(usage.completion_tokens, 100);
    assert_eq!(usage.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. File reference and plugin config types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn file_reference_serde_roundtrip() {
    let fref = KimiFileReference {
        file_id: "file-abc123".into(),
        filename: Some("report.pdf".into()),
        purpose: Some("file-extract".into()),
    };
    let json = serde_json::to_string(&fref).unwrap();
    let back: KimiFileReference = serde_json::from_str(&json).unwrap();
    assert_eq!(fref, back);
}

#[test]
fn plugin_config_serde_roundtrip() {
    let plugin = KimiPluginConfig {
        plugin_id: "web_search".into(),
        name: Some("Web Search".into()),
        enabled: Some(true),
        settings: Default::default(),
    };
    let json = serde_json::to_string(&plugin).unwrap();
    let back: KimiPluginConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(plugin, back);
}
