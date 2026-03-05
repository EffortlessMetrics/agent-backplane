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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-validation tests verifying SDK type mappings between all vendor types.
//!
//! Covers type completeness, conversion symmetry through the IR, and schema
//! compatibility across all six dialects (OpenAI, Claude, Gemini, Codex, Kimi,
//! Copilot).

use serde::{Deserialize, Serialize};
use serde_json::json;

// ── SDK types (vendor-specific wire types) ──────────────────────────────
use abp_sdk_types::claude::{
    ClaudeApiError, ClaudeConfig, ClaudeContentBlock, ClaudeMessage, ClaudeRequest, ClaudeResponse,
    ClaudeStreamEvent, ClaudeToolDef, ClaudeUsage,
};
use abp_sdk_types::codex::{
    CodexConfig, CodexContentPart, CodexFunctionDef, CodexInputItem, CodexRequest, CodexResponse,
    CodexResponseItem, CodexStreamEvent, CodexTool, CodexUsage,
};
use abp_sdk_types::copilot::{
    CopilotConfig, CopilotError, CopilotFunctionDef, CopilotMessage, CopilotRequest,
    CopilotResponse, CopilotStreamEvent, CopilotTool,
};
use abp_sdk_types::gemini::{
    GeminiCandidate, GeminiConfig, GeminiContent, GeminiFunctionDeclaration,
    GeminiGenerationConfig, GeminiPart, GeminiRequest, GeminiResponse, GeminiStreamChunk,
    GeminiTool, GeminiUsageMetadata,
};
use abp_sdk_types::kimi::{
    KimiChoice, KimiChunkChoice, KimiChunkDelta, KimiConfig, KimiFunctionDef, KimiMessage,
    KimiRequest, KimiResponse, KimiResponseMessage, KimiStreamChunk, KimiTool, KimiUsage,
};
use abp_sdk_types::openai::{
    OpenAiChoice, OpenAiConfig, OpenAiFunctionCall, OpenAiFunctionDef, OpenAiMessage,
    OpenAiRequest, OpenAiResponse, OpenAiStreamChoice, OpenAiStreamChunk, OpenAiStreamDelta,
    OpenAiToolCall, OpenAiToolDef, OpenAiUsage, ToolChoice, ToolChoiceMode,
};
use abp_sdk_types::{CanonicalToolDef, Dialect, DialectRequest, DialectResponse, ModelConfig};

// ── IR types ────────────────────────────────────────────────────────────
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};

// ── SDK lowering modules ────────────────────────────────────────────────
use abp_claude_sdk::lowering as claude_lowering;
use abp_codex_sdk::lowering as codex_lowering;
use abp_copilot_sdk::lowering as copilot_lowering;
use abp_gemini_sdk::lowering as gemini_lowering;
use abp_kimi_sdk::lowering as kimi_lowering;
use abp_openai_sdk::lowering as openai_lowering;

// ═══════════════════════════════════════════════════════════════════════
// Section A: Type completeness (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_request_has_required_fields() {
    let req = OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAiMessage {
            role: "user".into(),
            content: Some("hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: Some(vec![OpenAiToolDef {
            tool_type: "function".into(),
            function: OpenAiFunctionDef {
                name: "read".into(),
                description: "read file".into(),
                parameters: json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        temperature: Some(0.7),
        max_tokens: Some(4096),
        response_format: None,
        stream: Some(false),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("messages").is_some());
    assert!(json.get("tools").is_some());
    assert!(json.get("temperature").is_some());
    assert!(json.get("max_tokens").is_some());
}

#[test]
fn claude_request_has_required_fields() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: Some("Be helpful.".into()),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "hello".into(),
        }],
        tools: Some(vec![ClaudeToolDef {
            name: "read".into(),
            description: "read file".into(),
            input_schema: json!({"type": "object"}),
        }]),
        thinking: None,
        stream: Some(false),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("max_tokens").is_some());
    assert!(json.get("messages").is_some());
    assert!(json.get("system").is_some());
    assert!(json.get("tools").is_some());
}

#[test]
fn gemini_request_has_required_fields() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hello".into())],
        }],
        system_instruction: None,
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(4096),
            temperature: Some(0.7),
            ..Default::default()
        }),
        safety_settings: None,
        tools: Some(vec![GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "search".into(),
                description: "search".into(),
                parameters: json!({"type": "object"}),
            }],
        }]),
        tool_config: None,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("contents").is_some());
    assert!(json.get("generationConfig").is_some());
    assert!(json.get("tools").is_some());
}

#[test]
fn codex_request_has_required_fields() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Write tests".into(),
        }],
        max_output_tokens: Some(4096),
        temperature: Some(0.5),
        tools: vec![CodexTool::Function {
            function: CodexFunctionDef {
                name: "shell".into(),
                description: "run".into(),
                parameters: json!({"type": "object"}),
            },
        }],
        text: None,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("input").is_some());
    assert!(json.get("max_output_tokens").is_some());
    assert!(json.get("tools").is_some());
}

#[test]
fn kimi_request_has_required_fields() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: Some(4096),
        temperature: Some(0.7),
        stream: None,
        tools: None,
        use_search: Some(true),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("messages").is_some());
    assert!(json.get("max_tokens").is_some());
    assert!(json.get("use_search").is_some());
}

#[test]
fn copilot_request_has_required_fields() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "hello".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: None,
        turn_history: vec![],
        references: vec![],
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_some());
    assert!(json.get("messages").is_some());
}

#[test]
fn response_types_for_each_sdk_are_complete() {
    // OpenAI
    let openai_resp = OpenAiResponse {
        id: "r1".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    assert!(
        serde_json::to_value(&openai_resp)
            .unwrap()
            .get("id")
            .is_some()
    );

    // Claude
    let claude_resp = ClaudeResponse {
        id: "msg_1".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text { text: "hi".into() }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    assert!(
        serde_json::to_value(&claude_resp)
            .unwrap()
            .get("id")
            .is_some()
    );

    // Gemini
    let gemini_resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("hi".into())],
            },
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    assert!(
        serde_json::to_value(&gemini_resp)
            .unwrap()
            .get("candidates")
            .is_some()
    );

    // Codex
    let codex_resp = CodexResponse {
        id: "resp_1".into(),
        model: "codex-mini".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText { text: "hi".into() }],
        }],
        usage: Some(CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
        status: Some("completed".into()),
    };
    assert!(
        serde_json::to_value(&codex_resp)
            .unwrap()
            .get("id")
            .is_some()
    );

    // Kimi
    let kimi_resp = KimiResponse {
        id: "cmpl_1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("hi".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        refs: None,
    };
    assert!(
        serde_json::to_value(&kimi_resp)
            .unwrap()
            .get("id")
            .is_some()
    );

    // Copilot
    let copilot_resp = CopilotResponse {
        message: "hi".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    assert!(
        serde_json::to_value(&copilot_resp)
            .unwrap()
            .get("message")
            .is_some()
    );
}

#[test]
fn tool_types_for_each_sdk_exist() {
    // OpenAI
    let _td = OpenAiToolDef {
        tool_type: "function".into(),
        function: OpenAiFunctionDef {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        },
    };
    let _tc = OpenAiToolCall {
        id: "c".into(),
        call_type: "function".into(),
        function: OpenAiFunctionCall {
            name: "f".into(),
            arguments: "{}".into(),
        },
    };

    // Claude
    let _ct = ClaudeToolDef {
        name: "f".into(),
        description: "d".into(),
        input_schema: json!({}),
    };

    // Gemini
    let _gt = GeminiTool {
        function_declarations: vec![GeminiFunctionDeclaration {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        }],
    };

    // Codex
    let _cx = CodexTool::Function {
        function: CodexFunctionDef {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        },
    };

    // Kimi
    let _kt = KimiTool::Function {
        function: KimiFunctionDef {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        },
    };

    // Copilot
    let _cp = CopilotTool {
        tool_type: abp_sdk_types::copilot::CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        }),
        confirmation: None,
    };
}

#[test]
fn streaming_types_for_each_sdk_exist() {
    let _oai = OpenAiStreamChunk {
        id: "c".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAiStreamChoice {
            index: 0,
            delta: OpenAiStreamDelta::default(),
            finish_reason: None,
        }],
        usage: None,
    };

    let _claude = ClaudeStreamEvent::Ping {};

    let _gemini = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: None,
    };

    let _codex = CodexStreamEvent::Error {
        message: "test".into(),
        code: None,
    };

    let _kimi = KimiStreamChunk {
        id: "c".into(),
        object: "chat.completion.chunk".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };

    let _copilot = CopilotStreamEvent::Done {};
}

#[test]
fn error_types_for_each_sdk_exist() {
    // Claude has ClaudeApiError
    let _ce = ClaudeApiError {
        error_type: "invalid_request_error".into(),
        message: "bad request".into(),
    };
    let json = serde_json::to_value(&_ce).unwrap();
    assert!(json.get("message").is_some());

    // Copilot has CopilotError
    let _cp = CopilotError {
        error_type: "error".into(),
        message: "something failed".into(),
        code: Some("500".into()),
    };
    let json = serde_json::to_value(&_cp).unwrap();
    assert!(json.get("message").is_some());

    // Codex stream has error variant
    let ce = CodexStreamEvent::Error {
        message: "stream error".into(),
        code: Some("ERR".into()),
    };
    let json = serde_json::to_value(&ce).unwrap();
    assert!(json.get("message").is_some());

    // Claude stream has error variant
    let cse = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "server_error".into(),
            message: "oops".into(),
        },
    };
    let json = serde_json::to_value(&cse).unwrap();
    assert!(json.get("error").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// Section B: Type conversion symmetry (10 tests)
// ═══════════════════════════════════════════════════════════════════════

fn make_openai_messages() -> Vec<abp_openai_sdk::dialect::OpenAIMessage> {
    vec![
        abp_openai_sdk::dialect::OpenAIMessage {
            role: "system".into(),
            content: Some("You are helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        abp_openai_sdk::dialect::OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        abp_openai_sdk::dialect::OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hi there!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ]
}

fn make_claude_messages() -> (
    Vec<abp_claude_sdk::dialect::ClaudeMessage>,
    Option<&'static str>,
) {
    (
        vec![
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "Hello".into(),
            },
            abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "Hi there!".into(),
            },
        ],
        Some("You are helpful."),
    )
}

fn make_gemini_contents() -> Vec<abp_gemini_sdk::dialect::GeminiContent> {
    vec![
        abp_gemini_sdk::dialect::GeminiContent {
            role: "user".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hello".into())],
        },
        abp_gemini_sdk::dialect::GeminiContent {
            role: "model".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("Hi!".into())],
        },
    ]
}

fn make_codex_items() -> Vec<abp_codex_sdk::dialect::CodexResponseItem> {
    vec![abp_codex_sdk::dialect::CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![abp_codex_sdk::dialect::CodexContentPart::OutputText {
            text: "Done!".into(),
        }],
    }]
}

fn make_kimi_messages() -> Vec<abp_kimi_sdk::dialect::KimiMessage> {
    vec![
        abp_kimi_sdk::dialect::KimiMessage {
            role: "system".into(),
            content: Some("You are helpful.".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        abp_kimi_sdk::dialect::KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        abp_kimi_sdk::dialect::KimiMessage {
            role: "assistant".into(),
            content: Some("Hi there!".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ]
}

fn make_copilot_messages() -> Vec<abp_copilot_sdk::dialect::CopilotMessage> {
    vec![
        abp_copilot_sdk::dialect::CopilotMessage {
            role: "system".into(),
            content: "You are helpful.".into(),
            name: None,
            copilot_references: vec![],
        },
        abp_copilot_sdk::dialect::CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            copilot_references: vec![],
        },
        abp_copilot_sdk::dialect::CopilotMessage {
            role: "assistant".into(),
            content: "Hi there!".into(),
            name: None,
            copilot_references: vec![],
        },
    ]
}

#[test]
fn openai_to_ir_preserves_message_count() {
    let msgs = make_openai_messages();
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), msgs.len());
    // Verify roles map correctly
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
}

#[test]
fn claude_to_ir_preserves_message_count() {
    let (msgs, sys) = make_claude_messages();
    let conv = claude_lowering::to_ir(&msgs, sys);
    // Claude lowering prepends system prompt as an extra IrMessage
    assert_eq!(conv.len(), msgs.len() + 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
}

#[test]
fn ir_to_openai_preserves_message_count() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ]);
    let msgs = openai_lowering::from_ir(&conv);
    assert_eq!(msgs.len(), conv.len());
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
    assert_eq!(msgs[2].role, "assistant");
}

#[test]
fn ir_to_claude_preserves_message_count() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ]);
    // Claude from_ir skips system messages
    let msgs = claude_lowering::from_ir(&conv);
    assert_eq!(msgs.len(), 2); // system skipped
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");
    // System prompt extracted separately
    let sys = claude_lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("instructions"));
}

#[test]
fn gemini_to_ir_to_gemini_roundtrip() {
    let contents = make_gemini_contents();
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);

    assert_eq!(back.len(), contents.len());
    for (original, roundtripped) in contents.iter().zip(back.iter()) {
        assert_eq!(original.role, roundtripped.role);
        // Verify text parts match
        let orig_text: String = original
            .parts
            .iter()
            .filter_map(|p| match p {
                abp_gemini_sdk::dialect::GeminiPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let rt_text: String = roundtripped
            .parts
            .iter()
            .filter_map(|p| match p {
                abp_gemini_sdk::dialect::GeminiPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(orig_text, rt_text);
    }
}

#[test]
fn codex_to_ir_to_codex_roundtrip() {
    let items = make_codex_items();
    let conv = codex_lowering::to_ir(&items);
    let back = codex_lowering::from_ir(&conv);

    assert_eq!(back.len(), items.len());
    match (&items[0], &back[0]) {
        (
            abp_codex_sdk::dialect::CodexResponseItem::Message {
                role: r1,
                content: c1,
            },
            abp_codex_sdk::dialect::CodexResponseItem::Message {
                role: r2,
                content: c2,
            },
        ) => {
            assert_eq!(r1, r2);
            assert_eq!(c1.len(), c2.len());
        }
        _ => panic!("expected Message items"),
    }
}

#[test]
fn kimi_to_ir_to_kimi_roundtrip() {
    let msgs = make_kimi_messages();
    let conv = kimi_lowering::to_ir(&msgs);
    let back = kimi_lowering::from_ir(&conv);

    assert_eq!(back.len(), msgs.len());
    for (original, roundtripped) in msgs.iter().zip(back.iter()) {
        assert_eq!(original.role, roundtripped.role);
        assert_eq!(original.content, roundtripped.content);
    }
}

#[test]
fn copilot_to_ir_to_copilot_roundtrip() {
    let msgs = make_copilot_messages();
    let conv = copilot_lowering::to_ir(&msgs);
    let back = copilot_lowering::from_ir(&conv);

    assert_eq!(back.len(), msgs.len());
    for (original, roundtripped) in msgs.iter().zip(back.iter()) {
        assert_eq!(original.role, roundtripped.role);
        assert_eq!(original.content, roundtripped.content);
    }
}

#[test]
fn cross_vendor_openai_to_ir_to_claude_compatible() {
    let oai_msgs = make_openai_messages();
    let conv = openai_lowering::to_ir(&oai_msgs);

    // Verify IR is well-formed
    assert!(conv.system_message().is_some());
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "You are helpful."
    );

    // Lower to Claude
    let claude_msgs = claude_lowering::from_ir(&conv);
    let sys = claude_lowering::extract_system_prompt(&conv);

    assert_eq!(sys.as_deref(), Some("You are helpful."));
    // Claude skips system so user+assistant remain
    assert_eq!(claude_msgs.len(), 2);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Hello");
    assert_eq!(claude_msgs[1].role, "assistant");
    assert_eq!(claude_msgs[1].content, "Hi there!");
}

#[test]
fn cross_vendor_claude_to_ir_to_gemini_compatible() {
    let (claude_msgs, sys) = make_claude_messages();
    let conv = claude_lowering::to_ir(&claude_msgs, sys);

    // Lower to Gemini
    let gemini_contents = gemini_lowering::from_ir(&conv);
    let gemini_sys = gemini_lowering::extract_system_instruction(&conv);

    // System is extracted separately for Gemini
    assert!(gemini_sys.is_some());
    // Remaining messages: user + assistant
    assert_eq!(gemini_contents.len(), 2);
    assert_eq!(gemini_contents[0].role, "user");
    assert_eq!(gemini_contents[1].role, "model");

    // Verify text preserved
    match &gemini_contents[0].parts[0] {
        abp_gemini_sdk::dialect::GeminiPart::Text(t) => assert_eq!(t, "Hello"),
        other => panic!("expected Text, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Section C: Schema compatibility (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_types_serialize_to_valid_json_schema() {
    let req = OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAiMessage {
            role: "user".into(),
            content: Some("test".into()),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.is_object());
    assert_eq!(val["model"], "gpt-4o");
    assert!(val["messages"].is_array());
}

#[test]
fn claude_types_serialize_to_valid_json_schema() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: None,
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "test".into(),
        }],
        tools: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.is_object());
    assert_eq!(val["model"], "claude-sonnet-4-20250514");
    assert_eq!(val["max_tokens"], 4096);
}

#[test]
fn gemini_types_serialize_to_valid_json_schema() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("test".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.is_object());
    assert_eq!(val["model"], "gemini-2.5-flash");
    assert!(val["contents"].is_array());
}

#[test]
fn all_types_implement_clone_debug() {
    // Clone + Debug are required for all major types
    fn assert_clone_debug<T: Clone + std::fmt::Debug>(_: &T) {}

    let oai = OpenAiRequest {
        model: "m".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    };
    assert_clone_debug(&oai);
    assert_clone_debug(&oai.clone());

    let claude = ClaudeRequest {
        model: "m".into(),
        max_tokens: 1,
        system: None,
        messages: vec![],
        tools: None,
        thinking: None,
        stream: None,
    };
    assert_clone_debug(&claude);

    let gemini = GeminiRequest {
        model: "m".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    assert_clone_debug(&gemini);

    let codex = CodexRequest {
        model: "m".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
    };
    assert_clone_debug(&codex);

    let kimi = KimiRequest {
        model: "m".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    assert_clone_debug(&kimi);

    let copilot = CopilotRequest {
        model: "m".into(),
        messages: vec![],
        tools: None,
        turn_history: vec![],
        references: vec![],
    };
    assert_clone_debug(&copilot);

    // IR types
    let ir_conv = IrConversation::new();
    assert_clone_debug(&ir_conv);
    let ir_msg = IrMessage::text(IrRole::User, "hi");
    assert_clone_debug(&ir_msg);
    let ir_usage = IrUsage::default();
    assert_clone_debug(&ir_usage);
}

#[test]
fn all_types_implement_serialize_deserialize() {
    fn assert_serde<T: Serialize + for<'de> Deserialize<'de>>() {}

    assert_serde::<OpenAiRequest>();
    assert_serde::<OpenAiResponse>();
    assert_serde::<ClaudeRequest>();
    assert_serde::<ClaudeResponse>();
    assert_serde::<GeminiRequest>();
    assert_serde::<GeminiResponse>();
    assert_serde::<CodexRequest>();
    assert_serde::<CodexResponse>();
    assert_serde::<KimiRequest>();
    assert_serde::<KimiResponse>();
    assert_serde::<CopilotRequest>();
    assert_serde::<CopilotResponse>();
    assert_serde::<IrConversation>();
    assert_serde::<IrMessage>();
    assert_serde::<IrUsage>();
    assert_serde::<ModelConfig>();
    assert_serde::<DialectRequest>();
    assert_serde::<DialectResponse>();
    assert_serde::<CanonicalToolDef>();
}

#[test]
fn default_trait_implementations() {
    // Types that implement Default
    let _mc = ModelConfig::default();
    assert!(_mc.model.is_empty());

    let _oai_cfg = OpenAiConfig::default();
    assert_eq!(_oai_cfg.model, "gpt-4o");

    let _claude_cfg = ClaudeConfig::default();
    assert!(_claude_cfg.model.contains("claude"));

    let _gemini_cfg = GeminiConfig::default();
    assert!(_gemini_cfg.model.contains("gemini"));

    let _codex_cfg = CodexConfig::default();
    assert!(_codex_cfg.model.contains("codex"));

    let _kimi_cfg = KimiConfig::default();
    assert!(_kimi_cfg.model.contains("moonshot"));

    let _copilot_cfg = CopilotConfig::default();
    assert_eq!(_copilot_cfg.model, "gpt-4o");

    let _ir_usage = IrUsage::default();
    assert_eq!(_ir_usage.input_tokens, 0);
    assert_eq!(_ir_usage.output_tokens, 0);

    let _ir_conv = IrConversation::default();
    assert!(_ir_conv.is_empty());

    let _gen_cfg = GeminiGenerationConfig::default();
    assert!(_gen_cfg.max_output_tokens.is_none());
}

#[test]
fn type_size_assertions_no_unintended_bloat() {
    // Ensure key types stay within reasonable size bounds.
    // These are sanity checks — not strict ABI guarantees.
    assert!(
        std::mem::size_of::<IrRole>() <= 8,
        "IrRole should be small enum"
    );
    assert!(
        std::mem::size_of::<IrContentBlock>() <= 256,
        "IrContentBlock should stay compact"
    );
    assert!(
        std::mem::size_of::<IrMessage>() <= 256,
        "IrMessage should stay compact"
    );
    assert!(
        std::mem::size_of::<Dialect>() <= 8,
        "Dialect should be small enum"
    );
    assert!(
        std::mem::size_of::<IrUsage>() <= 64,
        "IrUsage should be compact"
    );
    assert!(
        std::mem::size_of::<ModelConfig>() <= 256,
        "ModelConfig should stay compact"
    );
}

#[test]
fn empty_minimal_instances_are_valid() {
    // Minimal OpenAI request
    let oai = OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    };
    let json = serde_json::to_string(&oai).unwrap();
    let _back: OpenAiRequest = serde_json::from_str(&json).unwrap();

    // Minimal Claude request
    let claude = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1,
        system: None,
        messages: vec![],
        tools: None,
        thinking: None,
        stream: None,
    };
    let json = serde_json::to_string(&claude).unwrap();
    let _back: ClaudeRequest = serde_json::from_str(&json).unwrap();

    // Minimal IR
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert!(back.is_empty());
}

#[test]
fn types_with_all_optional_fields_set() {
    // OpenAI with everything populated
    let oai = OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAiMessage {
            role: "user".into(),
            content: Some("hi".into()),
            tool_calls: Some(vec![]),
            tool_call_id: Some("tc_1".into()),
        }],
        tools: Some(vec![]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Required)),
        temperature: Some(0.9),
        max_tokens: Some(8192),
        response_format: Some(abp_sdk_types::openai::ResponseFormat::JsonObject {}),
        stream: Some(true),
    };
    let json = serde_json::to_string(&oai).unwrap();
    let back: OpenAiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.temperature, Some(0.9));
    assert_eq!(back.max_tokens, Some(8192));
    assert_eq!(back.stream, Some(true));

    // Kimi with all optional fields
    let kimi = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        max_tokens: Some(4096),
        temperature: Some(0.5),
        stream: Some(true),
        tools: Some(vec![]),
        use_search: Some(true),
    };
    let json = serde_json::to_string(&kimi).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.use_search, Some(true));
    assert_eq!(back.stream, Some(true));
}

#[test]
fn dialect_enum_covers_all_vendors() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
    let labels: Vec<&str> = all.iter().map(|d| d.label()).collect();
    assert!(labels.contains(&"OpenAI"));
    assert!(labels.contains(&"Claude"));
    assert!(labels.contains(&"Gemini"));
    assert!(labels.contains(&"Kimi"));
    assert!(labels.contains(&"Codex"));
    assert!(labels.contains(&"Copilot"));

    // DialectRequest covers all variants
    let _oai = DialectRequest::OpenAi(OpenAiRequest {
        model: "m".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    });
    assert_eq!(_oai.dialect(), Dialect::OpenAi);

    let _cl = DialectRequest::Claude(ClaudeRequest {
        model: "m".into(),
        max_tokens: 1,
        system: None,
        messages: vec![],
        tools: None,
        thinking: None,
        stream: None,
    });
    assert_eq!(_cl.dialect(), Dialect::Claude);

    let _ge = DialectRequest::Gemini(GeminiRequest {
        model: "m".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    });
    assert_eq!(_ge.dialect(), Dialect::Gemini);

    let _co = DialectRequest::Codex(CodexRequest {
        model: "m".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
    });
    assert_eq!(_co.dialect(), Dialect::Codex);

    let _ki = DialectRequest::Kimi(KimiRequest {
        model: "m".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    });
    assert_eq!(_ki.dialect(), Dialect::Kimi);

    let _cp = DialectRequest::Copilot(CopilotRequest {
        model: "m".into(),
        messages: vec![],
        tools: None,
        turn_history: vec![],
        references: vec![],
    });
    assert_eq!(_cp.dialect(), Dialect::Copilot);
}
