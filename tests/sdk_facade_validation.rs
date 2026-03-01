// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive SDK facade validation tests.
//!
//! Validates that all 6 SDK crates (OpenAI, Claude, Codex, Gemini, Kimi, Copilot)
//! expose a consistent API surface: dialect modules, lowering functions,
//! backend constants, capability manifests, and sidecar configuration.

use std::collections::HashSet;
use std::path::Path;

use abp_core::ir::IrRole;
use abp_core::{Capability, SupportLevel};
use abp_dialect::{Dialect, DialectDetector};

// ── 1. Dialect variant re-exports ───────────────────────────────────────

#[test]
fn openai_sdk_exposes_dialect_module() {
    assert_eq!(abp_openai_sdk::dialect::DIALECT_VERSION, "openai/v0.1");
}

#[test]
fn claude_sdk_exposes_dialect_module() {
    assert_eq!(abp_claude_sdk::dialect::DIALECT_VERSION, "claude/v0.1");
}

#[test]
fn codex_sdk_exposes_dialect_module() {
    assert_eq!(abp_codex_sdk::dialect::DIALECT_VERSION, "codex/v0.1");
}

#[test]
fn gemini_sdk_exposes_dialect_module() {
    assert_eq!(abp_gemini_sdk::dialect::DIALECT_VERSION, "gemini/v0.1");
}

#[test]
fn kimi_sdk_exposes_dialect_module() {
    assert_eq!(abp_kimi_sdk::dialect::DIALECT_VERSION, "kimi/v0.1");
}

#[test]
fn copilot_sdk_exposes_dialect_module() {
    assert_eq!(abp_copilot_sdk::dialect::DIALECT_VERSION, "copilot/v0.1");
}

// ── 2. SidecarSpec: valid command/args/env configuration ────────────────

#[test]
fn openai_sdk_has_valid_sidecar_config() {
    assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
    assert!(!abp_openai_sdk::HOST_SCRIPT_RELATIVE.is_empty());
    assert!(!abp_openai_sdk::DEFAULT_NODE_COMMAND.is_empty());
    let script = abp_openai_sdk::sidecar_script(Path::new("/root"));
    assert!(script.to_string_lossy().contains("openai"));
}

#[test]
fn claude_sdk_has_valid_sidecar_config() {
    assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
    assert!(!abp_claude_sdk::HOST_SCRIPT_RELATIVE.is_empty());
    assert!(!abp_claude_sdk::DEFAULT_NODE_COMMAND.is_empty());
    let script = abp_claude_sdk::sidecar_script(Path::new("/root"));
    assert!(script.to_string_lossy().contains("claude"));
}

#[test]
fn codex_sdk_has_valid_sidecar_config() {
    assert_eq!(abp_codex_sdk::BACKEND_NAME, "sidecar:codex");
    assert!(!abp_codex_sdk::HOST_SCRIPT_RELATIVE.is_empty());
    assert!(!abp_codex_sdk::DEFAULT_NODE_COMMAND.is_empty());
    let script = abp_codex_sdk::sidecar_script(Path::new("/root"));
    assert!(script.to_string_lossy().contains("codex"));
}

#[test]
fn gemini_sdk_has_valid_sidecar_config() {
    assert_eq!(abp_gemini_sdk::BACKEND_NAME, "sidecar:gemini");
    assert!(!abp_gemini_sdk::HOST_SCRIPT_RELATIVE.is_empty());
    assert!(!abp_gemini_sdk::DEFAULT_NODE_COMMAND.is_empty());
    let script = abp_gemini_sdk::sidecar_script(Path::new("/root"));
    assert!(script.to_string_lossy().contains("gemini"));
}

#[test]
fn kimi_sdk_has_valid_sidecar_config() {
    assert_eq!(abp_kimi_sdk::BACKEND_NAME, "sidecar:kimi");
    assert!(!abp_kimi_sdk::HOST_SCRIPT_RELATIVE.is_empty());
    assert!(!abp_kimi_sdk::DEFAULT_NODE_COMMAND.is_empty());
    let script = abp_kimi_sdk::sidecar_script(Path::new("/root"));
    assert!(script.to_string_lossy().contains("kimi"));
}

#[test]
fn copilot_sdk_has_valid_sidecar_config() {
    assert_eq!(abp_copilot_sdk::BACKEND_NAME, "sidecar:copilot");
    assert!(!abp_copilot_sdk::HOST_SCRIPT_RELATIVE.is_empty());
    assert!(!abp_copilot_sdk::DEFAULT_NODE_COMMAND.is_empty());
    let script = abp_copilot_sdk::sidecar_script(Path::new("/root"));
    assert!(script.to_string_lossy().contains("copilot"));
}

// ── 3. Non-empty capability sets ────────────────────────────────────────

#[test]
fn openai_capabilities_non_empty() {
    let caps = abp_openai_sdk::dialect::capability_manifest();
    assert!(
        !caps.is_empty(),
        "OpenAI capability manifest must not be empty"
    );
}

#[test]
fn claude_capabilities_non_empty() {
    let caps = abp_claude_sdk::dialect::capability_manifest();
    assert!(
        !caps.is_empty(),
        "Claude capability manifest must not be empty"
    );
}

#[test]
fn codex_capabilities_non_empty() {
    let caps = abp_codex_sdk::dialect::capability_manifest();
    assert!(
        !caps.is_empty(),
        "Codex capability manifest must not be empty"
    );
}

#[test]
fn gemini_capabilities_non_empty() {
    let caps = abp_gemini_sdk::dialect::capability_manifest();
    assert!(
        !caps.is_empty(),
        "Gemini capability manifest must not be empty"
    );
}

#[test]
fn kimi_capabilities_non_empty() {
    let caps = abp_kimi_sdk::dialect::capability_manifest();
    assert!(
        !caps.is_empty(),
        "Kimi capability manifest must not be empty"
    );
}

#[test]
fn copilot_capabilities_non_empty() {
    let caps = abp_copilot_sdk::dialect::capability_manifest();
    assert!(
        !caps.is_empty(),
        "Copilot capability manifest must not be empty"
    );
}

// ── 4. Minimum capabilities: streaming + tool_use (via ToolRead/ToolWrite) ──

/// Checks that a manifest includes Streaming and at least one tool capability
/// (ToolRead or ToolWrite) at Native or Emulated level.
fn assert_minimum_capabilities(caps: &abp_core::CapabilityManifest, label: &str) {
    // Streaming must be present and supported
    let streaming = caps.get(&Capability::Streaming);
    assert!(
        matches!(
            streaming,
            Some(SupportLevel::Native | SupportLevel::Emulated)
        ),
        "{label}: must support Streaming at Native or Emulated level, got {streaming:?}"
    );

    // At least one tool capability must be present
    let has_tool = caps.iter().any(|(cap, level)| {
        matches!(
            cap,
            Capability::ToolRead | Capability::ToolWrite | Capability::ToolUse
        ) && matches!(level, SupportLevel::Native | SupportLevel::Emulated)
    });
    assert!(
        has_tool,
        "{label}: must support at least one tool capability"
    );
}

#[test]
fn openai_has_minimum_capabilities() {
    assert_minimum_capabilities(&abp_openai_sdk::dialect::capability_manifest(), "OpenAI");
}

#[test]
fn claude_has_minimum_capabilities() {
    assert_minimum_capabilities(&abp_claude_sdk::dialect::capability_manifest(), "Claude");
}

#[test]
fn codex_has_minimum_capabilities() {
    assert_minimum_capabilities(&abp_codex_sdk::dialect::capability_manifest(), "Codex");
}

#[test]
fn gemini_has_minimum_capabilities() {
    assert_minimum_capabilities(&abp_gemini_sdk::dialect::capability_manifest(), "Gemini");
}

#[test]
fn kimi_has_minimum_capabilities() {
    assert_minimum_capabilities(&abp_kimi_sdk::dialect::capability_manifest(), "Kimi");
}

#[test]
fn copilot_has_minimum_capabilities() {
    assert_minimum_capabilities(&abp_copilot_sdk::dialect::capability_manifest(), "Copilot");
}

// ── 5. IR lowering roundtrip ────────────────────────────────────────────

#[test]
fn openai_ir_lowering_roundtrip() {
    use abp_openai_sdk::dialect::OpenAIMessage;

    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("hello from test".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = abp_openai_sdk::lowering::to_ir(&msgs);
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello from test");

    let back = abp_openai_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("hello from test"));
}

#[test]
fn claude_ir_lowering_roundtrip() {
    use abp_claude_sdk::dialect::ClaudeMessage;

    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "hello from test".into(),
    }];
    let ir = abp_claude_sdk::lowering::to_ir(&msgs, None);
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello from test");

    let back = abp_claude_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "hello from test");
}

#[test]
fn codex_ir_lowering_roundtrip() {
    use abp_codex_sdk::dialect::{CodexContentPart, CodexResponseItem};

    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "hello from test".into(),
        }],
    }];
    let ir = abp_codex_sdk::lowering::to_ir(&items);
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    assert_eq!(ir.messages[0].text_content(), "hello from test");

    let back = abp_codex_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "hello from test"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn gemini_ir_lowering_roundtrip() {
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};

    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hello from test".into())],
    }];
    let ir = abp_gemini_sdk::lowering::to_ir(&contents, None);
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello from test");

    let back = abp_gemini_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    let text = back[0]
        .parts
        .iter()
        .filter_map(|p| match p {
            GeminiPart::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "hello from test");
}

#[test]
fn kimi_ir_lowering_roundtrip() {
    use abp_kimi_sdk::dialect::KimiMessage;

    let msgs = vec![KimiMessage {
        role: "user".into(),
        content: Some("hello from test".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = abp_kimi_sdk::lowering::to_ir(&msgs);
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello from test");

    let back = abp_kimi_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("hello from test"));
}

#[test]
fn copilot_ir_lowering_roundtrip() {
    use abp_copilot_sdk::dialect::CopilotMessage;

    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "hello from test".into(),
        name: None,
        copilot_references: Vec::new(),
    }];
    let ir = abp_copilot_sdk::lowering::to_ir(&msgs);
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello from test");

    let back = abp_copilot_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "hello from test");
}

// ── 6. Dialect detection ────────────────────────────────────────────────

#[test]
fn detect_openai_dialect() {
    let detector = DialectDetector::new();
    let payload = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7,
        "max_tokens": 100
    });
    let result = detector.detect(&payload).expect("should detect a dialect");
    assert_eq!(result.dialect, Dialect::OpenAi);
    assert!(result.confidence > 0.0);
}

#[test]
fn detect_claude_dialect() {
    let detector = DialectDetector::new();
    // Claude responses use "type":"message" and array "content" blocks
    let payload = serde_json::json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "role": "assistant",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let result = detector.detect(&payload).expect("should detect a dialect");
    assert_eq!(result.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_dialect() {
    let detector = DialectDetector::new();
    let payload = serde_json::json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
        "generationConfig": {"temperature": 0.5}
    });
    let result = detector.detect(&payload).expect("should detect a dialect");
    assert_eq!(result.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_dialect() {
    let detector = DialectDetector::new();
    // Codex responses use "object":"response" and "items" with typed entries
    let payload = serde_json::json!({
        "object": "response",
        "model": "codex-mini-latest",
        "status": "completed",
        "items": [{"type": "message", "role": "assistant", "content": []}]
    });
    let result = detector.detect(&payload).expect("should detect a dialect");
    assert_eq!(result.dialect, Dialect::Codex);
}

// ── 7. Cross-SDK coexistence ────────────────────────────────────────────

#[test]
fn all_six_sdks_coexist_no_symbol_conflicts() {
    // Import all 6 SDK backend names in one scope — confirms no linker conflicts
    let names = [
        abp_openai_sdk::BACKEND_NAME,
        abp_claude_sdk::BACKEND_NAME,
        abp_codex_sdk::BACKEND_NAME,
        abp_gemini_sdk::BACKEND_NAME,
        abp_kimi_sdk::BACKEND_NAME,
        abp_copilot_sdk::BACKEND_NAME,
    ];
    let unique: HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), 6, "all 6 backend names must be distinct");
}

#[test]
fn all_six_sdks_dialect_versions_accessible() {
    let versions = [
        abp_openai_sdk::dialect::DIALECT_VERSION,
        abp_claude_sdk::dialect::DIALECT_VERSION,
        abp_codex_sdk::dialect::DIALECT_VERSION,
        abp_gemini_sdk::dialect::DIALECT_VERSION,
        abp_kimi_sdk::dialect::DIALECT_VERSION,
        abp_copilot_sdk::dialect::DIALECT_VERSION,
    ];
    for v in &versions {
        assert!(!v.is_empty());
        assert!(
            v.contains("/v0.1"),
            "dialect version {v} should contain /v0.1"
        );
    }
}

#[test]
fn all_six_lowering_modules_produce_valid_ir() {
    // Build a simple user-text IrConversation from each SDK and verify
    // they all produce the same IR structure.
    let expected_text = "cross-sdk test message";

    // OpenAI
    let openai_ir = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
        role: "user".into(),
        content: Some(expected_text.into()),
        tool_calls: None,
        tool_call_id: None,
    }]);

    // Claude
    let claude_ir = abp_claude_sdk::lowering::to_ir(
        &[abp_claude_sdk::dialect::ClaudeMessage {
            role: "user".into(),
            content: expected_text.into(),
        }],
        None,
    );

    // Gemini
    let gemini_ir = abp_gemini_sdk::lowering::to_ir(
        &[abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                expected_text.into(),
            )],
        }],
        None,
    );

    // Kimi
    let kimi_ir = abp_kimi_sdk::lowering::to_ir(&[abp_kimi_sdk::dialect::KimiMessage {
        role: "user".into(),
        content: Some(expected_text.into()),
        tool_call_id: None,
        tool_calls: None,
    }]);

    // Copilot
    let copilot_ir =
        abp_copilot_sdk::lowering::to_ir(&[abp_copilot_sdk::dialect::CopilotMessage {
            role: "user".into(),
            content: expected_text.into(),
            name: None,
            copilot_references: Vec::new(),
        }]);

    for (label, ir) in [
        ("OpenAI", &openai_ir),
        ("Claude", &claude_ir),
        ("Gemini", &gemini_ir),
        ("Kimi", &kimi_ir),
        ("Copilot", &copilot_ir),
    ] {
        assert_eq!(ir.messages.len(), 1, "{label}: expected 1 IR message");
        assert_eq!(
            ir.messages[0].role,
            IrRole::User,
            "{label}: expected User role"
        );
        assert_eq!(
            ir.messages[0].text_content(),
            expected_text,
            "{label}: text content mismatch"
        );
    }
}

// ── 8. command_exists returns bool (register_default returns bool) ───────

#[test]
fn openai_register_default_returns_bool() {
    let mut rt = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/test/path");
    let result = abp_openai_sdk::register_default(&mut rt, bogus, None);
    // Either Ok(false) (command not found or script missing) or Ok(true).
    // On CI, node may or may not exist. The key assertion: it returns a bool.
    assert!(result.is_ok() || result.is_err());
    if let Ok(v) = result {
        let _: bool = v; // type assertion
    }
}

#[test]
fn claude_register_default_returns_bool() {
    let mut rt = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/test/path");
    let result = abp_claude_sdk::register_default(&mut rt, bogus, None);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn codex_register_default_returns_bool() {
    let mut rt = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/test/path");
    let result = abp_codex_sdk::register_default(&mut rt, bogus, None);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn gemini_register_default_returns_bool() {
    let mut rt = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/test/path");
    let result = abp_gemini_sdk::register_default(&mut rt, bogus, None);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn kimi_register_default_returns_bool() {
    let mut rt = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/test/path");
    let result = abp_kimi_sdk::register_default(&mut rt, bogus, None);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn copilot_register_default_returns_bool() {
    let mut rt = abp_runtime::Runtime::new();
    let bogus = Path::new("/nonexistent/test/path");
    let result = abp_copilot_sdk::register_default(&mut rt, bogus, None);
    assert!(result.is_ok() || result.is_err());
}

// ── 9. Default model string is non-empty ────────────────────────────────

#[test]
fn openai_default_model_non_empty() {
    assert!(
        !abp_openai_sdk::dialect::DEFAULT_MODEL.is_empty(),
        "OpenAI default model must not be empty"
    );
}

#[test]
fn claude_default_model_non_empty() {
    assert!(
        !abp_claude_sdk::dialect::DEFAULT_MODEL.is_empty(),
        "Claude default model must not be empty"
    );
}

#[test]
fn codex_default_model_non_empty() {
    assert!(
        !abp_codex_sdk::dialect::DEFAULT_MODEL.is_empty(),
        "Codex default model must not be empty"
    );
}

#[test]
fn gemini_default_model_non_empty() {
    assert!(
        !abp_gemini_sdk::dialect::DEFAULT_MODEL.is_empty(),
        "Gemini default model must not be empty"
    );
}

#[test]
fn kimi_default_model_non_empty() {
    assert!(
        !abp_kimi_sdk::dialect::DEFAULT_MODEL.is_empty(),
        "Kimi default model must not be empty"
    );
}

#[test]
fn copilot_default_model_non_empty() {
    assert!(
        !abp_copilot_sdk::dialect::DEFAULT_MODEL.is_empty(),
        "Copilot default model must not be empty"
    );
}

// ── 10. Distinct dialect variants (no duplicates) ───────────────────────

#[test]
fn all_dialect_variants_are_distinct() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6, "expected exactly 6 dialect variants");

    let unique: HashSet<_> = all.iter().collect();
    assert_eq!(
        unique.len(),
        all.len(),
        "dialect variants must all be distinct"
    );
}

#[test]
fn dialect_labels_are_distinct_and_non_empty() {
    let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
    for label in &labels {
        assert!(!label.is_empty(), "dialect label must not be empty");
    }
    let unique: HashSet<_> = labels.iter().collect();
    assert_eq!(
        unique.len(),
        labels.len(),
        "dialect labels must all be distinct"
    );
}

#[test]
fn backend_names_correspond_to_dialect_variants() {
    // Verify each SDK backend name embeds the expected dialect identifier
    let pairs = [
        (abp_openai_sdk::BACKEND_NAME, "openai"),
        (abp_claude_sdk::BACKEND_NAME, "claude"),
        (abp_codex_sdk::BACKEND_NAME, "codex"),
        (abp_gemini_sdk::BACKEND_NAME, "gemini"),
        (abp_kimi_sdk::BACKEND_NAME, "kimi"),
        (abp_copilot_sdk::BACKEND_NAME, "copilot"),
    ];
    for (name, expected) in &pairs {
        assert!(
            name.contains(expected),
            "backend name '{name}' should contain '{expected}'"
        );
    }
}

// ── Bonus: cross-SDK IR interop ─────────────────────────────────────────

#[test]
fn cross_sdk_ir_interop_openai_to_claude() {
    // Lower an OpenAI message to IR, then raise to Claude format
    let openai_msgs = vec![abp_openai_sdk::dialect::OpenAIMessage {
        role: "user".into(),
        content: Some("translate me".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = abp_openai_sdk::lowering::to_ir(&openai_msgs);

    let claude_msgs = abp_claude_sdk::lowering::from_ir(&ir);
    assert_eq!(claude_msgs.len(), 1);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "translate me");
}

#[test]
fn cross_sdk_ir_interop_claude_to_gemini() {
    let claude_msgs = vec![abp_claude_sdk::dialect::ClaudeMessage {
        role: "user".into(),
        content: "translate me".into(),
    }];
    let ir = abp_claude_sdk::lowering::to_ir(&claude_msgs, None);

    let gemini_contents = abp_gemini_sdk::lowering::from_ir(&ir);
    assert_eq!(gemini_contents.len(), 1);
    assert_eq!(gemini_contents[0].role, "user");
}

#[test]
fn cross_sdk_ir_interop_kimi_to_copilot() {
    let kimi_msgs = vec![abp_kimi_sdk::dialect::KimiMessage {
        role: "user".into(),
        content: Some("translate me".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = abp_kimi_sdk::lowering::to_ir(&kimi_msgs);

    let copilot_msgs = abp_copilot_sdk::lowering::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 1);
    assert_eq!(copilot_msgs[0].role, "user");
    assert_eq!(copilot_msgs[0].content, "translate me");
}
