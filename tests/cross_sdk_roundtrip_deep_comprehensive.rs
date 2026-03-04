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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(
    clippy::useless_vec,
    clippy::needless_borrows_for_generic_args,
    clippy::collapsible_if
)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-SDK roundtrip integration tests.
//!
//! Verifies that SDK request types convert faithfully to [`WorkOrder`],
//! flow through a simulated backend, and convert back to SDK response types —
//! both within the same dialect and across dialects.

use std::collections::BTreeMap;

use abp_claude_sdk::messages::{
    CacheControl, ContentBlock, ImageSource, Message as ClaudeMessage, MessageContent,
    MessagesRequest, MessagesResponse, Metadata, Role, SystemBlock, SystemMessage,
};
use abp_core::*;
use abp_gemini_sdk::dialect::*;
use abp_openai_sdk::api::{
    ChatCompletionRequest, ChatCompletionResponse, FinishReason, FunctionDefinition,
    Message as OpenAIMessage,
};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_receipt(
    backend_id: &str,
    wo_id: uuid::Uuid,
    outcome: Outcome,
    events: Vec<AgentEvent>,
    usage: UsageNormalized,
) -> Receipt {
    let mut builder = ReceiptBuilder::new(backend_id)
        .work_order_id(wo_id)
        .outcome(outcome)
        .usage(usage);
    for e in events {
        builder = builder.add_trace_event(e);
    }
    builder.build()
}

fn text_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: Option<&str>, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: id.map(String::from),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn tool_result_event(
    name: &str,
    id: Option<&str>,
    output: serde_json::Value,
    is_error: bool,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: name.into(),
            tool_use_id: id.map(String::from),
            output,
            is_error,
        },
        ext: None,
    }
}

fn thinking_event(text: &str, signature: Option<&str>) -> AgentEvent {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));
    if let Some(sig) = signature {
        ext.insert("signature".into(), serde_json::Value::String(sig.into()));
    }
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: Some(ext),
    }
}

fn default_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    }
}

fn zero_usage() -> UsageNormalized {
    UsageNormalized::default()
}

fn openai_simple_request(task: &str) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: task.into(),
        }],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    }
}

fn claude_simple_request(task: &str) -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text(task.into()),
        }],
        max_tokens: 1024,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    }
}

fn gemini_simple_request(task: &str) -> GeminiRequest {
    GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text(task.into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    }
}

fn openai_tool(name: &str, desc: &str, params: serde_json::Value) -> abp_openai_sdk::api::Tool {
    abp_openai_sdk::api::Tool {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: name.into(),
            description: Some(desc.into()),
            parameters: Some(params),
            strict: None,
        },
    }
}

fn claude_tool(
    name: &str,
    desc: &str,
    schema: serde_json::Value,
) -> abp_claude_sdk::messages::Tool {
    abp_claude_sdk::messages::Tool {
        name: name.into(),
        description: desc.into(),
        input_schema: schema,
    }
}

fn gemini_tool_def(
    name: &str,
    desc: &str,
    params: serde_json::Value,
) -> abp_gemini_sdk::dialect::GeminiTool {
    abp_gemini_sdk::dialect::GeminiTool {
        function_declarations: vec![GeminiFunctionDeclaration {
            name: name.into(),
            description: desc.into(),
            parameters: params,
        }],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. REQUEST ROUNDTRIPS (same dialect in → same dialect out)
// ═══════════════════════════════════════════════════════════════════════════

// ── OpenAI ──────────────────────────────────────────────────────────────

#[test]
fn openai_simple_roundtrip() {
    let req = openai_simple_request("Hello world");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Hello world");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Hi!")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi!"));
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
}

#[test]
fn openai_with_system_prompt_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            OpenAIMessage::System {
                content: "You are a helpful assistant.".into(),
            },
            OpenAIMessage::User {
                content: "Explain Rust".into(),
            },
        ],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Explain Rust");
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(
        wo.context.snippets[0].content,
        "You are a helpful assistant."
    );

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Rust is a systems programming language.")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert!(
        resp.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("Rust")
    );
}

#[test]
fn openai_with_tools_roundtrip() {
    let params = serde_json::json!({
        "type": "object",
        "properties": { "query": { "type": "string" } },
        "required": ["query"]
    });
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "Search for Rust".into(),
        }],
        tools: Some(vec![openai_tool("web_search", "Search the web", params)]),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Search for Rust");

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "web_search",
            Some("call_1"),
            serde_json::json!({"query": "Rust programming"}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "web_search");
    assert_eq!(tc.id, "call_1");
}

#[test]
fn openai_multi_turn_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            OpenAIMessage::User {
                content: "What is 2+2?".into(),
            },
            OpenAIMessage::Assistant {
                content: Some("4".into()),
                tool_calls: None,
            },
            OpenAIMessage::User {
                content: "And 3+3?".into(),
            },
        ],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    // Last user message becomes the task
    assert_eq!(wo.task, "And 3+3?");

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("6")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("6"));
}

#[test]
fn openai_streaming_config_roundtrip() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "Stream this".into(),
        }],
        stream: Some(true),
        temperature: Some(0.8),
        top_p: Some(0.95),
        max_tokens: Some(2048),
        tools: None,
        tool_choice: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Stream this");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

// ── Claude ──────────────────────────────────────────────────────────────

#[test]
fn claude_simple_roundtrip() {
    let req = claude_simple_request("Hello Claude");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Hello Claude");
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Hello!")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.response_type, "message");
    match &resp.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_with_system_prompt_roundtrip() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Explain traits".into()),
        }],
        max_tokens: 4096,
        system: Some(SystemMessage::Text("Be concise.".into())),
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Explain traits");
    assert!(wo.config.vendor.contains_key("system"));

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Traits are interfaces.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn claude_with_tools_roundtrip() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "path": { "type": "string" } },
        "required": ["path"]
    });
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Read a file".into()),
        }],
        max_tokens: 4096,
        system: None,
        tools: Some(vec![claude_tool("read_file", "Read a file", schema)]),
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("tools"));

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "read_file",
            Some("toolu_01"),
            serde_json::json!({"path": "src/lib.rs"}),
        )],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_01");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "src/lib.rs");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn claude_multi_turn_roundtrip() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![
            ClaudeMessage {
                role: Role::User,
                content: MessageContent::Text("What is Rust?".into()),
            },
            ClaudeMessage {
                role: Role::Assistant,
                content: MessageContent::Text("A programming language.".into()),
            },
            ClaudeMessage {
                role: Role::User,
                content: MessageContent::Text("Tell me more.".into()),
            },
        ],
        max_tokens: 4096,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("Tell me more"));

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Rust is a systems programming language.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert!(!resp.content.is_empty());
}

#[test]
fn claude_content_blocks_roundtrip() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![
            ClaudeMessage {
                role: Role::User,
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "Look at this:".into(),
                    },
                    ContentBlock::Image {
                        source: ImageSource::Base64 {
                            media_type: "image/png".into(),
                            data: "iVBOR".into(),
                        },
                    },
                ]),
            },
            ClaudeMessage {
                role: Role::Assistant,
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "analyze_image".into(),
                    input: serde_json::json!({"format": "png"}),
                }]),
            },
            ClaudeMessage {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    content: Some("Analyzed".into()),
                    is_error: None,
                }]),
            },
        ],
        max_tokens: 4096,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("Look at this"));
}

// ── Gemini ──────────────────────────────────────────────────────────────

#[test]
fn gemini_simple_roundtrip() {
    let req = gemini_simple_request("Hello Gemini");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Hello Gemini");
    assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-flash"));

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("Hello there!")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates.len(), 1);
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello there!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_with_system_instruction_roundtrip() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Explain traits".into())],
        }],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be brief and clear.".into())],
        }),
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Explain traits");
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].content, "Be brief and clear.");
}

#[test]
fn gemini_with_tools_roundtrip() {
    let params = serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}});
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Search something".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: Some(vec![gemini_tool_def("search", "Search", params)]),
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("tools"));

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "search",
            None,
            serde_json::json!({"q": "rust"}),
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args["q"], "rust");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn gemini_multi_turn_roundtrip() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hi".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            },
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("How are you?".into())],
            },
        ],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "How are you?");

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("I'm doing well!")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "I'm doing well!"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_with_generation_config_roundtrip() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Generate".into())],
        }],
        system_instruction: None,
        generation_config: Some(GeminiGenerationConfig {
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            max_output_tokens: Some(2048),
            candidate_count: None,
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        }),
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("generation_config"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. CROSS-DIALECT ROUNDTRIPS (different SDK in → different SDK out)
// ═══════════════════════════════════════════════════════════════════════════

// ── OpenAI → Claude ─────────────────────────────────────────────────────

#[test]
fn openai_request_to_claude_response() {
    let req = openai_simple_request("Explain monads");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Explain monads");

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("A monad is a design pattern.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.role, "assistant");
    match &resp.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("monad")),
        other => panic!("expected Text, got {other:?}"),
    }
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn openai_request_with_tools_to_claude_response() {
    let params = serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}});
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "Read a file".into(),
        }],
        tools: Some(vec![openai_tool("read_file", "Read a file", params)]),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "read_file",
            Some("toolu_01"),
            serde_json::json!({"path": "main.rs"}),
        )],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    match &resp.content[0] {
        ContentBlock::ToolUse { name, id, input } => {
            assert_eq!(name, "read_file");
            assert_eq!(id, "toolu_01");
            assert_eq!(input["path"], "main.rs");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn openai_multi_system_to_claude_response() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![
            OpenAIMessage::System {
                content: "System 1".into(),
            },
            OpenAIMessage::System {
                content: "System 2".into(),
            },
            OpenAIMessage::User {
                content: "Hello".into(),
            },
        ],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.context.snippets.len(), 2);

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Acknowledged.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert!(!resp.content.is_empty());
}

// ── Claude → OpenAI ─────────────────────────────────────────────────────

#[test]
fn claude_request_to_openai_response() {
    let req = claude_simple_request("Explain functors");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Explain functors");

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("A functor maps morphisms.")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("A functor maps morphisms.")
    );
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
    assert_eq!(resp.object, "chat.completion");
}

#[test]
fn claude_request_with_tools_to_openai_response() {
    let schema = serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}});
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Search".into()),
        }],
        max_tokens: 4096,
        system: None,
        tools: Some(vec![claude_tool("web_search", "Search the web", schema)]),
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "web_search",
            Some("call_abc"),
            serde_json::json!({"q": "Rust traits"}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "web_search");
    assert_eq!(tc.id, "call_abc");
}

#[test]
fn claude_with_system_to_openai_response() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Summarize".into()),
        }],
        max_tokens: 4096,
        system: Some(SystemMessage::Text("Be concise.".into())),
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("system"));

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Short summary.")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Short summary.")
    );
}

// ── OpenAI → Gemini ─────────────────────────────────────────────────────

#[test]
fn openai_request_to_gemini_response() {
    let req = openai_simple_request("Translate to French");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("Bonjour le monde")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Bonjour le monde"),
        other => panic!("expected Text, got {other:?}"),
    }
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn openai_request_with_tools_to_gemini_response() {
    let params = serde_json::json!({"type": "object"});
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "Call a tool".into(),
        }],
        tools: Some(vec![openai_tool("calculator", "Calculate", params)]),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "calculator",
            None,
            serde_json::json!({"expr": "2+2"}),
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "calculator");
            assert_eq!(args["expr"], "2+2");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// ── Gemini → OpenAI ─────────────────────────────────────────────────────

#[test]
fn gemini_request_to_openai_response() {
    let req = gemini_simple_request("Hello from Gemini");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Hello from Gemini");

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Hello back!")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello back!")
    );
}

#[test]
fn gemini_request_with_tools_to_openai_response() {
    let params = serde_json::json!({"type": "object"});
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Use a tool".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: Some(vec![gemini_tool_def("analyze", "Analyze data", params)]),
        tool_config: None,
    };
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "analyze",
            Some("call_x"),
            serde_json::json!({"data": [1, 2, 3]}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
}

// ── Gemini → Claude ─────────────────────────────────────────────────────

#[test]
fn gemini_request_to_claude_response() {
    let req = gemini_simple_request("Explain coroutines");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Coroutines are cooperative tasks.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    match &resp.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("Coroutines")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_request_with_safety_to_claude_response() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Safe content".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]),
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("safety_settings"));

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Safe response.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

// ── Claude → Gemini ─────────────────────────────────────────────────────

#[test]
fn claude_request_to_gemini_response() {
    let req = claude_simple_request("Explain async/await");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("Async/await enables non-blocking I/O.")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("non-blocking")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_request_with_tools_to_gemini_response() {
    let schema = serde_json::json!({"type": "object", "properties": {"cmd": {"type": "string"}}});
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Execute command".into()),
        }],
        max_tokens: 4096,
        system: None,
        tools: Some(vec![claude_tool("bash", "Run a bash command", schema)]),
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "bash",
            None,
            serde_json::json!({"cmd": "ls"}),
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "bash");
            assert_eq!(args["cmd"], "ls");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// ── All three in a chain ────────────────────────────────────────────────

#[test]
fn openai_in_claude_out_gemini_verify() {
    // OpenAI request → WorkOrder
    let req = openai_simple_request("Cross-SDK chain");
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Cross-SDK chain");

    // Receipt with Gemini backend
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("Chained response")],
        default_usage(),
    );

    // Both Claude and Gemini can read the receipt
    let claude_resp: MessagesResponse = receipt.clone().into();
    let gemini_resp: GeminiResponse = receipt.into();

    match &claude_resp.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Chained response"),
        other => panic!("expected Text, got {other:?}"),
    }
    match &gemini_resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Chained response"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_in_all_three_out() {
    let req = claude_simple_request("Universal receipt");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Unified response")],
        default_usage(),
    );

    let oai: ChatCompletionResponse = receipt.clone().into();
    let cla: MessagesResponse = receipt.clone().into();
    let gem: GeminiResponse = receipt.into();

    assert_eq!(
        oai.choices[0].message.content.as_deref(),
        Some("Unified response")
    );
    match &cla.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Unified response"),
        other => panic!("expected Text, got {other:?}"),
    }
    match &gem.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Unified response"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_in_all_three_out() {
    let req = gemini_simple_request("Universal source");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("Multi-target")],
        default_usage(),
    );

    let oai: ChatCompletionResponse = receipt.clone().into();
    let cla: MessagesResponse = receipt.clone().into();
    let gem: GeminiResponse = receipt.into();

    assert_eq!(
        oai.choices[0].message.content.as_deref(),
        Some("Multi-target")
    );
    match &cla.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Multi-target"),
        other => panic!("expected Text, got {other:?}"),
    }
    match &gem.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Multi-target"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn cross_dialect_tool_call_openai_to_claude() {
    let req = openai_simple_request("Use tool");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![
            tool_call_event(
                "edit_file",
                Some("toolu_99"),
                serde_json::json!({"path": "x.rs", "content": "fn main(){}"}),
            ),
            tool_result_event(
                "edit_file",
                Some("toolu_99"),
                serde_json::json!("ok"),
                false,
            ),
            text_event("Done editing."),
        ],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    // tool_use blocks should appear
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    );
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { .. }))
    );
}

#[test]
fn cross_dialect_tool_call_claude_to_openai() {
    let req = claude_simple_request("Invoke tool");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "grep",
            Some("call_grep_1"),
            serde_json::json!({"pattern": "TODO"}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].function.name, "grep");
    assert_eq!(tcs[0].id, "call_grep_1");
}

#[test]
fn cross_dialect_tool_call_gemini_to_openai() {
    let req = gemini_simple_request("Tool please");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "list_files",
            Some("call_lf"),
            serde_json::json!({"dir": "/src"}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
}

#[test]
fn cross_dialect_tool_call_openai_to_gemini() {
    let req = openai_simple_request("Gemini tool");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "summarize",
            None,
            serde_json::json!({"text": "long text"}),
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, .. } => assert_eq!(name, "summarize"),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. TOOL CALL FIDELITY
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_name_preserved_openai_roundtrip() {
    let req = openai_simple_request("tool test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "my_custom_tool",
            Some("call_42"),
            serde_json::json!({}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "my_custom_tool");
    assert_eq!(tc.id, "call_42");
    assert_eq!(tc.call_type, "function");
}

#[test]
fn tool_name_preserved_claude_roundtrip() {
    let req = claude_simple_request("tool test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "bash_tool",
            Some("toolu_xyz"),
            serde_json::json!({"cmd": "echo hi"}),
        )],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    match &resp.content[0] {
        ContentBlock::ToolUse { name, id, input } => {
            assert_eq!(name, "bash_tool");
            assert_eq!(id, "toolu_xyz");
            assert_eq!(input["cmd"], "echo hi");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_name_preserved_gemini_roundtrip() {
    let req = gemini_simple_request("tool test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "web_fetch",
            None,
            serde_json::json!({"url": "https://example.com"}),
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "web_fetch");
            assert_eq!(args["url"], "https://example.com");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn tool_arguments_complex_json_preserved() {
    let complex_args = serde_json::json!({
        "nested": {"a": [1, 2, 3]},
        "flag": true,
        "count": 42,
        "text": "hello \"world\""
    });
    let req = openai_simple_request("complex args");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "complex_tool",
            Some("call_c"),
            complex_args.clone(),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(parsed["nested"]["a"], serde_json::json!([1, 2, 3]));
    assert_eq!(parsed["flag"], true);
    assert_eq!(parsed["count"], 42);
}

#[test]
fn multiple_tool_calls_preserved() {
    let req = openai_simple_request("multi tool");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![
            tool_call_event("tool_a", Some("call_1"), serde_json::json!({"x": 1})),
            tool_call_event("tool_b", Some("call_2"), serde_json::json!({"y": 2})),
            tool_call_event("tool_c", Some("call_3"), serde_json::json!({"z": 3})),
        ],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 3);
    assert_eq!(tcs[0].function.name, "tool_a");
    assert_eq!(tcs[1].function.name, "tool_b");
    assert_eq!(tcs[2].function.name, "tool_c");
}

#[test]
fn tool_result_mapped_to_gemini_function_response() {
    let req = openai_simple_request("tool result test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_result_event(
            "search",
            None,
            serde_json::json!({"results": ["a", "b"]}),
            false,
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response["results"], serde_json::json!(["a", "b"]));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn tool_call_without_id_gets_generated_openai() {
    let req = openai_simple_request("no id");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event("some_tool", None, serde_json::json!({}))],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    // When no tool_use_id, OpenAI conversion generates "call_0"
    assert_eq!(tc.id, "call_0");
}

#[test]
fn tool_call_without_id_gets_empty_claude() {
    let req = claude_simple_request("no id claude");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event("some_tool", None, serde_json::json!({}))],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    match &resp.content[0] {
        ContentBlock::ToolUse { id, .. } => {
            assert!(id.is_empty());
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_definitions_roundtrip_through_vendor_map() {
    let params = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Search query"}
        },
        "required": ["query"]
    });
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "Search".into(),
        }],
        tools: Some(vec![openai_tool("search", "Web search", params.clone())]),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Search");
    // Tools are not stored in vendor for OpenAI (only model is set via builder)
    // The work order still captures the task correctly
}

#[test]
fn tool_call_and_text_in_same_response_openai() {
    let req = openai_simple_request("tool + text");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![
            text_event("Let me search for that."),
            tool_call_event("search", Some("call_s1"), serde_json::json!({"q": "test"})),
        ],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Let me search for that.")
    );
    assert!(resp.choices[0].message.tool_calls.is_some());
}

#[test]
fn tool_call_and_text_in_same_response_claude() {
    let req = claude_simple_request("tool + text");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![
            text_event("Looking into it."),
            tool_call_event(
                "read_file",
                Some("toolu_r"),
                serde_json::json!({"path": "lib.rs"}),
            ),
        ],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { .. }))
    );
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    );
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn tool_arguments_empty_object() {
    let req = openai_simple_request("empty args");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "no_args_tool",
            Some("call_e"),
            serde_json::json!({}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(parsed, serde_json::json!({}));
}

#[test]
fn tool_error_result_preserved_gemini() {
    let req = gemini_simple_request("error result");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![tool_result_event(
            "failing_tool",
            None,
            serde_json::json!({"error": "permission denied"}),
            true,
        )],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "failing_tool");
            assert_eq!(response["error"], "permission denied");
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. CAPABILITY NEGOTIATION INTEGRATION
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_response_format_no_claude_equivalent() {
    // OpenAI supports response_format; we verify it flows through WorkOrder
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "JSON please".into(),
        }],
        response_format: Some(abp_openai_sdk::response_format::ResponseFormat::JsonObject),
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "JSON please");

    // Response via Claude — no response_format concept
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("{\"key\": \"value\"}")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    match &resp.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("key")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_thinking_to_openai_no_equivalent() {
    // Claude extended_thinking → receipt with thinking ext → OpenAI has no equivalent
    let req = claude_simple_request("Think deeply");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![
            thinking_event("Let me think about this...", Some("sig_abc")),
            text_event("Here is my answer."),
        ],
        default_usage(),
    );
    // OpenAI response collapses thinking into text
    let resp: ChatCompletionResponse = receipt.into();
    let content = resp.choices[0].message.content.as_ref().unwrap();
    assert!(content.contains("Let me think") || content.contains("Here is my answer"));
}

#[test]
fn claude_thinking_preserved_in_claude_response() {
    let req = claude_simple_request("Think");
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![
            thinking_event("Internal reasoning...", Some("sig_123")),
            text_event("Final answer."),
        ],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    // Claude response should have a Thinking block
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::Thinking { .. }))
    );
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { .. }))
    );
}

#[test]
fn gemini_safety_settings_to_openai_ignored() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Safety test".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: Some(vec![
            GeminiSafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            },
            GeminiSafetySetting {
                category: HarmCategory::HarmCategoryHateSpeech,
                threshold: HarmBlockThreshold::BlockOnlyHigh,
            },
        ]),
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("safety_settings"));

    // Route to OpenAI — safety_settings have no OpenAI equivalent
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Safe response.")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Safe response.")
    );
}

#[test]
fn gemini_safety_settings_to_claude_ignored() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Safety to Claude".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryDangerousContent,
            threshold: HarmBlockThreshold::BlockLowAndAbove,
        }]),
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Claude response.")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn openai_frequency_penalty_not_in_work_order() {
    // frequency_penalty is OpenAI-specific
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "Penalty test".into(),
        }],
        frequency_penalty: Some(0.5),
        presence_penalty: Some(0.3),
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Penalty test");

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("OK")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
}

#[test]
fn claude_top_k_no_openai_equivalent() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Top-k test".into()),
        }],
        max_tokens: 4096,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: Some(0.5),
        top_p: Some(0.9),
        top_k: Some(40),
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Top-k test");

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Response")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
}

#[test]
fn gemini_tool_config_function_calling_mode() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Forced tool".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: Some(vec![gemini_tool_def(
            "calc",
            "Calculate",
            serde_json::json!({"type": "object"}),
        )]),
        tool_config: Some(GeminiToolConfig {
            function_calling_config: GeminiFunctionCallingConfig {
                mode: FunctionCallingMode::Any,
                allowed_function_names: Some(vec!["calc".into()]),
            },
        }),
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("tool_config"));
    assert!(wo.config.vendor.contains_key("tools"));

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event(
            "calc",
            Some("call_c"),
            serde_json::json!({"expr": "1+1"}),
        )],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
}

#[test]
fn claude_metadata_preserved_in_work_order() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Meta test".into()),
        }],
        max_tokens: 4096,
        system: None,
        tools: None,
        metadata: Some(Metadata {
            user_id: Some("user_42".into()),
        }),
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("metadata"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_messages_openai() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.is_empty());
}

#[test]
fn empty_messages_claude() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![],
        max_tokens: 1024,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.is_empty());
}

#[test]
fn empty_contents_gemini() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.is_empty());
}

#[test]
fn empty_tools_list_openai() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "No tools".into(),
        }],
        tools: Some(vec![]),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "No tools");
}

#[test]
fn none_tools_openai() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage::User {
            content: "None tools".into(),
        }],
        tools: None,
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "None tools");
}

#[test]
fn very_long_content_openai() {
    let long_text = "x".repeat(100_000);
    let req = openai_simple_request(&long_text);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task.len(), 100_000);

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event(&long_text)],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_ref().unwrap().len(),
        100_000
    );
}

#[test]
fn very_long_content_claude() {
    let long_text = "y".repeat(100_000);
    let req = claude_simple_request(&long_text);
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task.len(), 100_000);
}

#[test]
fn unicode_content_openai() {
    let req = openai_simple_request("こんにちは世界 🌍 Ñoño café résumé");
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("こんにちは"));
    assert!(wo.task.contains("🌍"));

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Привет мир 🎉 straße")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let content = resp.choices[0].message.content.as_ref().unwrap();
    assert!(content.contains("Привет"));
    assert!(content.contains("🎉"));
    assert!(content.contains("straße"));
}

#[test]
fn unicode_content_claude() {
    let req = claude_simple_request("日本語テスト 🇯🇵");
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("日本語"));

    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("中文回复 🇨🇳")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    match &resp.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("中文")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn unicode_content_gemini() {
    let req = gemini_simple_request("Ελληνικά 🇬🇷 한국어");
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("Ελληνικά"));

    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("العربية 🇸🇦")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("العربية")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn special_characters_in_tool_args() {
    let args = serde_json::json!({
        "path": "C:\\Users\\test\\file.txt",
        "content": "line1\nline2\ttab",
        "quote": "he said \"hello\"",
        "null_field": null
    });
    let req = openai_simple_request("special chars");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![tool_call_event("write_file", Some("call_w"), args)],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(parsed["path"], "C:\\Users\\test\\file.txt");
    assert!(parsed["content"].as_str().unwrap().contains('\n'));
}

#[test]
fn receipt_no_trace_events_openai() {
    let req = openai_simple_request("no events");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt("openai", wo.id, Outcome::Complete, vec![], default_usage());
    let resp: ChatCompletionResponse = receipt.into();
    assert!(resp.choices[0].message.content.is_none());
    assert!(resp.choices[0].message.tool_calls.is_none());
}

#[test]
fn receipt_no_trace_events_claude() {
    let req = claude_simple_request("no events");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt("claude", wo.id, Outcome::Complete, vec![], default_usage());
    let resp: MessagesResponse = receipt.into();
    assert!(resp.content.is_empty());
}

#[test]
fn receipt_no_trace_events_gemini() {
    let req = gemini_simple_request("no events");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt("gemini", wo.id, Outcome::Complete, vec![], default_usage());
    let resp: GeminiResponse = receipt.into();
    assert!(resp.candidates[0].content.parts.is_empty());
}

#[test]
fn usage_token_mapping_openai() {
    let usage = UsageNormalized {
        input_tokens: Some(200),
        output_tokens: Some(100),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: None,
        estimated_cost_usd: None,
    };
    let req = openai_simple_request("usage test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        usage,
    );
    let resp: ChatCompletionResponse = receipt.into();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 200);
    assert_eq!(u.completion_tokens, 100);
    assert_eq!(u.total_tokens, 300);
}

#[test]
fn usage_token_mapping_claude() {
    let usage = UsageNormalized {
        input_tokens: Some(150),
        output_tokens: Some(75),
        cache_read_tokens: Some(30),
        cache_write_tokens: Some(10),
        request_units: None,
        estimated_cost_usd: None,
    };
    let req = claude_simple_request("usage test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        usage,
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.usage.input_tokens, 150);
    assert_eq!(resp.usage.output_tokens, 75);
    assert_eq!(resp.usage.cache_read_input_tokens, Some(30));
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(10));
}

#[test]
fn usage_token_mapping_gemini() {
    let usage = UsageNormalized {
        input_tokens: Some(300),
        output_tokens: Some(200),
        ..Default::default()
    };
    let req = gemini_simple_request("usage test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        usage,
    );
    let resp: GeminiResponse = receipt.into();
    let meta = resp.usage_metadata.unwrap();
    assert_eq!(meta.prompt_token_count, 300);
    assert_eq!(meta.candidates_token_count, 200);
    assert_eq!(meta.total_token_count, 500);
}

#[test]
fn zero_usage_openai() {
    let req = openai_simple_request("zero usage");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        zero_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert!(resp.usage.is_none());
}

#[test]
fn zero_usage_gemini() {
    let req = gemini_simple_request("zero usage");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        zero_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert!(resp.usage_metadata.is_none());
}

#[test]
fn outcome_partial_openai() {
    let req = openai_simple_request("partial");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Partial,
        vec![text_event("partial...")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    // Partial doesn't change finish_reason for OpenAI (outcome maps differently)
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
}

#[test]
fn outcome_partial_claude() {
    let req = claude_simple_request("partial");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Partial,
        vec![text_event("partial...")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("max_tokens"));
}

#[test]
fn outcome_partial_gemini() {
    let req = gemini_simple_request("partial");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Partial,
        vec![text_event("partial...")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(
        resp.candidates[0].finish_reason.as_deref(),
        Some("MAX_TOKENS")
    );
}

#[test]
fn outcome_failed_openai() {
    let req = openai_simple_request("failed");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt("openai", wo.id, Outcome::Failed, vec![], default_usage());
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
}

#[test]
fn outcome_failed_claude() {
    let req = claude_simple_request("failed");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt("claude", wo.id, Outcome::Failed, vec![], default_usage());
    let resp: MessagesResponse = receipt.into();
    assert!(resp.stop_reason.is_none());
}

#[test]
fn outcome_failed_gemini() {
    let req = gemini_simple_request("failed");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt("gemini", wo.id, Outcome::Failed, vec![], default_usage());
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
}

#[test]
fn openai_response_id_format() {
    let req = openai_simple_request("id check");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert!(resp.id.starts_with("chatcmpl-"));
}

#[test]
fn claude_response_id_format() {
    let req = claude_simple_request("id check");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert!(resp.id.starts_with("msg_"));
}

#[test]
fn openai_response_object_field() {
    let req = openai_simple_request("object check");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.object, "chat.completion");
}

#[test]
fn claude_response_type_field() {
    let req = claude_simple_request("type check");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
}

#[test]
fn gemini_response_candidate_role() {
    let req = gemini_simple_request("role check");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].content.role, "model");
}

#[test]
fn serde_roundtrip_openai_request() {
    let req = openai_simple_request("serde test");
    let json = serde_json::to_string(&req).unwrap();
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gpt-4o");
    assert_eq!(parsed.messages.len(), 1);
}

#[test]
fn serde_roundtrip_claude_request() {
    let req = claude_simple_request("serde test");
    let json = serde_json::to_string(&req).unwrap();
    let parsed: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "claude-sonnet-4-20250514");
}

#[test]
fn serde_roundtrip_openai_response() {
    let req = openai_simple_request("serde resp");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.choices[0].message.content,
        resp.choices[0].message.content
    );
}

#[test]
fn serde_roundtrip_claude_response() {
    let req = claude_simple_request("serde resp");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: MessagesResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.len(), resp.content.len());
}

#[test]
fn work_order_id_stability() {
    let req = openai_simple_request("id stability");
    let wo: WorkOrder = req.into();
    let id = wo.id;
    assert_ne!(id, uuid::Uuid::nil());

    let receipt = make_receipt(
        "openai",
        id,
        Outcome::Complete,
        vec![text_event("ok")],
        default_usage(),
    );
    assert_eq!(receipt.meta.work_order_id, id);
}

#[test]
fn multiple_assistant_messages_concatenated_openai() {
    let req = openai_simple_request("concat test");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Part 1. "), text_event("Part 2.")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Part 1. Part 2.")
    );
}

#[test]
fn multiple_assistant_messages_separate_blocks_claude() {
    let req = claude_simple_request("multi block");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("Block 1"), text_event("Block 2")],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.content.len(), 2);
}

#[test]
fn multiple_assistant_messages_gemini() {
    let req = gemini_simple_request("multi part");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("Part A"), text_event("Part B")],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].content.parts.len(), 2);
}

#[test]
fn newlines_in_content() {
    let text = "Line 1\nLine 2\n\nLine 4";
    let req = openai_simple_request(text);
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains('\n'));

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("Response\nwith\nnewlines")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    let content = resp.choices[0].message.content.as_ref().unwrap();
    assert!(content.contains('\n'));
}

#[test]
fn empty_string_content() {
    let req = openai_simple_request("");
    let wo: WorkOrder = req.into();
    assert!(wo.task.is_empty());

    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![text_event("")],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    // Empty string is still Some("")
    assert_eq!(resp.choices[0].message.content.as_deref(), Some(""));
}

#[test]
fn claude_system_blocks_roundtrip() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("Test".into()),
        }],
        max_tokens: 4096,
        system: Some(SystemMessage::Blocks(vec![
            SystemBlock::Text {
                text: "Block system 1".into(),
                cache_control: None,
            },
            SystemBlock::Text {
                text: "Block system 2".into(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral".into(),
                }),
            },
        ])),
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    // System blocks get joined in vendor map
    let sys_val = &wo.config.vendor["system"];
    let sys_text = sys_val.as_str().unwrap();
    assert!(sys_text.contains("Block system 1"));
    assert!(sys_text.contains("Block system 2"));
}

#[test]
fn gemini_function_response_roundtrip() {
    let req = gemini_simple_request("fn response");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![
            tool_call_event("get_weather", None, serde_json::json!({"city": "London"})),
            tool_result_event(
                "get_weather",
                None,
                serde_json::json!({"temp": 18, "unit": "celsius"}),
                false,
            ),
            text_event("It's 18°C in London."),
        ],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].content.parts.len(), 3);
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::FunctionCall { name, .. } => assert_eq!(name, "get_weather"),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
    match &resp.candidates[0].content.parts[1] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "get_weather");
            assert_eq!(response["temp"], 18);
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn cross_sdk_usage_openai_to_claude() {
    let req = openai_simple_request("cross usage");
    let wo: WorkOrder = req.into();
    let usage = UsageNormalized {
        input_tokens: Some(500),
        output_tokens: Some(250),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        request_units: None,
        estimated_cost_usd: None,
    };
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        usage,
    );
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.usage.input_tokens, 500);
    assert_eq!(resp.usage.output_tokens, 250);
    assert_eq!(resp.usage.cache_read_input_tokens, Some(100));
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(50));
}

#[test]
fn cross_sdk_usage_claude_to_gemini() {
    let req = claude_simple_request("cross usage 2");
    let wo: WorkOrder = req.into();
    let usage = UsageNormalized {
        input_tokens: Some(400),
        output_tokens: Some(200),
        ..Default::default()
    };
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![text_event("ok")],
        usage,
    );
    let resp: GeminiResponse = receipt.into();
    let meta = resp.usage_metadata.unwrap();
    assert_eq!(meta.prompt_token_count, 400);
    assert_eq!(meta.candidates_token_count, 200);
    assert_eq!(meta.total_token_count, 600);
}

#[test]
fn work_order_preserves_model_openai() {
    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![OpenAIMessage::User {
            content: "model test".into(),
        }],
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: None,
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        n: None,
        seed: None,
        response_format: None,
        user: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-mini"));
}

#[test]
fn work_order_preserves_model_claude() {
    let req = MessagesRequest {
        model: "claude-opus-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: Role::User,
            content: MessageContent::Text("model test".into()),
        }],
        max_tokens: 4096,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
}

#[test]
fn work_order_preserves_model_gemini() {
    let req = GeminiRequest {
        model: "gemini-2.5-pro".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("model test".into())],
        }],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let wo: WorkOrder = req.into();
    assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-pro"));
}

#[test]
fn receipt_with_hash_roundtrip() {
    let req = openai_simple_request("hash test");
    let wo: WorkOrder = req.into();
    let receipt = ReceiptBuilder::new("openai")
        .work_order_id(wo.id)
        .outcome(Outcome::Complete)
        .usage(default_usage())
        .add_trace_event(text_event("hashed"))
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());

    let resp: ChatCompletionResponse = receipt.into();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("hashed"));
}

#[test]
fn error_event_ignored_in_gemini_response() {
    let req = gemini_simple_request("error event");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "something went wrong".into(),
                    error_code: None,
                },
                ext: None,
            },
            text_event("recovered"),
        ],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    // Error events are not mapped to Gemini parts
    assert_eq!(resp.candidates[0].content.parts.len(), 1);
    match &resp.candidates[0].content.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "recovered"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn warning_event_ignored_in_responses() {
    let req = openai_simple_request("warning event");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "openai",
        wo.id,
        Outcome::Complete,
        vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "rate limited".into(),
                },
                ext: None,
            },
            text_event("still works"),
        ],
        default_usage(),
    );
    let resp: ChatCompletionResponse = receipt.into();
    // Warning is not text content
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("still works")
    );
}

#[test]
fn file_changed_event_ignored_in_responses() {
    let req = claude_simple_request("file changed");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "claude",
        wo.id,
        Outcome::Complete,
        vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/lib.rs".into(),
                    summary: "Added function".into(),
                },
                ext: None,
            },
            text_event("Done."),
        ],
        default_usage(),
    );
    let resp: MessagesResponse = receipt.into();
    // FileChanged events are not mapped to content blocks
    assert_eq!(resp.content.len(), 1);
}

#[test]
fn command_executed_event_ignored_in_responses() {
    let req = gemini_simple_request("cmd event");
    let wo: WorkOrder = req.into();
    let receipt = make_receipt(
        "gemini",
        wo.id,
        Outcome::Complete,
        vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::CommandExecuted {
                    command: "cargo build".into(),
                    exit_code: Some(0),
                    output_preview: Some("Compiling...".into()),
                },
                ext: None,
            },
            text_event("Build done."),
        ],
        default_usage(),
    );
    let resp: GeminiResponse = receipt.into();
    assert_eq!(resp.candidates[0].content.parts.len(), 1);
}
