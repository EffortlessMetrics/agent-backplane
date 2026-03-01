// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for streaming parity, thinking blocks, tool use flow,
//! passthrough fidelity, and stop reason mapping.

use abp_claude_sdk::dialect::{
    ClaudeApiError, ClaudeContentBlock, ClaudeMessageDelta, ClaudeResponse, ClaudeStopReason,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage, ThinkingConfig, from_passthrough_event,
    map_response, map_stop_reason, map_stream_event, parse_stop_reason, to_passthrough_event,
    verify_passthrough_fidelity,
};
use abp_core::AgentEventKind;

// ---------------------------------------------------------------------------
// Streaming event serde roundtrip (one per event type)
// ---------------------------------------------------------------------------

#[test]
fn serde_roundtrip_message_start() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_start_rt".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: Some(ClaudeUsage {
                input_tokens: 25,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_content_block_start_text() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_content_block_start_tool_use() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 1,
        content_block: ClaudeContentBlock::ToolUse {
            id: "toolu_01".into(),
            name: "bash".into(),
            input: serde_json::json!({}),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_content_block_delta_text() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "Hello, ".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_content_block_delta_thinking() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "Let me think about this...".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_content_block_stop() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 2 };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_message_delta() {
    let event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 0,
            output_tokens: 15,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_ping() {
    let event = ClaudeStreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn serde_roundtrip_error() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "API is temporarily overloaded".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

// ---------------------------------------------------------------------------
// Thinking block → ABP event mapping
// ---------------------------------------------------------------------------

#[test]
fn thinking_block_maps_to_assistant_message_with_ext() {
    let resp = ClaudeResponse {
        id: "msg_think".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Thinking {
                thinking: "I should analyze the code structure first.".into(),
                signature: Some("sig_abc123".into()),
            },
            ClaudeContentBlock::Text {
                text: "Here is my analysis.".into(),
            },
        ],
        stop_reason: Some("end_turn".into()),
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);

    // First event: thinking block
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text, "I should analyze the code structure first.");
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
    let ext = events[0].ext.as_ref().expect("thinking event should have ext");
    assert_eq!(ext.get("thinking"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        ext.get("signature"),
        Some(&serde_json::Value::String("sig_abc123".into()))
    );

    // Second event: normal text
    match &events[1].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text, "Here is my analysis.");
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
    assert!(events[1].ext.is_none());
}

#[test]
fn thinking_block_without_signature_omits_it_in_ext() {
    let resp = ClaudeResponse {
        id: "msg_think2".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Thinking {
            thinking: "Considering options...".into(),
            signature: None,
        }],
        stop_reason: Some("end_turn".into()),
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.get("signature").is_none());
}

#[test]
fn thinking_delta_maps_to_assistant_delta_with_ext() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "Let me consider...".into(),
        },
    };
    let agent_events = map_stream_event(&event);
    assert_eq!(agent_events.len(), 1);
    match &agent_events[0].kind {
        AgentEventKind::AssistantDelta { text } => {
            assert_eq!(text, "Let me consider...");
        }
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
    let ext = agent_events[0]
        .ext
        .as_ref()
        .expect("thinking delta should have ext");
    assert_eq!(ext.get("thinking"), Some(&serde_json::Value::Bool(true)));
}

#[test]
fn thinking_config_serde_roundtrip() {
    let cfg = ThinkingConfig::new(10000);
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("\"budget_tokens\":10000"));
    assert!(json.contains("\"type\":\"enabled\""));
    let parsed: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cfg);
}

// ---------------------------------------------------------------------------
// Tool use content block → ABP ToolCall conversion
// ---------------------------------------------------------------------------

#[test]
fn tool_use_content_block_maps_to_tool_call() {
    let resp = ClaudeResponse {
        id: "msg_tool".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::ToolUse {
            id: "toolu_01A".into(),
            name: "read_file".into(),
            input: serde_json::json!({"path": "src/main.rs"}),
        }],
        stop_reason: Some("tool_use".into()),
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("toolu_01A"));
            assert!(parent_tool_use_id.is_none());
            assert_eq!(input, &serde_json::json!({"path": "src/main.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn tool_result_content_block_maps_to_tool_result() {
    let resp = ClaudeResponse {
        id: "msg_tr".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "toolu_01A".into(),
            content: Some("file contents here".into()),
            is_error: None,
        }],
        stop_reason: None,
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_use_id,
            output,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id.as_deref(), Some("toolu_01A"));
            assert_eq!(output, &serde_json::Value::String("file contents here".into()));
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn stream_content_block_start_tool_use_maps_to_tool_call() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 1,
        content_block: ClaudeContentBlock::ToolUse {
            id: "toolu_stream_01".into(),
            name: "bash".into(),
            input: serde_json::json!({}),
        },
    };
    let agent_events = map_stream_event(&event);
    assert_eq!(agent_events.len(), 1);
    match &agent_events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_use_id.as_deref(), Some("toolu_stream_01"));
            assert_eq!(input, &serde_json::json!({}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_content_block_start_text_maps_to_nothing() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let agent_events = map_stream_event(&event);
    assert!(agent_events.is_empty());
}

// ---------------------------------------------------------------------------
// Passthrough stream equivalence
// ---------------------------------------------------------------------------

#[test]
fn passthrough_roundtrip_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "Hello world".into(),
        },
    };
    let wrapped = to_passthrough_event(&event);
    assert!(wrapped.ext.is_some());
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(
        ext.get("dialect"),
        Some(&serde_json::Value::String("claude".into()))
    );
    let recovered = from_passthrough_event(&wrapped).expect("should recover event");
    assert_eq!(recovered, event);
}

#[test]
fn passthrough_roundtrip_full_stream() {
    let events = vec![
        ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_pt".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        },
        ClaudeStreamEvent::Ping {},
        ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text {
                text: String::new(),
            },
        },
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "Hello".into(),
            },
        },
        ClaudeStreamEvent::ContentBlockStop { index: 0 },
        ClaudeStreamEvent::MessageDelta {
            delta: ClaudeMessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: Some(ClaudeUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        },
        ClaudeStreamEvent::MessageStop {},
    ];
    assert!(verify_passthrough_fidelity(&events));
}

#[test]
fn passthrough_roundtrip_error_event() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "invalid_request_error".into(),
            message: "max_tokens must be positive".into(),
        },
    };
    let wrapped = to_passthrough_event(&event);
    let recovered = from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn passthrough_from_non_passthrough_returns_none() {
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "hi".into(),
        },
        ext: None,
    };
    assert!(from_passthrough_event(&event).is_none());
}

// ---------------------------------------------------------------------------
// Stop reason mapping
// ---------------------------------------------------------------------------

#[test]
fn stop_reason_end_turn_roundtrip() {
    let parsed = parse_stop_reason("end_turn").unwrap();
    assert_eq!(parsed, ClaudeStopReason::EndTurn);
    assert_eq!(map_stop_reason(parsed), "end_turn");
}

#[test]
fn stop_reason_tool_use_roundtrip() {
    let parsed = parse_stop_reason("tool_use").unwrap();
    assert_eq!(parsed, ClaudeStopReason::ToolUse);
    assert_eq!(map_stop_reason(parsed), "tool_use");
}

#[test]
fn stop_reason_max_tokens_roundtrip() {
    let parsed = parse_stop_reason("max_tokens").unwrap();
    assert_eq!(parsed, ClaudeStopReason::MaxTokens);
    assert_eq!(map_stop_reason(parsed), "max_tokens");
}

#[test]
fn stop_reason_stop_sequence_roundtrip() {
    let parsed = parse_stop_reason("stop_sequence").unwrap();
    assert_eq!(parsed, ClaudeStopReason::StopSequence);
    assert_eq!(map_stop_reason(parsed), "stop_sequence");
}

#[test]
fn stop_reason_unknown_returns_none() {
    assert!(parse_stop_reason("unknown_reason").is_none());
}

#[test]
fn stop_reason_serde_roundtrip() {
    let reasons = [
        ClaudeStopReason::EndTurn,
        ClaudeStopReason::ToolUse,
        ClaudeStopReason::MaxTokens,
        ClaudeStopReason::StopSequence,
    ];
    for reason in &reasons {
        let json = serde_json::to_string(reason).unwrap();
        let parsed: ClaudeStopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, reason);
    }
}
