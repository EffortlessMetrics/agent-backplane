// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive cross-SDK dialect mapping tests via the IR layer.
//!
//! Tests the FULL pipeline: SDK A → IR → SDK B for all SDK pairs,
//! covering text messages, tool calls, tool results, system prompts,
//! thinking blocks, images, multi-turn conversations, and edge cases.

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_claude_sdk::lowering as claude_ir;
use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, ReasoningSummary,
};
use abp_codex_sdk::lowering as codex_ir;
use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
use abp_copilot_sdk::lowering as copilot_ir;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_gemini_sdk::lowering as gemini_ir;
use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall};
use abp_kimi_sdk::lowering as kimi_ir;
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_ir;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
//  OpenAI → IR → Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_user_text() {
    let openai = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello Claude!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hello Claude!");
}

#[test]
fn openai_to_claude_assistant_text() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Sure thing!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "assistant");
    assert_eq!(claude[0].content, "Sure thing!");
}

#[test]
fn openai_to_claude_system_becomes_extractable() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    // Claude skips system messages in from_ir; extract separately
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be helpful."));
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1); // system skipped
    assert_eq!(claude[0].role, "user");
}

#[test]
fn openai_to_claude_tool_call() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_abc".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse as blocks");
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_abc");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn openai_to_claude_tool_result() {
    let openai = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("file contents here".into()),
        tool_calls: None,
        tool_call_id: Some("call_abc".into()),
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    // Claude maps tool results as user role with structured blocks
    assert_eq!(claude[0].role, "user");
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "call_abc");
            assert_eq!(content.as_deref(), Some("file contents here"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn openai_to_claude_multi_turn() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be concise.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Bye".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 3); // system skipped
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hi");
    assert_eq!(claude[1].role, "assistant");
    assert_eq!(claude[2].role, "user");
    assert_eq!(claude[2].content, "Bye");
}

#[test]
fn openai_to_claude_text_and_tool_call() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Let me check.".into()),
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "ls".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], ClaudeContentBlock::Text { .. }));
    assert!(matches!(&blocks[1], ClaudeContentBlock::ToolUse { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Claude → IR → OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_openai_user_text() {
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello OpenAI!".into(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "user");
    assert_eq!(openai[0].content.as_deref(), Some("Hello OpenAI!"));
}

#[test]
fn claude_to_openai_system_prompt_preserved() {
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let ir = claude_ir::to_ir(&claude, Some("Be helpful"));
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai.len(), 2);
    assert_eq!(openai[0].role, "system");
    assert_eq!(openai[0].content.as_deref(), Some("Be helpful"));
    assert_eq!(openai[1].role, "user");
}

#[test]
fn claude_to_openai_tool_use() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    }];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "assistant");
    let tc = &openai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "tu_1");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn claude_to_openai_tool_result() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("results".into()),
        is_error: None,
    }];
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    // Claude puts tool results in user-role messages; IR preserves IrRole::User
    // so OpenAI from_ir maps it as "user" (not "tool", which requires IrRole::Tool)
    assert_eq!(openai[0].role, "user");
}

#[test]
fn claude_to_openai_thinking_becomes_text() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig123".into()),
        },
        ClaudeContentBlock::Text {
            text: "Answer".into(),
        },
    ];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    // OpenAI flattens thinking + text into content
    let text = openai[0].content.as_deref().unwrap();
    assert!(text.contains("Let me reason..."));
    assert!(text.contains("Answer"));
}

#[test]
fn claude_to_openai_content_blocks_mixed() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Here:".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        },
    ];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].content.as_deref(), Some("Here:"));
    assert!(openai[0].tool_calls.is_some());
}

#[test]
fn claude_to_openai_image_block_lossy() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        },
    }];
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    // OpenAI doesn't natively support image blocks; they're dropped
    assert!(openai[0].content.is_none() || openai[0].content.as_deref() == Some(""));
}

// ═══════════════════════════════════════════════════════════════════════════
//  OpenAI → IR → Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_gemini_user_text() {
    let openai = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello Gemini!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "user");
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello Gemini!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_assistant_text() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Sure!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "model");
}

#[test]
fn openai_to_gemini_system_becomes_extractable() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be brief.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let sys = gemini_ir::extract_system_instruction(&ir);
    assert!(sys.is_some());
    match &sys.unwrap().parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be brief."),
        other => panic!("expected Text, got {other:?}"),
    }
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1); // system skipped
}

#[test]
fn openai_to_gemini_tool_call_becomes_function_call() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_42".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_tool_result_becomes_function_response() {
    let openai = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("result data".into()),
        tool_calls: None,
        tool_call_id: Some("call_42".into()),
    }];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "call_42");
            assert_eq!(response, &json!("result data"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn openai_to_gemini_multi_turn() {
    let openai = vec![
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Bye".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 3);
    assert_eq!(gemini[0].role, "user");
    assert_eq!(gemini[1].role, "model");
    assert_eq!(gemini[2].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Gemini → IR → OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_openai_user_text() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello OpenAI!".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "user");
    assert_eq!(openai[0].content.as_deref(), Some("Hello OpenAI!"));
}

#[test]
fn gemini_to_openai_model_text() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Sure!".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "assistant");
}

#[test]
fn gemini_to_openai_function_call_becomes_tool_call() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        }],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    let tc = &openai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "search");
    // Gemini synthesizes ID as gemini_<name>
    assert_eq!(tc.id, "gemini_search");
}

#[test]
fn gemini_to_openai_function_response_becomes_user() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results here"),
        }],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    // Gemini puts function responses in user-role; IR preserves IrRole::User
    assert_eq!(openai[0].role, "user");
}

#[test]
fn gemini_to_openai_system_instruction_preserved() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be helpful".into())],
    };
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, Some(&sys));
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai.len(), 2);
    assert_eq!(openai[0].role, "system");
    assert_eq!(openai[0].content.as_deref(), Some("Be helpful"));
}

#[test]
fn gemini_to_openai_inline_data_lossy() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "base64data".into(),
        })],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    // OpenAI doesn't support image blocks; they get dropped
    assert!(openai[0].content.is_none() || openai[0].content.as_deref() == Some(""));
}

#[test]
fn gemini_to_openai_multiple_parts() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::Text("Let me search.".into()),
            GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({}),
            },
        ],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].content.as_deref(), Some("Let me search."));
    assert!(openai[0].tool_calls.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Claude → IR → Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_gemini_user_text() {
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello Gemini!".into(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "user");
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello Gemini!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_to_gemini_system_prompt() {
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let ir = claude_ir::to_ir(&claude, Some("Be concise"));
    let sys = gemini_ir::extract_system_instruction(&ir);
    assert!(sys.is_some());
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1); // system skipped
}

#[test]
fn claude_to_gemini_tool_use() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read_file".into(),
        input: json!({"path": "lib.rs"}),
    }];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "read_file");
            assert_eq!(args, &json!({"path": "lib.rs"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn claude_to_gemini_tool_result() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("file data".into()),
        is_error: None,
    }];
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "tu_1");
            assert_eq!(response, &json!("file data"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn claude_to_gemini_thinking_becomes_text() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "reasoning...".into(),
        signature: None,
    }];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir);
    // Gemini has no native thinking; becomes text
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "reasoning..."),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_to_gemini_image_block() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "imgdata".into(),
        },
    }];
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "imgdata");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Gemini → IR → Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_claude_user_text() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi Claude!".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hi Claude!");
}

#[test]
fn gemini_to_claude_model_becomes_assistant() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Hello!".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "assistant");
}

#[test]
fn gemini_to_claude_function_call() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "grep".into(),
            args: json!({"pattern": "fn main"}),
        }],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { name, input, .. } => {
            assert_eq!(name, "grep");
            assert_eq!(input, &json!({"pattern": "fn main"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn gemini_to_claude_function_response() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "grep".into(),
            response: json!("match found"),
        }],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "gemini_grep");
            assert_eq!(content.as_deref(), Some("match found"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn gemini_to_claude_inline_data_becomes_image() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "pngdata".into(),
        })],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 { media_type, data },
        } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "pngdata");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn gemini_to_claude_system_instruction() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("You are helpful".into())],
    };
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, Some(&sys));
    let prompt = claude_ir::extract_system_prompt(&ir);
    assert_eq!(prompt.as_deref(), Some("You are helpful"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Codex ↔ OpenAI (similar formats)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_to_openai_assistant_text() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Done!".into(),
        }],
    }];
    let ir = codex_ir::to_ir(&items);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "assistant");
    assert_eq!(openai[0].content.as_deref(), Some("Done!"));
}

#[test]
fn codex_to_openai_function_call() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: None,
        name: "shell".into(),
        arguments: r#"{"command":"ls"}"#.into(),
    }];
    let ir = codex_ir::to_ir(&items);
    let openai = openai_ir::from_ir(&ir);
    let tc = &openai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "fc_1");
    assert_eq!(tc.function.name, "shell");
}

#[test]
fn codex_to_openai_function_call_output() {
    let items = vec![CodexResponseItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "file list".into(),
    }];
    let ir = codex_ir::to_ir(&items);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "tool");
    assert_eq!(openai[0].tool_call_id.as_deref(), Some("fc_1"));
    assert_eq!(openai[0].content.as_deref(), Some("file list"));
}

#[test]
fn codex_to_openai_reasoning_becomes_text() {
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![ReasoningSummary {
            text: "thinking...".into(),
        }],
    }];
    let ir = codex_ir::to_ir(&items);
    let openai = openai_ir::from_ir(&ir);
    // Reasoning becomes Thinking in IR, which OpenAI puts in content
    let content = openai[0].content.as_deref().unwrap();
    assert!(content.contains("thinking..."));
}

#[test]
fn openai_to_codex_assistant_text() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Hello".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let codex = codex_ir::from_ir(&ir);
    assert_eq!(codex.len(), 1);
    match &codex[0] {
        CodexResponseItem::Message { role, content } => {
            assert_eq!(role, "assistant");
            match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Hello"),
            }
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn openai_to_codex_tool_call() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "read".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let codex = codex_ir::from_ir(&ir);
    match &codex[0] {
        CodexResponseItem::FunctionCall { id, name, .. } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "read");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn openai_to_codex_tool_result() {
    let openai = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("data".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    }];
    let ir = openai_ir::to_ir(&openai);
    let codex = codex_ir::from_ir(&ir);
    match &codex[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "call_1");
            assert_eq!(output, "data");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn openai_to_codex_system_and_user_skipped() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("user".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("asst".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let codex = codex_ir::from_ir(&ir);
    // Codex from_ir only outputs assistant and tool messages
    assert_eq!(codex.len(), 1);
}

#[test]
fn codex_input_to_openai() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "Be helpful".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        },
    ];
    let ir = codex_ir::input_to_ir(&items);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "system");
    assert_eq!(openai[0].content.as_deref(), Some("Be helpful"));
    assert_eq!(openai[1].role, "user");
    assert_eq!(openai[1].content.as_deref(), Some("Hello"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Kimi ↔ OpenAI (OpenAI-compatible with extensions)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_to_openai_user_text() {
    let kimi = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "user");
    assert_eq!(openai[0].content.as_deref(), Some("Hello"));
}

#[test]
fn kimi_to_openai_system() {
    let kimi = vec![KimiMessage {
        role: "system".into(),
        content: Some("You are helpful.".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "system");
    assert_eq!(openai[0].content.as_deref(), Some("You are helpful."));
}

#[test]
fn kimi_to_openai_tool_call() {
    let kimi = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "call_k1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"query":"rust"}"#.into(),
            },
        }]),
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let openai = openai_ir::from_ir(&ir);
    let tc = &openai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_k1");
    assert_eq!(tc.function.name, "web_search");
}

#[test]
fn kimi_to_openai_tool_result() {
    let kimi = vec![KimiMessage {
        role: "tool".into(),
        content: Some("search results".into()),
        tool_call_id: Some("call_k1".into()),
        tool_calls: None,
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "tool");
    assert_eq!(openai[0].tool_call_id.as_deref(), Some("call_k1"));
}

#[test]
fn openai_to_kimi_user_text() {
    let openai = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello Kimi!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].role, "user");
    assert_eq!(kimi[0].content.as_deref(), Some("Hello Kimi!"));
}

#[test]
fn openai_to_kimi_tool_call() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_o1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let kimi = kimi_ir::from_ir(&ir);
    let tc = &kimi[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_o1");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn openai_to_kimi_tool_result() {
    let openai = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("result".into()),
        tool_calls: None,
        tool_call_id: Some("call_o1".into()),
    }];
    let ir = openai_ir::to_ir(&openai);
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].role, "tool");
    assert_eq!(kimi[0].tool_call_id.as_deref(), Some("call_o1"));
    assert_eq!(kimi[0].content.as_deref(), Some("result"));
}

#[test]
fn kimi_to_openai_multi_turn() {
    let kimi = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("Be brief.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let ir = kimi_ir::to_ir(&kimi);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai.len(), 3);
    assert_eq!(openai[0].role, "system");
    assert_eq!(openai[1].role, "user");
    assert_eq!(openai[2].role, "assistant");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Copilot ↔ OpenAI
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_openai_user_text() {
    let copilot = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello OpenAI!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "user");
    assert_eq!(openai[0].content.as_deref(), Some("Hello OpenAI!"));
}

#[test]
fn copilot_to_openai_system() {
    let copilot = vec![CopilotMessage {
        role: "system".into(),
        content: "Be helpful.".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "system");
    assert_eq!(openai[0].content.as_deref(), Some("Be helpful."));
}

#[test]
fn copilot_to_openai_assistant() {
    let copilot = vec![CopilotMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "assistant");
    assert_eq!(openai[0].content.as_deref(), Some("Sure!"));
}

#[test]
fn copilot_to_openai_references_lost() {
    let copilot = vec![CopilotMessage {
        role: "user".into(),
        content: "Check this file".into(),
        name: None,
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f1".into(),
            data: json!({"path": "main.rs"}),
            metadata: None,
        }],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let openai = openai_ir::from_ir(&ir);
    // References are in IR metadata but OpenAI doesn't have them
    assert_eq!(openai[0].content.as_deref(), Some("Check this file"));
    assert!(openai[0].tool_calls.is_none());
}

#[test]
fn openai_to_copilot_user_text() {
    let openai = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello Copilot!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "user");
    assert_eq!(copilot[0].content, "Hello Copilot!");
}

#[test]
fn openai_to_copilot_tool_role_becomes_user() {
    let openai = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("data".into()),
        tool_calls: None,
        tool_call_id: Some("c1".into()),
    }];
    let ir = openai_ir::to_ir(&openai);
    let copilot = copilot_ir::from_ir(&ir);
    // Copilot has no tool role; mapped to user
    assert_eq!(copilot[0].role, "user");
}

#[test]
fn openai_to_copilot_multi_turn() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("instructions".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hello!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot.len(), 3);
    assert_eq!(copilot[0].role, "system");
    assert_eq!(copilot[1].role, "user");
    assert_eq!(copilot[2].role, "assistant");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Multi-turn conversations through IR (full pipeline)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_tool_flow_through_claude() {
    let openai = vec![
        OpenAIMessage {
            role: "user".into(),
            content: Some("Read main.rs".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "tool".into(),
            content: Some("fn main() {}".into()),
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Done.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 4);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[1].role, "assistant");
    assert_eq!(claude[2].role, "user"); // tool result → user in Claude
    assert_eq!(claude[3].role, "assistant");
}

#[test]
fn claude_tool_flow_through_openai() {
    let tool_use = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read".into(),
        input: json!({}),
    }];
    let tool_result = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("data".into()),
        is_error: None,
    }];
    let claude = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Do it".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&tool_use).unwrap(),
        },
        ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&tool_result).unwrap(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "Done.".into(),
        },
    ];
    let ir = claude_ir::to_ir(&claude, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai.len(), 4);
    assert_eq!(openai[0].role, "user");
    assert!(openai[1].tool_calls.is_some());
    assert_eq!(openai[2].role, "user"); // Claude tool result is user-role in IR
    assert_eq!(openai[3].role, "assistant");
}

#[test]
fn gemini_tool_flow_through_openai() {
    let gemini = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Search rust".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
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
            parts: vec![GeminiPart::Text("Here.".into())],
        },
    ];
    let ir = gemini_ir::to_ir(&gemini, None);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai.len(), 4);
    assert_eq!(openai[1].role, "assistant");
    assert!(openai[1].tool_calls.is_some());
    assert_eq!(openai[2].role, "user"); // Gemini function response is user-role in IR
}

#[test]
fn openai_tool_flow_through_gemini() {
    let openai = vec![
        OpenAIMessage {
            role: "user".into(),
            content: Some("Search".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "tool".into(),
            content: Some("found".into()),
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Here.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 4);
    assert!(matches!(
        &gemini[1].parts[0],
        GeminiPart::FunctionCall { .. }
    ));
    assert!(matches!(
        &gemini[2].parts[0],
        GeminiPart::FunctionResponse { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Tool definitions across SDKs (canonical form)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_openai_to_claude_canonical() {
    use abp_claude_sdk::dialect::{CanonicalToolDef as ClaudeCanon, tool_def_to_claude};
    use abp_openai_sdk::dialect::{OpenAIFunctionDef, OpenAIToolDef, tool_def_from_openai};

    let openai_tool = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
    };
    let canonical = tool_def_from_openai(&openai_tool);
    let claude_canon = ClaudeCanon {
        name: canonical.name,
        description: canonical.description,
        parameters_schema: canonical.parameters_schema,
    };
    let claude_tool = tool_def_to_claude(&claude_canon);
    assert_eq!(claude_tool.name, "read_file");
    assert_eq!(claude_tool.description, "Read a file");
}

#[test]
fn tool_def_claude_to_gemini_canonical() {
    use abp_claude_sdk::dialect::{ClaudeToolDef, tool_def_from_claude};
    use abp_gemini_sdk::dialect::{CanonicalToolDef as GemCanon, tool_def_to_gemini};

    let claude_tool = ClaudeToolDef {
        name: "search".into(),
        description: "Search code".into(),
        input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let canonical = tool_def_from_claude(&claude_tool);
    let gem_canon = GemCanon {
        name: canonical.name,
        description: canonical.description,
        parameters_schema: canonical.parameters_schema,
    };
    let gemini_tool = tool_def_to_gemini(&gem_canon);
    assert_eq!(gemini_tool.name, "search");
    assert_eq!(gemini_tool.description, "Search code");
}

#[test]
fn tool_def_gemini_to_kimi_canonical() {
    use abp_gemini_sdk::dialect::{GeminiFunctionDeclaration, tool_def_from_gemini};
    use abp_kimi_sdk::dialect::{CanonicalToolDef as KimiCanon, tool_def_to_kimi};

    let gemini_func = GeminiFunctionDeclaration {
        name: "grep".into(),
        description: "Search patterns".into(),
        parameters: json!({"type": "object"}),
    };
    let canonical = tool_def_from_gemini(&gemini_func);
    let kimi_canon = KimiCanon {
        name: canonical.name,
        description: canonical.description,
        parameters_schema: canonical.parameters_schema,
    };
    let kimi_tool = tool_def_to_kimi(&kimi_canon);
    assert_eq!(kimi_tool.function.name, "grep");
}

// ═══════════════════════════════════════════════════════════════════════════
//  System message handling differences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_openai_to_all_sdks() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("System prompt".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);

    // Claude: system extracted separately
    let claude_sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(claude_sys.as_deref(), Some("System prompt"));
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1);

    // Gemini: system extracted as system_instruction
    let gemini_sys = gemini_ir::extract_system_instruction(&ir);
    assert!(gemini_sys.is_some());
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini.len(), 1);

    // Kimi: system kept inline (like OpenAI)
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi.len(), 2);
    assert_eq!(kimi[0].role, "system");

    // Copilot: system kept inline
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot.len(), 2);
    assert_eq!(copilot[0].role, "system");

    // Codex: system/user skipped (response-only)
    let codex = codex_ir::from_ir(&ir);
    assert!(codex.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Content block type mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_block_claude_to_all() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "deep thought".into(),
        signature: None,
    }];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);

    // OpenAI: thinking becomes text content
    let openai = openai_ir::from_ir(&ir);
    assert!(
        openai[0]
            .content
            .as_deref()
            .unwrap()
            .contains("deep thought")
    );

    // Gemini: thinking becomes text part
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "deep thought"),
        other => panic!("expected Text, got {other:?}"),
    }

    // Codex: thinking becomes Reasoning
    let codex = codex_ir::from_ir(&ir);
    match &codex[0] {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary[0].text, "deep thought");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }

    // Kimi: thinking becomes text
    let kimi = kimi_ir::from_ir(&ir);
    assert!(kimi[0].content.as_deref().unwrap().contains("deep thought"));

    // Copilot: thinking becomes text
    let copilot = copilot_ir::from_ir(&ir);
    assert!(copilot[0].content.contains("deep thought"));
}

#[test]
fn image_block_claude_to_gemini() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/webp".into(),
            data: "webpdata".into(),
        },
    }];
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/webp");
            assert_eq!(d.data, "webpdata");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn image_block_gemini_to_claude() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/gif".into(),
            data: "gifdata".into(),
        })],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 { media_type, data },
        } => {
            assert_eq!(media_type, "image/gif");
            assert_eq!(data, "gifdata");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Edge cases: features only available in one SDK
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_thinking_signature_lost_in_roundtrip() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: Some("sig_secret".into()),
    }];
    let claude_in = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude_in, None);
    // Through any other SDK and back, signature is lost
    let openai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&openai);
    let claude_out = claude_ir::from_ir(&ir2);
    // The thinking text is preserved but signature is lost
    assert!(claude_out[0].content.contains("hmm"));
}

#[test]
fn codex_reasoning_through_claude() {
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![
            ReasoningSummary {
                text: "Step 1".into(),
            },
            ReasoningSummary {
                text: "Step 2".into(),
            },
        ],
    }];
    let ir = codex_ir::to_ir(&items);
    let claude = claude_ir::from_ir(&ir);
    // Claude represents thinking as a block
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::Thinking { thinking, .. } => {
            assert!(thinking.contains("Step 1"));
            assert!(thinking.contains("Step 2"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn copilot_references_lost_through_openai() {
    let copilot_in = vec![CopilotMessage {
        role: "user".into(),
        content: "check file".into(),
        name: Some("alice".into()),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f1".into(),
            data: json!({"path": "main.rs"}),
            metadata: None,
        }],
    }];
    let ir = copilot_ir::to_ir(&copilot_in);
    // Through OpenAI
    let openai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&openai);
    let copilot_out = copilot_ir::from_ir(&ir2);
    // Text preserved
    assert_eq!(copilot_out[0].content, "check file");
    // References and name lost after OpenAI roundtrip
    assert!(copilot_out[0].copilot_references.is_empty());
    assert!(copilot_out[0].name.is_none());
}

#[test]
fn copilot_name_preserved_through_ir() {
    let copilot_in = vec![CopilotMessage {
        role: "user".into(),
        content: "Hi".into(),
        name: Some("bob".into()),
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot_in);
    let copilot_out = copilot_ir::from_ir(&ir);
    assert_eq!(copilot_out[0].name.as_deref(), Some("bob"));
}

#[test]
fn claude_image_url_lossy_through_openai() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    }];
    let claude = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    // URL images become text placeholders in IR
    let text = ir.messages[0].text_content();
    assert!(text.contains("https://example.com/img.png"));
}

#[test]
fn gemini_function_id_synthesis() {
    // Gemini doesn't have per-call IDs; they're synthesized
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "custom_tool".into(),
            args: json!({}),
        }],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolUse { id, .. } => {
            assert_eq!(id, "gemini_custom_tool");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Lossy mappings (features that can't be preserved)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_tool_result_error_flag_through_openai() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("not found".into()),
        is_error: Some(true),
    }];
    let claude_in = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_ir::to_ir(&claude_in, None);
    // Claude tool result is user-role in IR; through OpenAI it becomes a plain user
    // message. The ToolResult structure (including is_error) is lost in translation.
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "user");
    // The ToolResult content doesn't map to OpenAI user content naturally
    // so the is_error flag is completely lost
}

#[test]
fn openai_tool_call_type_not_preserved_through_gemini() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_xyz".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "read".into(),
                arguments: r#"{"p":"x"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let gemini = gemini_ir::from_ir(&ir);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    // ID is re-synthesized by Gemini as "gemini_<name>"
    match &ir2.messages[0].content[0] {
        IrContentBlock::ToolUse { id, .. } => {
            assert_eq!(id, "gemini_read"); // original "call_xyz" is lost
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn codex_call_id_lost_through_openai() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: Some("corr_1".into()),
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    }];
    let ir = codex_ir::to_ir(&items);
    let openai = openai_ir::from_ir(&ir);
    let ir2 = openai_ir::to_ir(&openai);
    let codex_out = codex_ir::from_ir(&ir2);
    match &codex_out[0] {
        CodexResponseItem::FunctionCall { call_id, .. } => {
            // call_id (correlation ID) is Codex-specific and lost through OpenAI
            assert!(call_id.is_none());
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Empty / edge case conversations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_openai_to_all_sdks() {
    let ir = openai_ir::to_ir(&[]);
    assert!(claude_ir::from_ir(&ir).is_empty());
    assert!(gemini_ir::from_ir(&ir).is_empty());
    assert!(codex_ir::from_ir(&ir).is_empty());
    assert!(kimi_ir::from_ir(&ir).is_empty());
    assert!(copilot_ir::from_ir(&ir).is_empty());
}

#[test]
fn empty_claude_to_all_sdks() {
    let ir = claude_ir::to_ir(&[], None);
    assert!(openai_ir::from_ir(&ir).is_empty());
    assert!(gemini_ir::from_ir(&ir).is_empty());
    assert!(codex_ir::from_ir(&ir).is_empty());
    assert!(kimi_ir::from_ir(&ir).is_empty());
    assert!(copilot_ir::from_ir(&ir).is_empty());
}

#[test]
fn empty_gemini_to_all_sdks() {
    let ir = gemini_ir::to_ir(&[], None);
    assert!(openai_ir::from_ir(&ir).is_empty());
    assert!(claude_ir::from_ir(&ir).is_empty());
    assert!(codex_ir::from_ir(&ir).is_empty());
    assert!(kimi_ir::from_ir(&ir).is_empty());
    assert!(copilot_ir::from_ir(&ir).is_empty());
}

#[test]
fn openai_none_content_through_claude() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].content, "");
}

#[test]
fn openai_empty_content_through_kimi() {
    let openai = vec![OpenAIMessage {
        role: "user".into(),
        content: Some(String::new()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let kimi = kimi_ir::from_ir(&ir);
    assert!(kimi[0].content.is_none() || kimi[0].content.as_deref() == Some(""));
}

#[test]
fn malformed_tool_args_openai_through_claude() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c_bad".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "foo".into(),
                arguments: "not-json".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { input, .. } => {
            // Malformed args preserved as JSON string value
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Cross-SDK triangular roundtrips (A → IR → B → IR → C)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_to_gemini_text() {
    let openai = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello world".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir1 = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir1);
    let ir2 = claude_ir::to_ir(&claude, None);
    let gemini = gemini_ir::from_ir(&ir2);
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello world"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_to_kimi_to_copilot_text() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Answer".into())],
    }];
    let ir1 = gemini_ir::to_ir(&gemini, None);
    let kimi = kimi_ir::from_ir(&ir1);
    let ir2 = kimi_ir::to_ir(&kimi);
    let copilot = copilot_ir::from_ir(&ir2);
    assert_eq!(copilot[0].content, "Answer");
    assert_eq!(copilot[0].role, "assistant");
}

#[test]
fn claude_to_codex_to_openai_tool_call() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_abc".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    }];
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir1 = claude_ir::to_ir(&claude, None);
    let codex = codex_ir::from_ir(&ir1);
    let ir2 = codex_ir::to_ir(&codex);
    let openai = openai_ir::from_ir(&ir2);
    let tc = &openai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "tu_abc");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn kimi_to_gemini_to_claude_tool_call() {
    let kimi = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "k_call".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"q":"ai"}"#.into(),
            },
        }]),
    }];
    let ir1 = kimi_ir::to_ir(&kimi);
    let gemini = gemini_ir::from_ir(&ir1);
    let ir2 = gemini_ir::to_ir(&gemini, None);
    let claude = claude_ir::from_ir(&ir2);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { name, .. } => {
            assert_eq!(name, "web_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Multiple tool calls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_tool_calls_openai_to_claude() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read".into(),
                    arguments: r#"{"p":"a"}"#.into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "write".into(),
                    arguments: r#"{"p":"b"}"#.into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    assert_eq!(blocks.len(), 2);
}

#[test]
fn multiple_tool_calls_openai_to_kimi() {
    let openai = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "a".into(),
                    arguments: "{}".into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "b".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&openai);
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn multiple_tool_calls_kimi_to_openai() {
    let kimi = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![
            KimiToolCall {
                id: "k1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "x".into(),
                    arguments: "{}".into(),
                },
            },
            KimiToolCall {
                id: "k2".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "y".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].tool_calls.as_ref().unwrap().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
//  IR conversation accessors after cross-SDK lowering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_accessors_after_openai_lowering() {
    let openai = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: "{}".into(),
                },
            }]),
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai);
    assert_eq!(ir.len(), 3);
    assert!(ir.system_message().is_some());
    assert_eq!(ir.system_message().unwrap().text_content(), "sys");
    assert_eq!(ir.tool_calls().len(), 1);
    assert_eq!(ir.messages_by_role(IrRole::User).len(), 1);
    assert!(ir.last_assistant().is_some());
}

#[test]
fn ir_accessors_after_claude_lowering() {
    let tool_use = vec![ClaudeContentBlock::ToolUse {
        id: "tu".into(),
        name: "read".into(),
        input: json!({}),
    }];
    let claude = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "hello".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&tool_use).unwrap(),
        },
    ];
    let ir = claude_ir::to_ir(&claude, Some("prompt"));
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.system_message().unwrap().text_content(), "prompt");
    assert_eq!(ir.tool_calls().len(), 1);
}

#[test]
fn ir_accessors_after_gemini_lowering() {
    let gemini = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hi".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "f".into(),
                args: json!({}),
            }],
        },
    ];
    let ir = gemini_ir::to_ir(&gemini, None);
    assert_eq!(ir.tool_calls().len(), 1);
    assert!(ir.last_assistant().is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Copilot ↔ Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_claude_user_text() {
    let copilot = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello Claude!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hello Claude!");
}

#[test]
fn copilot_to_claude_system_extracted() {
    let copilot = vec![
        CopilotMessage {
            role: "system".into(),
            content: "Be nice".into(),
            name: None,
            copilot_references: vec![],
        },
        CopilotMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: None,
            copilot_references: vec![],
        },
    ];
    let ir = copilot_ir::to_ir(&copilot);
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be nice"));
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1); // system skipped
}

#[test]
fn claude_to_copilot_assistant_text() {
    let claude = vec![ClaudeMessage {
        role: "assistant".into(),
        content: "Sure, Copilot!".into(),
    }];
    let ir = claude_ir::to_ir(&claude, None);
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "assistant");
    assert_eq!(copilot[0].content, "Sure, Copilot!");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Copilot ↔ Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_gemini_user_text() {
    let copilot = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello Gemini!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "user");
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello Gemini!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_to_copilot_model_text() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Hello Copilot!".into())],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "assistant");
    assert_eq!(copilot[0].content, "Hello Copilot!");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Codex ↔ Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_to_claude_assistant_text() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Hi Claude!".into(),
        }],
    }];
    let ir = codex_ir::to_ir(&items);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "assistant");
    assert_eq!(claude[0].content, "Hi Claude!");
}

#[test]
fn codex_to_claude_function_call() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: None,
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    }];
    let ir = codex_ir::to_ir(&items);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Codex ↔ Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_to_gemini_assistant_text() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Hi Gemini!".into(),
        }],
    }];
    let ir = codex_ir::to_ir(&items);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "model");
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hi Gemini!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn codex_to_gemini_function_call() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: None,
        name: "search".into(),
        arguments: r#"{"q":"test"}"#.into(),
    }];
    let ir = codex_ir::to_ir(&items);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "test"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Kimi ↔ Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_to_claude_user_text() {
    let kimi = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello Claude!".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "user");
    assert_eq!(claude[0].content, "Hello Claude!");
}

#[test]
fn kimi_to_claude_system_extracted() {
    let kimi = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("Be smart".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let ir = kimi_ir::to_ir(&kimi);
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be smart"));
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude.len(), 1); // system skipped
}

#[test]
fn kimi_to_claude_tool_call() {
    let kimi = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "k_call".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude[0].content).expect("should parse");
    match &blocks[0] {
        ClaudeContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "k_call");
            assert_eq!(name, "web_search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Kimi ↔ Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_to_gemini_user_text() {
    let kimi = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello Gemini!".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let gemini = gemini_ir::from_ir(&ir);
    assert_eq!(gemini[0].role, "user");
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello Gemini!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn kimi_to_gemini_tool_call() {
    let kimi = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "k1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"ai"}"#.into(),
            },
        }]),
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "ai"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn gemini_to_kimi_function_call() {
    let gemini = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "read".into(),
            args: json!({"file": "a.rs"}),
        }],
    }];
    let ir = gemini_ir::to_ir(&gemini, None);
    let kimi = kimi_ir::from_ir(&ir);
    let tc = &kimi[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "read");
    assert_eq!(tc.id, "gemini_read");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Copilot ↔ Kimi
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_kimi_user_text() {
    let copilot = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello Kimi!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].role, "user");
    assert_eq!(kimi[0].content.as_deref(), Some("Hello Kimi!"));
}

#[test]
fn kimi_to_copilot_assistant_text() {
    let kimi = vec![KimiMessage {
        role: "assistant".into(),
        content: Some("Hello Copilot!".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let ir = kimi_ir::to_ir(&kimi);
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "assistant");
    assert_eq!(copilot[0].content, "Hello Copilot!");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Copilot ↔ Codex
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_to_codex_assistant_text() {
    let copilot = vec![CopilotMessage {
        role: "assistant".into(),
        content: "Done!".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&copilot);
    let codex = codex_ir::from_ir(&ir);
    assert_eq!(codex.len(), 1);
    match &codex[0] {
        CodexResponseItem::Message { role, content } => {
            assert_eq!(role, "assistant");
            match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Done!"),
            }
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn codex_to_copilot_assistant_text() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Hello Copilot!".into(),
        }],
    }];
    let ir = codex_ir::to_ir(&items);
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "assistant");
    assert_eq!(copilot[0].content, "Hello Copilot!");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Direct IR construction → all SDKs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_text_to_all_sdks() {
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")]);

    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].content.as_deref(), Some("Hello"));

    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].content, "Hello");

    let gemini = gemini_ir::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello"),
        other => panic!("expected Text, got {other:?}"),
    }

    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].content.as_deref(), Some("Hello"));

    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].content, "Hello");
}

#[test]
fn ir_tool_use_to_all_sdks() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "read".into(),
            input: json!({"path": "a.rs"}),
        }],
    )]);

    // OpenAI
    let openai = openai_ir::from_ir(&ir);
    let tc = &openai[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "t1");

    // Claude
    let claude = claude_ir::from_ir(&ir);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[0].content).unwrap();
    assert!(matches!(&blocks[0], ClaudeContentBlock::ToolUse { .. }));

    // Gemini
    let gemini = gemini_ir::from_ir(&ir);
    assert!(matches!(
        &gemini[0].parts[0],
        GeminiPart::FunctionCall { .. }
    ));

    // Codex
    let codex = codex_ir::from_ir(&ir);
    assert!(matches!(&codex[0], CodexResponseItem::FunctionCall { .. }));

    // Kimi
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].tool_calls.as_ref().unwrap()[0].id, "t1");

    // Copilot: tool use becomes text (no native tool call in message)
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "assistant");
}

#[test]
fn ir_tool_result_to_all_sdks() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text { text: "ok".into() }],
            is_error: false,
        }],
    )]);

    // OpenAI
    let openai = openai_ir::from_ir(&ir);
    assert_eq!(openai[0].role, "tool");
    assert_eq!(openai[0].tool_call_id.as_deref(), Some("t1"));

    // Claude (tool results as user with structured blocks)
    let claude = claude_ir::from_ir(&ir);
    assert_eq!(claude[0].role, "user");

    // Gemini
    let gemini = gemini_ir::from_ir(&ir);
    assert!(matches!(
        &gemini[0].parts[0],
        GeminiPart::FunctionResponse { .. }
    ));

    // Codex
    let codex = codex_ir::from_ir(&ir);
    assert!(matches!(
        &codex[0],
        CodexResponseItem::FunctionCallOutput { .. }
    ));

    // Kimi
    let kimi = kimi_ir::from_ir(&ir);
    assert_eq!(kimi[0].role, "tool");
    assert_eq!(kimi[0].tool_call_id.as_deref(), Some("t1"));

    // Copilot (tool → user)
    let copilot = copilot_ir::from_ir(&ir);
    assert_eq!(copilot[0].role, "user");
}
