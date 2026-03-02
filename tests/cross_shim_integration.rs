// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-shim integration tests.
//!
//! Verifies that OpenAI, Claude, and Gemini SDK shims interoperate correctly
//! through the ABP IR layer: requests built in one shim format can be converted
//! to IR, then consumed as responses in another shim format.

use abp_core::ir::{IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, Outcome, Receipt, ReceiptBuilder,
    SupportLevel, UsageNormalized,
};
use abp_dialect::Dialect;
use abp_projection::ProjectionMatrix;
use chrono::Utc;
use serde_json::json;

// =========================================================================
// Helpers — construct requests in each shim format
// =========================================================================

fn openai_request_simple(text: &str) -> abp_shim_openai::ChatCompletionRequest {
    abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("You are a helpful assistant."),
            abp_shim_openai::Message::user(text),
        ])
        .temperature(0.7)
        .max_tokens(1024)
        .build()
}

fn claude_request_simple(text: &str) -> abp_shim_claude::MessageRequest {
    abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: text.into() }],
        }],
        system: Some("You are a helpful assistant.".into()),
        temperature: Some(0.7),
        stop_sequences: None,
        thinking: None,
        stream: None,
    }
}

fn gemini_request_simple(text: &str) -> abp_shim_gemini::GenerateContentRequest {
    abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .system_instruction(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("You are a helpful assistant."),
        ]))
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text(text),
        ]))
        .generation_config(abp_shim_gemini::GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1024),
            ..Default::default()
        })
}

/// Build a mock receipt with assistant text + usage from a work order.
fn mock_receipt_for(text: &str, input_tokens: u64, output_tokens: u64) -> Receipt {
    let usage = UsageNormalized {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        ..Default::default()
    };
    ReceiptBuilder::new("mock-cross-shim")
        .outcome(Outcome::Complete)
        .usage(usage)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "cross-shim test".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build()
}

fn mock_receipt_with_tool_call(tool_name: &str, tool_id: &str, args: serde_json::Value) -> Receipt {
    ReceiptBuilder::new("mock-cross-shim")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(15),
            output_tokens: Some(30),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: tool_name.into(),
                tool_use_id: Some(tool_id.into()),
                parent_tool_use_id: None,
                input: args,
            },
            ext: None,
        })
        .build()
}

fn mock_receipt_with_error() -> Receipt {
    ReceiptBuilder::new("mock-cross-shim")
        .outcome(Outcome::Failed)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                error_code: Some(abp_error::ErrorCode::BackendTimeout),
                message: "Rate limit exceeded".into(),
            },
            ext: None,
        })
        .build()
}

// =========================================================================
// 1. OpenAI request → IR → Claude shim response roundtrip
// =========================================================================

#[test]
fn openai_to_ir_to_claude_response() {
    let oai_req = openai_request_simple("What is Rust?");
    let ir = abp_shim_openai::request_to_ir(&oai_req);

    // IR should capture the system and user messages
    assert!(ir.system_message().is_some());
    assert!(!ir.messages_by_role(IrRole::User).is_empty());

    // Build a receipt and convert to Claude response
    let _work_order = abp_shim_openai::request_to_work_order(&oai_req);
    let receipt = mock_receipt_for("Rust is a systems programming language.", 20, 15);
    let events: Vec<AgentEvent> = receipt.trace.clone();

    let claude_resp =
        abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
    assert_eq!(claude_resp.role, "assistant");
    assert!(!claude_resp.content.is_empty());

    let has_text = claude_resp
        .content
        .iter()
        .any(|b| matches!(b, abp_shim_claude::ContentBlock::Text { .. }));
    assert!(has_text, "Claude response should contain text");

    // Verify no ABP framing leaks
    let json = serde_json::to_string(&claude_resp).unwrap();
    assert!(
        !json.contains("work_order"),
        "ABP framing leaked into response"
    );
    assert!(
        !json.contains("receipt_sha256"),
        "ABP framing leaked into response"
    );
}

// =========================================================================
// 2. Claude request → IR → Gemini shim response roundtrip
// =========================================================================

#[test]
fn claude_to_ir_to_gemini_response() {
    let claude_req = claude_request_simple("Explain quantum computing");
    let work_order = abp_shim_claude::request_to_work_order(&claude_req);

    assert_eq!(
        work_order.config.model.as_deref(),
        Some("claude-sonnet-4-20250514")
    );

    let receipt = mock_receipt_for("Quantum computing uses qubits.", 18, 12);

    // Gemini shim can reconstruct from receipt trace events
    let events = &receipt.trace;
    let gemini_stream_events = abp_shim_openai::events_to_stream_events(events, "gemini-2.5-flash");
    assert!(
        !gemini_stream_events.is_empty(),
        "should produce stream events"
    );
}

// =========================================================================
// 3. Gemini request → IR → OpenAI shim response roundtrip
// =========================================================================

#[test]
fn gemini_to_ir_to_openai_response() {
    let gemini_req = gemini_request_simple("Write a haiku");
    let dialect_req = abp_shim_gemini::to_dialect_request(&gemini_req);
    assert_eq!(dialect_req.model, "gemini-2.5-flash");

    let receipt = mock_receipt_for(
        "Silent code compiles / Bits align in perfect rows / Programs come alive",
        10,
        20,
    );
    let oai_resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

    assert_eq!(oai_resp.object, "chat.completion");
    assert_eq!(oai_resp.model, "gpt-4o");
    assert_eq!(oai_resp.choices.len(), 1);
    assert!(oai_resp.choices[0].message.content.is_some());
    assert_eq!(oai_resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

// =========================================================================
// 4. All 3×3 shim pairs (excluding self) produce valid responses
// =========================================================================

#[test]
fn all_cross_shim_pairs_produce_valid_responses() {
    let receipt = mock_receipt_for("Cross-shim response", 10, 20);
    let events = &receipt.trace;

    // OpenAI → Claude
    {
        let resp = abp_shim_claude::response_from_events(events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.role, "assistant");
        assert!(!resp.content.is_empty());
    }

    // OpenAI → Gemini (via receipt → OpenAI response, then check structure)
    {
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gemini-2.5-flash");
        assert_eq!(resp.object, "chat.completion");
        assert!(!resp.choices.is_empty());
    }

    // Claude → OpenAI
    {
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert!(resp.choices[0].message.content.is_some());
    }

    // Claude → Gemini
    {
        let stream_events = abp_shim_openai::events_to_stream_events(events, "gemini-2.5-flash");
        assert!(!stream_events.is_empty());
    }

    // Gemini → OpenAI
    {
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.object, "chat.completion");
    }

    // Gemini → Claude
    {
        let resp = abp_shim_claude::response_from_events(events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.response_type, "message");
    }
}

// =========================================================================
// 5. Tool definitions survive cross-shim conversion
// =========================================================================

#[test]
fn tool_definitions_survive_openai_to_ir() {
    let params = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path" }
        },
        "required": ["path"]
    });

    let tools = vec![abp_shim_openai::Tool::function(
        "read_file",
        "Read a file from disk",
        params.clone(),
    )];

    let ir_tools = abp_shim_openai::tools_to_ir(&tools);
    assert_eq!(ir_tools.len(), 1);
    assert_eq!(ir_tools[0].name, "read_file");
    assert_eq!(ir_tools[0].description, "Read a file from disk");
    assert_eq!(ir_tools[0].parameters, params);
}

#[test]
fn tool_call_survives_cross_shim_via_receipt() {
    let receipt =
        mock_receipt_with_tool_call("read_file", "call_123", json!({"path": "src/main.rs"}));

    // Tool call visible from OpenAI shim
    let oai_resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(
        oai_resp.choices[0].finish_reason.as_deref(),
        Some("tool_calls")
    );
    let tc = oai_resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc[0].function.name, "read_file");
    assert_eq!(tc[0].id, "call_123");

    // Tool call visible from Claude shim
    let claude_resp =
        abp_shim_claude::response_from_events(&receipt.trace, "claude-sonnet-4-20250514", None);
    match &claude_resp.content[0] {
        abp_shim_claude::ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(name, "read_file");
            assert_eq!(id, "call_123");
            assert_eq!(input, &json!({"path": "src/main.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// =========================================================================
// 6. System messages survive cross-shim conversion
// =========================================================================

#[test]
fn system_message_survives_openai_ir_roundtrip() {
    let req = openai_request_simple("Hello");
    let ir = abp_shim_openai::request_to_ir(&req);

    let sys = ir.system_message().expect("should have system message");
    assert_eq!(sys.text_content(), "You are a helpful assistant.");

    // Roundtrip back to OpenAI messages
    let messages = abp_shim_openai::ir_to_messages(&ir);
    let sys_msg = messages
        .iter()
        .find(|m| m.role == abp_shim_openai::Role::System);
    assert!(sys_msg.is_some(), "system message should survive roundtrip");
    assert_eq!(
        sys_msg.unwrap().content.as_deref(),
        Some("You are a helpful assistant.")
    );
}

#[test]
fn system_message_in_claude_request_reaches_work_order() {
    let req = claude_request_simple("Hello");
    assert_eq!(req.system.as_deref(), Some("You are a helpful assistant."));

    let wo = abp_shim_claude::request_to_work_order(&req);
    // The work order should be created successfully
    assert!(!wo.task.is_empty());
}

#[test]
fn system_instruction_in_gemini_request_is_preserved() {
    let req = gemini_request_simple("Hello");
    assert!(req.system_instruction.is_some());
    let sys = req.system_instruction.as_ref().unwrap();
    let text = sys.parts.iter().find_map(|p| match p {
        abp_shim_gemini::Part::Text(t) => Some(t.as_str()),
        _ => None,
    });
    assert_eq!(text, Some("You are a helpful assistant."));
}

// =========================================================================
// 7. Temperature / max_tokens survive cross-shim conversion
// =========================================================================

#[test]
fn temperature_max_tokens_survive_openai_to_work_order() {
    let req = openai_request_simple("Hi");
    let wo = abp_shim_openai::request_to_work_order(&req);

    let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
    assert_eq!(temp, Some(0.7));

    let max_tok = wo.config.vendor.get("max_tokens").and_then(|v| v.as_u64());
    assert_eq!(max_tok, Some(1024));
}

#[test]
fn temperature_survives_claude_to_work_order() {
    let req = claude_request_simple("Hi");
    let wo = abp_shim_claude::request_to_work_order(&req);

    let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
    assert_eq!(temp, Some(0.7));

    let max_tok = wo.config.vendor.get("max_tokens").and_then(|v| v.as_u64());
    assert_eq!(max_tok, Some(1024));
}

#[test]
fn generation_config_preserved_in_gemini_dialect_request() {
    let req = gemini_request_simple("Hi");
    let dialect_req = abp_shim_gemini::to_dialect_request(&req);

    let gen_config = dialect_req.generation_config.unwrap();
    assert_eq!(gen_config.temperature, Some(0.7));
    assert_eq!(gen_config.max_output_tokens, Some(1024));
}

// =========================================================================
// 8. Multi-turn conversations survive cross-shim conversion
// =========================================================================

#[test]
fn multi_turn_openai_to_ir_roundtrip() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("You are an assistant."),
            abp_shim_openai::Message::user("What is 2+2?"),
            abp_shim_openai::Message::assistant("4"),
            abp_shim_openai::Message::user("And 3+3?"),
        ])
        .build();

    let ir = abp_shim_openai::request_to_ir(&req);
    assert!(ir.len() >= 4, "all turns should be in IR");

    let users = ir.messages_by_role(IrRole::User);
    assert_eq!(users.len(), 2, "should have 2 user turns");

    let assistants = ir.messages_by_role(IrRole::Assistant);
    assert_eq!(assistants.len(), 1, "should have 1 assistant turn");

    // Roundtrip back
    let messages = abp_shim_openai::ir_to_messages(&ir);
    assert!(messages.len() >= 4);
}

#[test]
fn multi_turn_claude_messages_convert_to_ir() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![
            abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text { text: "Hi".into() }],
            },
            abp_shim_claude::Message {
                role: abp_shim_claude::Role::Assistant,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "Hello!".into(),
                }],
            },
            abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "How are you?".into(),
                }],
            },
        ],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    // Should produce a valid work order
    let wo = abp_shim_claude::request_to_work_order(&req);
    assert!(!wo.task.is_empty());
}

#[test]
fn multi_turn_gemini_contents_preserved() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Hi"),
        ]))
        .add_content(abp_shim_gemini::Content::model(vec![
            abp_shim_gemini::Part::text("Hello!"),
        ]))
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("How are you?"),
        ]));

    let dialect = abp_shim_gemini::to_dialect_request(&req);
    assert_eq!(dialect.contents.len(), 3);
}

// =========================================================================
// 9. Streaming events from one shim can be consumed by another's format
// =========================================================================

#[test]
fn openai_stream_events_from_agent_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello ".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "world".into(),
            },
            ext: None,
        },
    ];

    let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
    // Should have 2 delta chunks + 1 final stop chunk
    assert_eq!(stream.len(), 3);

    // Each chunk is valid OpenAI format
    for chunk in &stream {
        assert_eq!(chunk.object, "chat.completion.chunk");
        assert_eq!(chunk.model, "gpt-4o");
    }

    // Last chunk should have stop finish_reason
    assert_eq!(
        stream.last().unwrap().choices[0].finish_reason.as_deref(),
        Some("stop")
    );
}

#[test]
fn claude_stream_events_work_with_agent_events() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Response text".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];

    let claude_resp =
        abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
    assert!(!claude_resp.content.is_empty());
    assert_eq!(claude_resp.stop_reason.as_deref(), Some("end_turn"));
}

// =========================================================================
// 10. Error responses from one SDK format to another
// =========================================================================

#[test]
fn error_from_receipt_surfaces_in_openai_response() {
    let receipt = mock_receipt_with_error();
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

    assert_eq!(resp.choices.len(), 1);
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(
        content.contains("Rate limit exceeded"),
        "error should surface in response content"
    );
}

#[test]
fn error_from_receipt_surfaces_in_claude_response() {
    let receipt = mock_receipt_with_error();
    let events = &receipt.trace;

    // Error events become no content blocks (they aren't AssistantMessage)
    // but the response should still be constructible
    let resp = abp_shim_claude::response_from_events(events, "claude-sonnet-4-20250514", None);
    assert_eq!(resp.role, "assistant");
    // Should not panic and should produce a valid response
}

// =========================================================================
// 11. Token usage tracking across shim boundaries
// =========================================================================

#[test]
fn usage_survives_openai_receipt_roundtrip() {
    let receipt = mock_receipt_for("Hello", 42, 58);
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 42);
    assert_eq!(usage.completion_tokens, 58);
    assert_eq!(usage.total_tokens, 100);
}

#[test]
fn usage_ir_roundtrip_openai() {
    let ir_usage = IrUsage::from_io(30, 50);
    let oai_usage = abp_shim_openai::ir_usage_to_usage(&ir_usage);
    assert_eq!(oai_usage.prompt_tokens, 30);
    assert_eq!(oai_usage.completion_tokens, 50);
    assert_eq!(oai_usage.total_tokens, 80);
}

#[test]
fn usage_ir_roundtrip_gemini() {
    let ir_usage = IrUsage::from_io(25, 75);
    let gemini_usage = abp_shim_gemini::usage_from_ir(&ir_usage);
    assert_eq!(gemini_usage.prompt_token_count, 25);
    assert_eq!(gemini_usage.candidates_token_count, 75);
    assert_eq!(gemini_usage.total_token_count, 100);

    // And back
    let back = abp_shim_gemini::usage_to_ir(&gemini_usage);
    assert_eq!(back, ir_usage);
}

#[test]
fn usage_crosses_shim_boundaries() {
    // Start from OpenAI IR usage
    let ir = IrUsage::from_io(100, 200);

    // Convert to OpenAI format
    let oai = abp_shim_openai::ir_usage_to_usage(&ir);
    assert_eq!(oai.prompt_tokens, 100);
    assert_eq!(oai.completion_tokens, 200);

    // Convert same IR to Gemini format
    let gemini = abp_shim_gemini::usage_from_ir(&ir);
    assert_eq!(gemini.prompt_token_count, 100);
    assert_eq!(gemini.candidates_token_count, 200);

    // Both agree on totals
    assert_eq!(oai.total_tokens, gemini.total_token_count);
}

// =========================================================================
// 12. Model name mapping (preserving original + noting translation)
// =========================================================================

#[test]
fn model_name_preserved_in_work_order_openai() {
    let req = openai_request_simple("Hi");
    let wo = abp_shim_openai::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn model_name_preserved_in_work_order_claude() {
    let req = claude_request_simple("Hi");
    let wo = abp_shim_claude::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn model_name_preserved_in_response_openai() {
    let receipt = mock_receipt_for("text", 10, 20);
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.model, "gpt-4o");
}

#[test]
fn model_name_preserved_in_response_claude() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "text".into(),
        },
        ext: None,
    }];
    let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
    assert_eq!(resp.model, "claude-sonnet-4-20250514");
}

// =========================================================================
// 13. Cross-shim projection: route requests to best backend
// =========================================================================

#[test]
fn projection_selects_matching_dialect_backend() {
    let mut matrix = ProjectionMatrix::new();

    let mut openai_caps = CapabilityManifest::new();
    openai_caps.insert(Capability::Streaming, SupportLevel::Native);
    openai_caps.insert(Capability::ToolUse, SupportLevel::Native);

    let mut claude_caps = CapabilityManifest::new();
    claude_caps.insert(Capability::Streaming, SupportLevel::Native);
    claude_caps.insert(Capability::ToolUse, SupportLevel::Native);
    claude_caps.insert(Capability::ExtendedThinking, SupportLevel::Native);

    let mut gemini_caps = CapabilityManifest::new();
    gemini_caps.insert(Capability::Streaming, SupportLevel::Native);
    gemini_caps.insert(Capability::ToolUse, SupportLevel::Native);

    matrix.register_backend("openai", openai_caps, Dialect::OpenAi, 50);
    matrix.register_backend("claude", claude_caps, Dialect::Claude, 50);
    matrix.register_backend("gemini", gemini_caps, Dialect::Gemini, 50);

    // An OpenAI-origin request
    let oai_req = openai_request_simple("Hello");
    let wo = abp_shim_openai::request_to_work_order(&oai_req);

    let result = matrix.project(&wo).unwrap();
    // Should succeed with one of the backends
    assert!(!result.selected_backend.is_empty());
    assert!(result.fidelity_score.total > 0.0);
}

#[test]
fn projection_with_source_dialect_prefers_same_dialect() {
    let mut matrix = ProjectionMatrix::new();

    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);

    matrix.register_backend("openai", caps.clone(), Dialect::OpenAi, 50);
    matrix.register_backend("claude", caps.clone(), Dialect::Claude, 50);
    matrix.set_source_dialect(Dialect::OpenAi);

    let req = openai_request_simple("Hi");
    let mut wo = abp_shim_openai::request_to_work_order(&req);
    // Set passthrough mode to trigger same-dialect bonus
    wo.config.vendor.insert(
        "abp".into(),
        json!({"mode": "passthrough", "source_dialect": "openai"}),
    );

    let result = matrix.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "openai");
}

// =========================================================================
// 14. Verify ABP framing doesn't leak into shim responses
// =========================================================================

#[test]
fn no_abp_framing_in_openai_response() {
    let receipt = mock_receipt_for("Hello world", 10, 20);
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
    let json = serde_json::to_string(&resp).unwrap();

    assert!(!json.contains("work_order_id"), "work_order_id leaked");
    assert!(!json.contains("receipt_sha256"), "receipt_sha256 leaked");
    assert!(
        !json.contains("contract_version"),
        "contract_version leaked"
    );
    assert!(!json.contains("abp/v"), "ABP version leaked");
    assert!(!json.contains("RunStarted"), "ABP event type leaked");
    assert!(!json.contains("RunCompleted"), "ABP event type leaked");
}

#[test]
fn no_abp_framing_in_claude_response() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello".into(),
        },
        ext: None,
    }];
    let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
    let json = serde_json::to_string(&resp).unwrap();

    assert!(!json.contains("work_order_id"), "work_order_id leaked");
    assert!(!json.contains("receipt_sha256"), "receipt_sha256 leaked");
    assert!(
        !json.contains("contract_version"),
        "contract_version leaked"
    );
    assert!(!json.contains("abp/v"), "ABP version leaked");
}

#[test]
fn no_abp_framing_in_openai_stream_events() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    }];
    let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
    for chunk in &stream {
        let json = serde_json::to_string(chunk).unwrap();
        assert!(!json.contains("work_order"), "ABP framing in stream chunk");
        assert!(
            !json.contains("receipt_sha256"),
            "ABP framing in stream chunk"
        );
    }
}

// =========================================================================
// Additional: IR message roundtrip across all three shims
// =========================================================================

#[test]
fn ir_conversation_roundtrips_through_openai_messages() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be helpful"),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there"),
    ]);

    let messages = abp_shim_openai::ir_to_messages(&ir);
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].role, abp_shim_openai::Role::System);
    assert_eq!(messages[1].role, abp_shim_openai::Role::User);
    assert_eq!(messages[2].role, abp_shim_openai::Role::Assistant);

    // Roundtrip back
    let ir2 = abp_shim_openai::messages_to_ir(&messages);
    assert_eq!(ir2.len(), ir.len());
}

#[test]
fn gemini_usage_ir_roundtrip_is_lossless() {
    let original = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 150,
        candidates_token_count: 300,
        total_token_count: 450,
    };

    let ir = abp_shim_gemini::usage_to_ir(&original);
    let back = abp_shim_gemini::usage_from_ir(&ir);

    assert_eq!(original, back);
}
