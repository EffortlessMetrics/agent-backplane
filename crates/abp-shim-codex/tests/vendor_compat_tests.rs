// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vendor-compatibility tests for the Codex shim.

use abp_shim_codex::types::{CodexContextItem, CodexExtendedRequest, CodexSandboxConfig};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Extended request wire format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extended_request_serialises_to_codex_wire_format() {
    use abp_codex_sdk::dialect::CodexInputItem;

    let req = CodexExtendedRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Write a function".into(),
        }],
        instructions: Some("You are a coding assistant.".into()),
        context: vec![CodexContextItem {
            path: "src/main.rs".into(),
            content: Some("fn main() {}".into()),
        }],
        max_output_tokens: Some(4096),
        temperature: Some(0.2),
        tools: vec![],
        text: None,
        sandbox: None,
        metadata: Default::default(),
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "codex-mini-latest");
    assert_eq!(v["instructions"], "You are a coding assistant.");
    assert_eq!(v["context"][0]["path"], "src/main.rs");
    assert_eq!(v["max_output_tokens"], 4096);
    // Empty tools should be omitted
    assert!(v.get("tools").is_none());
}

#[test]
fn extended_request_with_sandbox_config() {
    use abp_codex_sdk::dialect::CodexInputItem;

    let req = CodexExtendedRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "test".into(),
        }],
        instructions: None,
        context: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
        sandbox: Some(CodexSandboxConfig {
            sandbox_type: "docker".into(),
            image: Some("node:20".into()),
            timeout_secs: Some(300),
            env: Default::default(),
        }),
        metadata: Default::default(),
    };

    let v = serde_json::to_value(&req).unwrap();
    assert!(v["sandbox"].is_object());
    assert_eq!(v["sandbox"]["image"], "node:20");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Base request conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extended_request_converts_to_base() {
    use abp_codex_sdk::dialect::CodexInputItem;

    let req = CodexExtendedRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }],
        instructions: Some("Be helpful".into()),
        context: vec![],
        max_output_tokens: Some(1024),
        temperature: None,
        tools: vec![],
        text: None,
        sandbox: None,
        metadata: Default::default(),
    };

    let base = req.to_base_request();
    assert_eq!(base.model, "codex-mini-latest");
    // Instructions should be prepended as system message
    assert!(base.input.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Context item roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn context_item_serde_roundtrip() {
    let item = CodexContextItem {
        path: "src/lib.rs".into(),
        content: Some("pub fn hello() {}".into()),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexContextItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn context_item_without_content_omits_field() {
    let item = CodexContextItem {
        path: "Cargo.toml".into(),
        content: None,
    };
    let v = serde_json::to_value(&item).unwrap();
    assert!(v.get("content").is_none());
}
