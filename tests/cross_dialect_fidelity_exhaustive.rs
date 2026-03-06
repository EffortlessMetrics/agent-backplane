#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exhaustive cross-dialect fidelity tests covering all 36 dialect×dialect pairs.
//!
//! Tests verify message roundtrip fidelity, tool call/result mapping, system
//! prompt handling, streaming event mapping, error code mapping, identity
//! losslessness, semantic preservation, fidelity scoring, and early failure
//! for unsupported pairs.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_error::ErrorCode;
use abp_mapper::validation::{DefaultMappingValidator, MappingValidator, RoundtripResult};
use abp_mapper::{IrMapper, MapError, MappingError, default_ir_mapper, supported_ir_pairs};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

// ── Dialect helpers ────────────────────────────────────────────────────

const ALL_DIALECTS: &[Dialect] = &[
    Dialect::OpenAi,
    Dialect::Claude,
    Dialect::Gemini,
    Dialect::Codex,
    Dialect::Kimi,
    Dialect::Copilot,
];

fn dialect_label(d: Dialect) -> &'static str {
    match d {
        Dialect::OpenAi => "OpenAi",
        Dialect::Claude => "Claude",
        Dialect::Gemini => "Gemini",
        Dialect::Codex => "Codex",
        Dialect::Kimi => "Kimi",
        Dialect::Copilot => "Copilot",
    }
}

// ── Unsupported pairs (all 36 minus 24 supported = 12 unsupported) ───

fn unsupported_pairs() -> Vec<(Dialect, Dialect)> {
    let supported = supported_ir_pairs();
    let mut out = Vec::new();
    for &from in ALL_DIALECTS {
        for &to in ALL_DIALECTS {
            if !supported.contains(&(from, to)) {
                out.push((from, to));
            }
        }
    }
    out
}

// ── Conversation fixtures ──────────────────────────────────────────────

fn simple_text_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello, how are you?"),
        IrMessage::text(IrRole::Assistant, "I'm doing well, thanks!"),
    ])
}

fn tool_call_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather in Paris?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check the weather.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "Paris", "units": "celsius"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_abc".into(),
                content: vec![IrContentBlock::Text {
                    text: "22°C, partly cloudy".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 22°C and partly cloudy in Paris."),
    ])
}

fn tool_error_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Run a command"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_err".into(),
                name: "bash".into(),
                input: json!({"cmd": "invalid_command"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_err".into(),
                content: vec![IrContentBlock::Text {
                    text: "command not found".into(),
                }],
                is_error: true,
            }],
        ),
    ])
}

fn thinking_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve this complex problem"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me analyze step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ),
    ])
}

fn image_conversation() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is in this image?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgoAAAANSUhEUg==".into(),
            },
        ],
    )])
}

fn multi_tool_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Search and read files"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"query": "rust async"}),
                },
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/main.rs"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "Found 5 results".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main() {}".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

fn system_only_conversation() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(
        IrRole::System,
        "You are a code reviewer.",
    )])
}

fn empty_conversation() -> IrConversation {
    IrConversation::new()
}

fn metadata_conversation() -> IrConversation {
    let mut msg = IrMessage::text(IrRole::User, "hello with metadata");
    msg.metadata.insert("source".into(), json!("test_harness"));
    msg.metadata.insert("priority".into(), json!(1));
    IrConversation::from_messages(vec![msg])
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 1: Factory & supported pairs verification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn supported_ir_pairs_returns_24() {
    let pairs = supported_ir_pairs();
    assert_eq!(
        pairs.len(),
        24,
        "expected 24 supported pairs, got {}",
        pairs.len()
    );
}

#[test]
fn all_36_pairs_enumerated() {
    let mut count = 0;
    for &from in ALL_DIALECTS {
        for &to in ALL_DIALECTS {
            count += 1;
            // Every pair either has a mapper or is unsupported
            let _has_mapper = default_ir_mapper(from, to).is_some();
        }
    }
    assert_eq!(count, 36);
}

#[test]
fn supported_pairs_all_have_mappers() {
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to);
        assert!(
            mapper.is_some(),
            "supported pair ({}, {}) has no mapper",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn unsupported_pairs_have_no_mapper() {
    for (from, to) in unsupported_pairs() {
        let mapper = default_ir_mapper(from, to);
        assert!(
            mapper.is_none(),
            "unsupported pair ({}, {}) unexpectedly has a mapper",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn exactly_12_unsupported_pairs() {
    let unsup = unsupported_pairs();
    assert_eq!(
        unsup.len(),
        12,
        "expected 12 unsupported pairs, got {}",
        unsup.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 2: Identity mapping losslessness (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn identity_simple_text_all_dialects() {
    let conv = simple_text_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(result, conv, "identity failed for {}", dialect_label(d));
    }
}

#[test]
fn identity_tool_call_all_dialects() {
    let conv = tool_call_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(
            result,
            conv,
            "identity tool call failed for {}",
            dialect_label(d)
        );
    }
}

#[test]
fn identity_thinking_all_dialects() {
    let conv = thinking_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(
            result,
            conv,
            "identity thinking failed for {}",
            dialect_label(d)
        );
    }
}

#[test]
fn identity_empty_all_dialects() {
    let conv = empty_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert!(
            result.is_empty(),
            "identity empty failed for {}",
            dialect_label(d)
        );
    }
}

#[test]
fn identity_response_all_dialects() {
    let conv = simple_text_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_response(d, d, &conv).unwrap();
        assert_eq!(
            result,
            conv,
            "identity response failed for {}",
            dialect_label(d)
        );
    }
}

#[test]
fn identity_preserves_metadata_all_dialects() {
    let conv = metadata_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(
            result.messages[0].metadata,
            conv.messages[0].metadata,
            "identity metadata failed for {}",
            dialect_label(d)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 3: Message roundtrip fidelity for all 24 supported pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn message_roundtrip_text_preserved_all_supported_pairs() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        // Text content from user messages should always survive
        let user_texts: Vec<String> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::User)
            .map(|m| m.text_content())
            .collect();
        assert!(
            !user_texts.is_empty() || from == Dialect::Codex || to == Dialect::Codex,
            "user text lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn message_roundtrip_assistant_text_preserved_all_supported_pairs() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let asst_texts: Vec<String> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Assistant)
            .map(|m| m.text_content())
            .collect();
        // Codex drops everything except text in user/assistant
        if to != Dialect::Codex {
            assert!(
                !asst_texts.is_empty(),
                "assistant text lost for {} -> {}",
                dialect_label(from),
                dialect_label(to)
            );
        }
    }
}

#[test]
fn roundtrip_fidelity_simple_text_all_bidirectional() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        if from == to {
            continue;
        }
        // Skip codex since it's lossy
        if from == Dialect::Codex || to == Dialect::Codex {
            continue;
        }
        let forward = default_ir_mapper(from, to).unwrap();
        let backward_mapper = default_ir_mapper(to, from);
        if backward_mapper.is_none() {
            continue;
        }
        let backward = backward_mapper.unwrap();

        let mapped = forward.map_request(from, to, &conv).unwrap();
        let roundtripped = backward.map_request(to, from, &mapped).unwrap();

        // Text content should survive the roundtrip
        let orig_user_text = conv
            .messages
            .iter()
            .filter(|m| m.role == IrRole::User)
            .map(|m| m.text_content())
            .collect::<Vec<_>>();
        let rt_user_text = roundtripped
            .messages
            .iter()
            .filter(|m| m.role == IrRole::User)
            .map(|m| m.text_content())
            .collect::<Vec<_>>();
        assert_eq!(
            orig_user_text,
            rt_user_text,
            "roundtrip user text mismatch for {} -> {} -> {}",
            dialect_label(from),
            dialect_label(to),
            dialect_label(from)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 4: Tool call/result mapping for all supported pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tool_calls_preserved_non_codex_pairs() {
    let conv = tool_call_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue; // Codex drops tool calls
        }
        if from == to {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let tool_uses = result.tool_calls();
        assert!(
            !tool_uses.is_empty(),
            "tool calls lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn tool_call_name_preserved_across_dialects() {
    let conv = tool_call_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let tool_names: Vec<&str> = result
            .tool_calls()
            .iter()
            .filter_map(|b| match b {
                IrContentBlock::ToolUse { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            tool_names.contains(&"get_weather"),
            "tool name lost for {} -> {}: got {:?}",
            dialect_label(from),
            dialect_label(to),
            tool_names
        );
    }
}

#[test]
fn tool_call_input_preserved_across_dialects() {
    let conv = tool_call_conversation();
    let expected_input = json!({"city": "Paris", "units": "celsius"});
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let found = result.tool_calls().iter().any(|b| match b {
            IrContentBlock::ToolUse { input, .. } => *input == expected_input,
            _ => false,
        });
        assert!(
            found,
            "tool input lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn tool_result_preserved_non_codex_pairs() {
    let conv = tool_call_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_tool_result = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        });
        assert!(
            has_tool_result,
            "tool result lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn tool_error_flag_preserved_non_codex_pairs() {
    let conv = tool_error_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_error_result = result.messages.iter().any(|m| {
            m.content.iter().any(|b| match b {
                IrContentBlock::ToolResult { is_error, .. } => *is_error,
                _ => false,
            })
        });
        assert!(
            has_error_result,
            "tool error flag lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn multi_tool_results_preserved_non_codex() {
    let conv = multi_tool_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let tool_result_count: usize = result
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            .count();
        assert!(
            tool_result_count >= 2,
            "multi tool results lost for {} -> {}, got {}",
            dialect_label(from),
            dialect_label(to),
            tool_result_count
        );
    }
}

#[test]
fn codex_target_drops_tool_calls() {
    let conv = tool_call_conversation();
    for &from in ALL_DIALECTS {
        if from == Dialect::Codex {
            continue; // Identity mapper is passthrough
        }
        if default_ir_mapper(from, Dialect::Codex).is_none() {
            continue;
        }
        let mapper = default_ir_mapper(from, Dialect::Codex).unwrap();
        let result = mapper.map_request(from, Dialect::Codex, &conv).unwrap();
        let tool_uses = result.tool_calls();
        assert!(
            tool_uses.is_empty(),
            "Codex target should drop tool calls from {}",
            dialect_label(from)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 5: System prompt handling for all supported pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn system_prompt_preserved_non_codex() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue; // Codex drops system
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let sys = result.system_message();
        assert!(
            sys.is_some(),
            "system prompt lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn system_prompt_text_content_preserved_non_codex() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let sys_text = result.system_message().map(|m| m.text_content());
        assert_eq!(
            sys_text.as_deref(),
            Some("You are a helpful assistant."),
            "system text mismatch for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn codex_target_drops_system_messages() {
    let conv = simple_text_conversation();
    for &from in ALL_DIALECTS {
        if from == Dialect::Codex {
            continue; // Identity mapper is passthrough
        }
        if default_ir_mapper(from, Dialect::Codex).is_none() {
            continue;
        }
        let mapper = default_ir_mapper(from, Dialect::Codex).unwrap();
        let result = mapper.map_request(from, Dialect::Codex, &conv).unwrap();
        let sys = result.system_message();
        assert!(
            sys.is_none(),
            "Codex should drop system messages from {}",
            dialect_label(from)
        );
    }
}

#[test]
fn system_only_conversation_mapped_all_supported() {
    let conv = system_only_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv);
        // Should not error, even if result is empty for Codex
        assert!(
            result.is_ok(),
            "system-only conversation failed for {} -> {}: {:?}",
            dialect_label(from),
            dialect_label(to),
            result.err()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 6: Streaming event mapping (AgentEvent serde fidelity)
// ═══════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[test]
fn streaming_event_assistant_delta_serializes() {
    let event = make_event(AgentEventKind::AssistantDelta {
        text: "Hello".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "assistant_delta");
    assert_eq!(json["text"], "Hello");
}

#[test]
fn streaming_event_assistant_message_serializes() {
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "Complete response".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "assistant_message");
    assert_eq!(json["text"], "Complete response");
}

#[test]
fn streaming_event_tool_call_serializes() {
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: "get_weather".into(),
        tool_use_id: Some("call_1".into()),
        parent_tool_use_id: None,
        input: json!({"city": "NYC"}),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_call");
    assert_eq!(json["tool_name"], "get_weather");
    assert_eq!(json["tool_use_id"], "call_1");
    assert_eq!(json["input"]["city"], "NYC");
}

#[test]
fn streaming_event_tool_result_serializes() {
    let event = make_event(AgentEventKind::ToolResult {
        tool_name: "get_weather".into(),
        tool_use_id: Some("call_1".into()),
        output: json!("72°F"),
        is_error: false,
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["is_error"], false);
}

#[test]
fn streaming_event_tool_result_error_serializes() {
    let event = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: Some("call_err".into()),
        output: json!("permission denied"),
        is_error: true,
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["is_error"], true);
}

#[test]
fn streaming_event_run_started_serializes() {
    let event = make_event(AgentEventKind::RunStarted {
        message: "Starting run".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run_started");
}

#[test]
fn streaming_event_run_completed_serializes() {
    let event = make_event(AgentEventKind::RunCompleted {
        message: "Done".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run_completed");
}

#[test]
fn streaming_event_warning_serializes() {
    let event = make_event(AgentEventKind::Warning {
        message: "Rate limit approaching".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "warning");
}

#[test]
fn streaming_event_error_serializes() {
    let event = make_event(AgentEventKind::Error {
        message: "Backend timeout".into(),
        error_code: Some(ErrorCode::BackendTimeout),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["error_code"], "backend_timeout");
}

#[test]
fn streaming_event_file_changed_serializes() {
    let event = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "Added function".into(),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "file_changed");
    assert_eq!(json["path"], "src/main.rs");
}

#[test]
fn streaming_event_command_executed_serializes() {
    let event = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("All tests passed".into()),
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "command_executed");
    assert_eq!(json["exit_code"], 0);
}

#[test]
fn streaming_event_roundtrip_serde_all_variants() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        make_event(AgentEventKind::AssistantDelta { text: "tok".into() }),
        make_event(AgentEventKind::AssistantMessage {
            text: "full".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: Some("id".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: Some("id".into()),
            output: json!("ok"),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: Some(1),
            output_preview: None,
        }),
        make_event(AgentEventKind::Warning {
            message: "w".into(),
        }),
        make_event(AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        }),
    ];

    for event in &events {
        let json_str = serde_json::to_string(event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
        let json_str2 = serde_json::to_string(&back).unwrap();
        // Roundtrip should produce the same JSON (modulo timestamp precision)
        let v1: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&json_str2).unwrap();
        assert_eq!(v1["type"], v2["type"]);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 7: Error code mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_code_as_str_is_snake_case() {
    let codes = [
        (
            ErrorCode::MappingUnsupportedCapability,
            "mapping_unsupported_capability",
        ),
        (
            ErrorCode::MappingDialectMismatch,
            "mapping_dialect_mismatch",
        ),
        (
            ErrorCode::MappingLossyConversion,
            "mapping_lossy_conversion",
        ),
        (ErrorCode::MappingUnmappableTool, "mapping_unmappable_tool"),
        (ErrorCode::BackendTimeout, "backend_timeout"),
        (ErrorCode::BackendNotFound, "backend_not_found"),
        (ErrorCode::DialectUnknown, "dialect_unknown"),
        (ErrorCode::DialectMappingFailed, "dialect_mapping_failed"),
        (ErrorCode::IrLoweringFailed, "ir_lowering_failed"),
        (ErrorCode::IrInvalid, "ir_invalid"),
    ];
    for (code, expected) in &codes {
        assert_eq!(
            code.as_str(),
            *expected,
            "ErrorCode::{:?} as_str mismatch",
            code
        );
    }
}

#[test]
fn error_code_serde_roundtrip() {
    let codes = [
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendTimeout,
        ErrorCode::DialectMappingFailed,
    ];
    for code in &codes {
        let json = serde_json::to_string(code).unwrap();
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(*code, back);
    }
}

#[test]
fn map_error_unsupported_pair_contains_dialect_names() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"), "missing 'Kimi' in: {msg}");
    assert!(msg.contains("Copilot"), "missing 'Copilot' in: {msg}");
}

#[test]
fn map_error_lossy_conversion_contains_field() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "target has no thinking block".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
}

#[test]
fn map_error_unmappable_tool_contains_name() {
    let err = MapError::UnmappableTool {
        name: "apply_patch".into(),
        reason: "no equivalent".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("apply_patch"));
}

#[test]
fn map_error_unmappable_content_contains_field() {
    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "image blocks".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("system"));
}

#[test]
fn map_error_incompatible_capability_contains_capability() {
    let err = MapError::IncompatibleCapability {
        capability: "logprobs".into(),
        reason: "not supported".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
}

#[test]
fn map_error_all_variants_serialize_roundtrip() {
    let errors = vec![
        MapError::UnsupportedPair {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MapError::LossyConversion {
            field: "f".into(),
            reason: "r".into(),
        },
        MapError::UnmappableTool {
            name: "n".into(),
            reason: "r".into(),
        },
        MapError::IncompatibleCapability {
            capability: "c".into(),
            reason: "r".into(),
        },
        MapError::UnmappableContent {
            field: "f".into(),
            reason: "r".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 8: Thinking block handling across pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn thinking_preserved_only_for_claude_identity() {
    let conv = thinking_conversation();
    // Claude identity should preserve thinking
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Claude).unwrap();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &conv)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(has_thinking, "Claude identity should preserve thinking");
}

#[test]
fn thinking_dropped_for_non_claude_targets() {
    let conv = thinking_conversation();
    let non_claude_targets = [
        Dialect::OpenAi,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
        Dialect::Copilot,
    ];
    for &target in &non_claude_targets {
        // Use Claude as source (thinking blocks originate from Claude)
        if let Some(mapper) = default_ir_mapper(Dialect::Claude, target) {
            let result = mapper.map_request(Dialect::Claude, target, &conv).unwrap();
            let has_thinking = result.messages.iter().any(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
            });
            assert!(
                !has_thinking,
                "thinking should be dropped for Claude -> {}",
                dialect_label(target)
            );
        }
    }
}

#[test]
fn thinking_text_content_survives_even_when_thinking_dropped() {
    let conv = thinking_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        // The text "The answer is 42." should always survive
        let all_text: String = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Assistant)
            .map(|m| m.text_content())
            .collect::<Vec<_>>()
            .join("");
        if !all_text.is_empty() {
            assert!(
                all_text.contains("The answer is 42."),
                "text lost for {} -> {}: got '{}'",
                dialect_label(from),
                dialect_label(to),
                all_text
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 9: Image content block handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn image_preserved_non_codex_targets() {
    let conv = image_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex || to == Dialect::Kimi || to == Dialect::Copilot {
            continue; // Codex/Kimi/Copilot drop images
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_image = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
        });
        assert!(
            has_image,
            "image lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn codex_target_drops_images() {
    let conv = image_conversation();
    for &from in ALL_DIALECTS {
        if from == Dialect::Codex {
            continue; // Identity mapper is passthrough
        }
        if default_ir_mapper(from, Dialect::Codex).is_none() {
            continue;
        }
        let mapper = default_ir_mapper(from, Dialect::Codex).unwrap();
        let result = mapper.map_request(from, Dialect::Codex, &conv).unwrap();
        let has_image = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
        });
        assert!(
            !has_image,
            "Codex should drop images from {}",
            dialect_label(from)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 10: Fidelity scoring via validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_roundtrip_identity_is_lossless() {
    let validator = DefaultMappingValidator::new();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = validator.validate_roundtrip(&val, &val);
    assert!(result.is_lossless());
    assert!(result.lost_fields.is_empty());
    assert!(result.added_fields.is_empty());
    assert!(result.changed_fields.is_empty());
}

#[test]
fn validation_roundtrip_detects_lost_fields() {
    let validator = DefaultMappingValidator::new();
    let orig = json!({"model": "gpt-4", "temperature": 0.7, "messages": []});
    let roundtripped = json!({"model": "gpt-4", "messages": []});
    let result = validator.validate_roundtrip(&orig, &roundtripped);
    assert!(!result.is_lossless());
    assert!(result.lost_fields.contains(&"temperature".to_string()));
}

#[test]
fn validation_roundtrip_detects_added_fields() {
    let validator = DefaultMappingValidator::new();
    let orig = json!({"model": "gpt-4"});
    let roundtripped = json!({"model": "gpt-4", "extra_field": true});
    let result = validator.validate_roundtrip(&orig, &roundtripped);
    assert!(!result.is_lossless());
    assert!(result.added_fields.contains(&"extra_field".to_string()));
}

#[test]
fn validation_roundtrip_detects_changed_fields() {
    let validator = DefaultMappingValidator::new();
    let orig = json!({"model": "gpt-4", "temperature": 0.7});
    let roundtripped = json!({"model": "gpt-4", "temperature": 0.9});
    let result = validator.validate_roundtrip(&orig, &roundtripped);
    assert!(!result.is_lossless());
    assert!(result.changed_fields.contains(&"temperature".to_string()));
}

#[test]
fn validation_pre_mapping_all_dialects() {
    let validator = DefaultMappingValidator::new();
    let requests = [
        (Dialect::OpenAi, json!({"model": "gpt-4", "messages": []})),
        (
            Dialect::Claude,
            json!({"model": "claude-3", "messages": [], "max_tokens": 1024}),
        ),
        (
            Dialect::Gemini,
            json!({"model": "gemini-pro", "contents": []}),
        ),
        (Dialect::Codex, json!({"model": "codex", "messages": []})),
        (Dialect::Kimi, json!({"model": "kimi", "messages": []})),
        (
            Dialect::Copilot,
            json!({"model": "copilot", "messages": []}),
        ),
    ];
    for (dialect, req) in &requests {
        let result = validator.validate_pre_mapping(*dialect, req);
        assert!(
            result.is_valid(),
            "pre-mapping failed for {}: {:?}",
            dialect_label(*dialect),
            result.issues
        );
        assert_eq!(result.field_coverage, 100.0);
    }
}

#[test]
fn validation_field_coverage_partial() {
    let validator = DefaultMappingValidator::new();
    // OpenAI requires model + messages; provide only model
    let req = json!({"model": "gpt-4"});
    let result = validator.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(!result.is_valid());
    assert_eq!(result.field_coverage, 50.0);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 11: Early failure for unsupported mappings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unsupported_pairs_return_none_from_factory() {
    for (from, to) in unsupported_pairs() {
        assert!(
            default_ir_mapper(from, to).is_none(),
            "expected None for unsupported pair {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn specific_unsupported_pairs_verified() {
    // Verify specific known unsupported pairs
    let known_unsupported = [
        (Dialect::Kimi, Dialect::Copilot),
        (Dialect::Copilot, Dialect::Kimi),
        (Dialect::Codex, Dialect::Copilot),
        (Dialect::Copilot, Dialect::Codex),
        (Dialect::Codex, Dialect::Gemini),
        (Dialect::Gemini, Dialect::Codex),
        (Dialect::Codex, Dialect::Kimi),
        (Dialect::Kimi, Dialect::Codex),
        (Dialect::Copilot, Dialect::Claude),
        (Dialect::Claude, Dialect::Copilot),
        (Dialect::Copilot, Dialect::Gemini),
        (Dialect::Gemini, Dialect::Copilot),
    ];
    for (from, to) in &known_unsupported {
        assert!(
            default_ir_mapper(*from, *to).is_none(),
            "expected no mapper for {} -> {}",
            dialect_label(*from),
            dialect_label(*to)
        );
    }
}

#[test]
fn codex_to_claude_early_failure_on_unmappable_tool() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_patch".into(),
            input: json!({"patch": "..."}),
        }],
    )]);
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(result.is_err());
    match result.unwrap_err() {
        MapError::UnmappableTool { name, .. } => {
            assert_eq!(name, "apply_patch");
        }
        other => panic!("expected UnmappableTool, got {:?}", other),
    }
}

#[test]
fn codex_to_claude_early_failure_on_apply_diff() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t2".into(),
            name: "apply_diff".into(),
            input: json!({"diff": "..."}),
        }],
    )]);
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        MapError::UnmappableTool { .. }
    ));
}

#[test]
fn claude_to_gemini_early_failure_on_image_in_system() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "System prompt".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    )]);
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
    let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
    assert!(result.is_err());
    match result.unwrap_err() {
        MapError::UnmappableContent { field, .. } => {
            assert_eq!(field, "system");
        }
        other => panic!("expected UnmappableContent, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 12: Semantic preservation tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn message_count_preserved_or_increases_for_tool_split() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        if to == Dialect::Codex {
            // Codex drops system and tool messages
            continue;
        }
        assert!(
            result.len() >= conv.len() || from == Dialect::Codex,
            "message count dropped for {} -> {}: {} -> {}",
            dialect_label(from),
            dialect_label(to),
            conv.len(),
            result.len()
        );
    }
}

#[test]
fn role_distribution_makes_sense_after_mapping() {
    let conv = tool_call_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        // Should always have at least one user and one assistant message
        let has_user = result.messages.iter().any(|m| m.role == IrRole::User);
        let has_asst = result.messages.iter().any(|m| m.role == IrRole::Assistant);
        assert!(
            has_user,
            "no user message after {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
        assert!(
            has_asst,
            "no assistant message after {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn empty_conversation_stays_empty_all_pairs() {
    let conv = empty_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        assert!(
            result.is_empty(),
            "empty conversation became non-empty for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn metadata_preserved_through_all_non_codex_pairs() {
    let conv = metadata_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        // Find the user message and check metadata
        let user_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::User)
            .collect();
        assert!(
            !user_msgs.is_empty(),
            "no user message after {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
        assert_eq!(
            user_msgs[0].metadata.get("source"),
            Some(&json!("test_harness")),
            "metadata lost for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 13: Response mapping (map_response) all supported pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn response_mapping_works_all_supported_pairs() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_response(from, to, &conv);
        assert!(
            result.is_ok(),
            "response mapping failed for {} -> {}: {:?}",
            dialect_label(from),
            dialect_label(to),
            result.err()
        );
    }
}

#[test]
fn response_identity_preserves_full_conversation() {
    let conv = tool_call_conversation();
    for &d in ALL_DIALECTS {
        let mapper = default_ir_mapper(d, d).unwrap();
        let result = mapper.map_response(d, d, &conv).unwrap();
        assert_eq!(
            result,
            conv,
            "response identity failed for {}",
            dialect_label(d)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 14: Specific dialect pair tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_tool_result_role_change() {
    let conv = tool_call_conversation();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    // Tool messages should become User messages
    let tool_role_count = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert_eq!(
        tool_role_count, 0,
        "OpenAi->Claude should convert Tool to User"
    );
}

#[test]
fn claude_to_openai_tool_result_role_change() {
    let conv = multi_tool_conversation();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    // User messages with only ToolResult blocks should become Tool messages
    let tool_role_count = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert!(
        tool_role_count >= 2,
        "Claude->OpenAi should create Tool role messages"
    );
}

#[test]
fn openai_to_gemini_tool_result_role_change() {
    let conv = tool_call_conversation();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    // Tool messages become User in Gemini
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert!(tool_msgs.is_empty(), "Gemini should not have Tool role");
}

#[test]
fn gemini_to_openai_tool_result_split() {
    let conv = multi_tool_conversation();
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).unwrap();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(
        tool_msgs.len(),
        2,
        "Gemini->OpenAi should split tool results"
    );
}

#[test]
fn openai_to_kimi_near_identity() {
    let conv = simple_text_conversation();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).unwrap();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap();
    // Should be near-identical (only thinking stripped)
    assert_eq!(result.len(), conv.len());
    for (o, r) in conv.messages.iter().zip(result.messages.iter()) {
        assert_eq!(o.role, r.role);
        assert_eq!(o.text_content(), r.text_content());
    }
}

#[test]
fn openai_to_copilot_near_identity() {
    let conv = simple_text_conversation();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).unwrap();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
    for (o, r) in conv.messages.iter().zip(result.messages.iter()) {
        assert_eq!(o.role, r.role);
        assert_eq!(o.text_content(), r.text_content());
    }
}

#[test]
fn claude_to_kimi_tool_result_split() {
    let conv = multi_tool_conversation();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Kimi).unwrap();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(
        tool_msgs.len(),
        2,
        "Claude->Kimi should create Tool role for results"
    );
}

#[test]
fn kimi_to_claude_tool_role_to_user() {
    let conv = tool_call_conversation();
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Claude).unwrap();
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Claude, &conv)
        .unwrap();
    // Kimi tool role → Claude user role
    let tool_role_count = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert_eq!(
        tool_role_count, 0,
        "Kimi->Claude should convert Tool to User"
    );
}

#[test]
fn gemini_to_kimi_tool_result_split() {
    let conv = multi_tool_conversation();
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Kimi).unwrap();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2, "Gemini->Kimi should split tool results");
}

#[test]
fn kimi_to_gemini_tool_to_user() {
    let conv = tool_call_conversation();
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Gemini).unwrap();
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Gemini, &conv)
        .unwrap();
    let tool_role_count = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert_eq!(
        tool_role_count, 0,
        "Kimi->Gemini should convert Tool to User"
    );
}

#[test]
fn codex_to_openai_lossless() {
    // Simple text only - Codex output is just text
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Write code"),
        IrMessage::text(IrRole::Assistant, "fn main() {}"),
    ]);
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::OpenAi).unwrap();
    let result = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result, conv, "Codex->OpenAi simple text should be lossless");
}

#[test]
fn openai_to_codex_drops_everything_except_text() {
    let conv = tool_call_conversation();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Codex).unwrap();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    // Should only have text content blocks
    for msg in &result.messages {
        for block in &msg.content {
            assert!(
                matches!(block, IrContentBlock::Text { .. }),
                "Codex should only have Text blocks"
            );
        }
    }
}

#[test]
fn claude_to_codex_drops_system_tool_thinking() {
    let mut msgs = Vec::new();
    msgs.push(IrMessage::text(IrRole::System, "System prompt"));
    msgs.push(IrMessage::text(IrRole::User, "Hello"));
    msgs.push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "thinking...".into(),
            },
            IrContentBlock::Text {
                text: "response".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            },
        ],
    ));
    msgs.push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text { text: "ok".into() }],
            is_error: false,
        }],
    ));
    let conv = IrConversation::from_messages(msgs);
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Codex).unwrap();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();
    // System and Tool messages dropped
    assert!(result.system_message().is_none());
    assert!(result.messages.iter().all(|m| m.role != IrRole::Tool));
    // Only text blocks remain
    for msg in &result.messages {
        for block in &msg.content {
            assert!(matches!(block, IrContentBlock::Text { .. }));
        }
    }
}

#[test]
fn gemini_to_claude_tool_role_to_user() {
    let conv = tool_call_conversation();
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Claude).unwrap();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &conv)
        .unwrap();
    // Gemini Tool role → Claude User role
    let tool_role_count = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert_eq!(
        tool_role_count, 0,
        "Gemini->Claude should convert Tool to User"
    );
}

#[test]
fn claude_to_gemini_system_preserved() {
    let conv = simple_text_conversation();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    let sys = result.system_message().unwrap();
    assert_eq!(sys.text_content(), "You are a helpful assistant.");
}

#[test]
fn codex_to_claude_no_unmappable_tool_passes() {
    // Regular tool calls should work fine
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "bash".into(),
            input: json!({"cmd": "ls"}),
        }],
    )]);
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 15: Supported pairs list specific content
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn supported_pairs_includes_all_identity() {
    let pairs = supported_ir_pairs();
    for &d in ALL_DIALECTS {
        assert!(
            pairs.contains(&(d, d)),
            "missing identity pair for {}",
            dialect_label(d)
        );
    }
}

#[test]
fn supported_pairs_includes_openai_claude() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
}

#[test]
fn supported_pairs_includes_openai_gemini() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
}

#[test]
fn supported_pairs_includes_claude_gemini() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
}

#[test]
fn supported_pairs_includes_openai_codex() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Codex)));
    assert!(pairs.contains(&(Dialect::Codex, Dialect::OpenAi)));
}

#[test]
fn supported_pairs_includes_openai_kimi() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::OpenAi)));
}

#[test]
fn supported_pairs_includes_claude_kimi() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Claude)));
}

#[test]
fn supported_pairs_includes_openai_copilot() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
    assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
}

#[test]
fn supported_pairs_includes_gemini_kimi() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
}

#[test]
fn supported_pairs_includes_codex_claude() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 16: IrMapper trait compliance tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_mappers_support_their_declared_pairs() {
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let supported = mapper.supported_pairs();
        assert!(
            supported.contains(&(from, to)),
            "mapper for {} -> {} doesn't declare its own pair in supported_pairs()",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn all_mappers_handle_empty_conversation() {
    let conv = IrConversation::new();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let req_result = mapper.map_request(from, to, &conv);
        let resp_result = mapper.map_response(from, to, &conv);
        assert!(
            req_result.is_ok(),
            "empty conv request failed for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
        assert!(
            resp_result.is_ok(),
            "empty conv response failed for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn all_mappers_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    // All mappers returned from factory must be Send + Sync
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        fn check(_m: &(dyn IrMapper + Send + Sync)) {}
        check(mapper.as_ref());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 17: Dialect enum exhaustiveness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_6() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_labels_correct() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn dialect_display_uses_label() {
    for &d in ALL_DIALECTS {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_serde_roundtrip() {
    for &d in ALL_DIALECTS {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 18: Complex multi-step mapping scenarios
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn three_hop_mapping_preserves_text() {
    // OpenAI -> Claude -> Gemini: text should survive
    let conv = simple_text_conversation();
    let m1 = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let m2 = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();

    let step1 = m1
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let step2 = m2
        .map_request(Dialect::Claude, Dialect::Gemini, &step1)
        .unwrap();

    let user_text: String = step2
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .collect();
    assert!(user_text.contains("Hello, how are you?"));
}

#[test]
fn three_hop_mapping_tool_calls_survive() {
    // OpenAI -> Claude -> Kimi: tool calls should survive
    let conv = tool_call_conversation();
    let m1 = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let m2 = default_ir_mapper(Dialect::Claude, Dialect::Kimi).unwrap();

    let step1 = m1
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let step2 = m2
        .map_request(Dialect::Claude, Dialect::Kimi, &step1)
        .unwrap();

    let tools = step2.tool_calls();
    assert!(!tools.is_empty(), "tool calls lost in three-hop mapping");
}

#[test]
fn four_hop_mapping_openai_claude_gemini_kimi() {
    let conv = simple_text_conversation();
    let m1 = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let m2 = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
    let m3 = default_ir_mapper(Dialect::Gemini, Dialect::Kimi).unwrap();

    let s1 = m1
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let s2 = m2
        .map_request(Dialect::Claude, Dialect::Gemini, &s1)
        .unwrap();
    let s3 = m3.map_request(Dialect::Gemini, Dialect::Kimi, &s2).unwrap();

    let user_text: String = s3
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .collect();
    assert!(user_text.contains("Hello, how are you?"));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 19: Conversation accessor consistency after mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn last_assistant_accessible_after_mapping() {
    let conv = simple_text_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let last = result.last_assistant();
        assert!(
            last.is_some(),
            "no last_assistant after {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}

#[test]
fn messages_by_role_consistent_after_mapping() {
    let conv = tool_call_conversation();
    for (from, to) in supported_ir_pairs() {
        if to == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        // Total messages should equal sum of all role counts
        let total = result.len();
        let by_role_count = result.messages_by_role(IrRole::System).len()
            + result.messages_by_role(IrRole::User).len()
            + result.messages_by_role(IrRole::Assistant).len()
            + result.messages_by_role(IrRole::Tool).len();
        assert_eq!(
            total,
            by_role_count,
            "role distribution mismatch for {} -> {}",
            dialect_label(from),
            dialect_label(to)
        );
    }
}
