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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! End-to-end tests for the sidecar process lifecycle.
//!
//! These tests exercise the full protocol flow without requiring external
//! processes by using in-process mock sidecars (child scripts) and the
//! `MockBackend` from `abp-integrations`.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    Outcome, Receipt, ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_integrations::{Backend, MockBackend};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use tokio::sync::mpsc;

use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a minimal `WorkOrder` for testing.
fn test_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

/// Build a mock `BackendIdentity`.
fn mock_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1".into()),
    }
}

/// Build a minimal capability manifest.
fn mock_capabilities() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m
}

/// Helper to create an `AgentEvent` of a given kind.
fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

/// Build a valid `Receipt` for testing protocol flows.
fn mock_receipt(_run_id: Uuid, wo_id: Uuid) -> Receipt {
    ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .work_order_id(wo_id)
        .build()
}

/// Encode a sequence of envelopes into a JSONL `Vec<u8>` buffer.
fn encode_envelopes(envelopes: &[Envelope]) -> Vec<u8> {
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, envelopes).unwrap();
    buf
}

// ===========================================================================
// Section 1: Protocol codec basics
// ===========================================================================

#[test]
fn codec_hello_roundtrip() {
    let hello = Envelope::hello(mock_identity(), mock_capabilities());
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn codec_run_roundtrip() {
    let wo = test_work_order("test");
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { id, .. } if id == "run-1"));
}

#[test]
fn codec_event_roundtrip() {
    let event = make_event(AgentEventKind::AssistantDelta {
        text: "hello".into(),
    });
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(event.kind, AgentEventKind::AssistantDelta { .. }));
        }
        _ => panic!("expected Event envelope"),
    }
}

#[test]
fn codec_final_roundtrip() {
    let receipt = mock_receipt(Uuid::nil(), Uuid::nil());
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn codec_fatal_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something went wrong".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-1".into()));
            assert_eq!(error, "something went wrong");
        }
        _ => panic!("expected Fatal envelope"),
    }
}

#[test]
fn codec_fatal_no_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "pre-run error".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal envelope"),
    }
}

// ===========================================================================
// Section 2: Protocol handshake validation
// ===========================================================================

#[test]
fn handshake_hello_contains_contract_version() {
    let hello = Envelope::hello(mock_identity(), mock_capabilities());
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains(&format!("\"contract_version\":\"{CONTRACT_VERSION}\"")));
}

#[test]
fn handshake_hello_has_t_tag() {
    let hello = Envelope::hello(mock_identity(), mock_capabilities());
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains("\"t\":\"hello\""));
}

#[test]
fn handshake_run_has_t_tag() {
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: test_work_order("t"),
    };
    let encoded = JsonlCodec::encode(&run).unwrap();
    assert!(encoded.contains("\"t\":\"run\""));
}

#[test]
fn handshake_event_has_t_tag() {
    let event = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains("\"t\":\"event\""));
}

#[test]
fn handshake_final_has_t_tag() {
    let receipt = mock_receipt(Uuid::nil(), Uuid::nil());
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains("\"t\":\"final\""));
}

#[test]
fn handshake_fatal_has_t_tag() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains("\"t\":\"fatal\""));
}

#[test]
fn handshake_hello_then_run_sequence() {
    let hello = Envelope::hello(mock_identity(), mock_capabilities());
    let run = Envelope::Run {
        id: "r1".into(),
        work_order: test_work_order("t"),
    };

    let buf = encode_envelopes(&[hello, run]);
    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Run { .. }));
}

#[test]
fn handshake_event_ref_id_matches_run_id() {
    let run_id = "test-run-42";
    let event = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
    let env = Envelope::Event {
        ref_id: run_id.into(),
        event,
    };
    match JsonlCodec::decode(JsonlCodec::encode(&env).unwrap().trim_end()).unwrap() {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, run_id),
        _ => panic!("expected Event"),
    }
}

#[test]
fn handshake_final_ref_id_matches_run_id() {
    let run_id = "test-run-42";
    let receipt = mock_receipt(Uuid::nil(), Uuid::nil());
    let env = Envelope::Final {
        ref_id: run_id.into(),
        receipt,
    };
    match JsonlCodec::decode(JsonlCodec::encode(&env).unwrap().trim_end()).unwrap() {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, run_id),
        _ => panic!("expected Final"),
    }
}

// ===========================================================================
// Section 3: Error handling — invalid JSON, protocol violations
// ===========================================================================

#[test]
fn error_decode_invalid_json() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn error_decode_empty_object() {
    // Missing required `t` tag.
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn error_decode_unknown_envelope_type() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":42}"#);
    assert!(result.is_err());
}

#[test]
fn error_decode_malformed_hello() {
    // Has `t` but missing required fields.
    let result = JsonlCodec::decode(r#"{"t":"hello"}"#);
    assert!(result.is_err());
}

#[test]
fn error_decode_malformed_run() {
    let result = JsonlCodec::decode(r#"{"t":"run"}"#);
    assert!(result.is_err());
}

#[test]
fn error_decode_malformed_event_missing_ref_id() {
    let result = JsonlCodec::decode(r#"{"t":"event"}"#);
    assert!(result.is_err());
}

#[test]
fn error_decode_stream_with_bad_line() {
    let input = format!(
        "{}\nnot valid json\n",
        JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "err".into(),
            error_code: None,
        })
        .unwrap()
        .trim_end()
    );
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn error_decode_stream_blank_lines_skipped() {
    let hello_line =
        JsonlCodec::encode(&Envelope::hello(mock_identity(), mock_capabilities())).unwrap();
    let input = format!("\n\n{}\n\n", hello_line.trim_end());
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0], Envelope::Hello { .. }));
}

#[test]
fn error_fatal_envelope_terminates_protocol() {
    let run_id = "run-1";
    let envelopes = vec![
        Envelope::hello(mock_identity(), mock_capabilities()),
        Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        },
        Envelope::Fatal {
            ref_id: Some(run_id.into()),
            error: "crash!".into(),
            error_code: None,
        },
    ];

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[2], Envelope::Fatal { .. }));
}

#[test]
fn error_fatal_without_ref_id_is_valid() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "pre-run crash".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "pre-run crash");
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// Section 4: Full protocol simulation (hello→run→events→final)
// ===========================================================================

#[test]
fn full_protocol_flow_simulation() {
    let run_id = "run-full";
    let wo = test_work_order("full test");
    let wo_id = wo.id;
    let receipt = mock_receipt(Uuid::nil(), wo_id);

    let envelopes = vec![
        Envelope::hello(mock_identity(), mock_capabilities()),
        Envelope::Run {
            id: run_id.into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: "chunk 1".into(),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: "chunk 2".into(),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt,
        },
    ];

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 7);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));

    // Verify events 2-5 have correct ref_id.
    for env in &decoded[2..6] {
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, run_id),
            other => panic!("expected Event, got {other:?}"),
        }
    }
    assert!(matches!(&decoded[6], Envelope::Final { ref_id, .. } if ref_id == run_id));
}

#[test]
fn full_protocol_with_fatal_instead_of_final() {
    let run_id = "run-fatal";
    let envelopes = vec![
        Envelope::hello(mock_identity(), mock_capabilities()),
        Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
        },
        Envelope::Fatal {
            ref_id: Some(run_id.into()),
            error: "out of memory".into(),
            error_code: None,
        },
    ];

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 3);
    match &decoded[2] {
        Envelope::Fatal { error, .. } => assert_eq!(error, "out of memory"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ===========================================================================
// Section 5: Event streaming verification
// ===========================================================================

#[test]
fn event_stream_multiple_text_deltas() {
    let run_id = "run-deltas";
    let envelopes: Vec<Envelope> = (0..10)
        .map(|i| Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: format!("delta-{i}"),
            }),
        })
        .collect();

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 10);
    for (i, env) in decoded.iter().enumerate() {
        match env {
            Envelope::Event { ref_id, event } => {
                assert_eq!(ref_id, run_id);
                match &event.kind {
                    AgentEventKind::AssistantDelta { text } => {
                        assert_eq!(text, &format!("delta-{i}"));
                    }
                    other => panic!("expected AssistantDelta, got {other:?}"),
                }
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }
}

#[test]
fn event_stream_tool_use_events() {
    let run_id = "run-tools";
    let tool_call = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/tmp/test.txt"}),
    });
    let tool_result = make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        output: "file contents here".into(),
        is_error: false,
    });

    let envelopes = vec![
        Envelope::Event {
            ref_id: run_id.into(),
            event: tool_call,
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: tool_result,
        },
    ];

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 2);
    match &decoded[0] {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file")
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
    match &decoded[1] {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::ToolResult { is_error, .. } if !is_error)
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_stream_mixed_event_types() {
    let run_id = "run-mixed";
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "begin".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "thinking...".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"command": "ls"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            output: "file1.txt\nfile2.txt".into(),
            is_error: false,
        }),
        make_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        }),
        make_event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("Compiling...".into()),
        }),
        make_event(AgentEventKind::Warning {
            message: "unused var".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "all done".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let envelopes: Vec<Envelope> = events
        .into_iter()
        .map(|event| Envelope::Event {
            ref_id: run_id.into(),
            event,
        })
        .collect();

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 9);

    // Verify ordering is preserved.
    let kinds: Vec<&str> = decoded
        .iter()
        .map(|e| match e {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::RunStarted { .. } => "run_started",
                AgentEventKind::AssistantDelta { .. } => "assistant_delta",
                AgentEventKind::ToolCall { .. } => "tool_call",
                AgentEventKind::ToolResult { .. } => "tool_result",
                AgentEventKind::FileChanged { .. } => "file_changed",
                AgentEventKind::CommandExecuted { .. } => "command_executed",
                AgentEventKind::Warning { .. } => "warning",
                AgentEventKind::AssistantMessage { .. } => "assistant_message",
                AgentEventKind::RunCompleted { .. } => "run_completed",
                _ => "other",
            },
            _ => "non_event",
        })
        .collect();

    assert_eq!(
        kinds,
        vec![
            "run_started",
            "assistant_delta",
            "tool_call",
            "tool_result",
            "file_changed",
            "command_executed",
            "warning",
            "assistant_message",
            "run_completed",
        ]
    );
}

#[test]
fn event_stream_ordering_preserved_large_batch() {
    let run_id = "run-order";
    let envelopes: Vec<Envelope> = (0..100)
        .map(|i| Envelope::Event {
            ref_id: run_id.into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: format!("msg-{i:03}"),
            }),
        })
        .collect();

    let buf = encode_envelopes(&envelopes);
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 100);
    for (i, env) in decoded.iter().enumerate() {
        if let Envelope::Event { event, .. } = env
            && let AgentEventKind::AssistantDelta { text } = &event.kind
        {
            assert_eq!(text, &format!("msg-{i:03}"));
        }
    }
}

#[test]
fn event_stream_error_event_type() {
    let event = make_event(AgentEventKind::Error {
        message: "rate limit exceeded".into(),
        error_code: None,
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                &event.kind,
                AgentEventKind::Error { message, .. } if message == "rate limit exceeded"
            ));
        }
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// Section 6: Backend integration (MockBackend)
// ===========================================================================

#[tokio::test]
async fn mock_backend_identity() {
    let backend = MockBackend;
    let id = backend.identity();
    assert_eq!(id.id, "mock");
    assert_eq!(id.backend_version, Some("0.1".into()));
    assert_eq!(id.adapter_version, Some("0.1".into()));
}

#[tokio::test]
async fn mock_backend_capabilities() {
    let backend = MockBackend;
    let caps = backend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_backend_run_returns_complete() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), test_work_order("hello"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn mock_backend_streams_events() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), test_work_order("hello"), tx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    assert!(
        events.len() >= 4,
        "expected at least 4 events, got {}",
        events.len()
    );

    // First event should be RunStarted.
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    // Last event should be RunCompleted.
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn mock_backend_receipt_has_hash() {
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), test_work_order("hash test"), tx)
        .await
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn mock_backend_receipt_trace_matches_events() {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), test_work_order("trace test"), tx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // The trace in receipt should match streamed events.
    assert_eq!(receipt.trace.len(), events.len());
    for (i, (trace_ev, stream_ev)) in receipt.trace.iter().zip(events.iter()).enumerate() {
        assert_eq!(
            std::mem::discriminant(&trace_ev.kind),
            std::mem::discriminant(&stream_ev.kind),
            "event kind mismatch at index {i}"
        );
    }
}

#[tokio::test]
async fn mock_backend_receipt_metadata() {
    let run_id = Uuid::new_v4();
    let wo = test_work_order("meta test");
    let wo_id = wo.id;
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend.run(run_id, wo, tx).await.unwrap();

    assert_eq!(receipt.meta.run_id, run_id);
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn mock_backend_concurrent_runs() {
    let mut handles = Vec::new();
    for i in 0..5 {
        let handle = tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            let receipt = MockBackend
                .run(
                    Uuid::new_v4(),
                    test_work_order(&format!("concurrent-{i}")),
                    tx,
                )
                .await
                .unwrap();
            assert_eq!(receipt.outcome, Outcome::Complete);
            receipt
        });
        handles.push(handle);
    }

    for h in handles {
        let receipt = h.await.unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }
}

#[tokio::test]
async fn mock_backend_event_timestamps_monotonic() {
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), test_work_order("timestamps"), tx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "event timestamps should be monotonically non-decreasing"
        );
    }
}

// ===========================================================================
// Section 7: Receipt builder and validation
// ===========================================================================

#[test]
fn receipt_builder_default_outcome() {
    let receipt = ReceiptBuilder::new("test-backend").build();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_with_hash() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn receipt_builder_with_trace() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let mut builder = ReceiptBuilder::new("test");
    for ev in &events {
        builder = builder.add_trace_event(ev.clone());
    }
    let receipt = builder.build();
    assert_eq!(receipt.trace.len(), 3);
}

#[test]
fn receipt_builder_partial_outcome() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Partial)
        .build();
    assert_eq!(receipt.outcome, Outcome::Partial);
}

#[test]
fn receipt_builder_failed_outcome() {
    let receipt = ReceiptBuilder::new("test").outcome(Outcome::Failed).build();
    assert_eq!(receipt.outcome, Outcome::Failed);
}

// ===========================================================================
// Section 8: Version compatibility
// ===========================================================================

#[test]
fn version_parse_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn version_parse_invalid() {
    assert_eq!(abp_protocol::parse_version("invalid"), None);
    assert_eq!(abp_protocol::parse_version("v0.1"), None);
    assert_eq!(abp_protocol::parse_version("abp/0.1"), None);
    assert_eq!(abp_protocol::parse_version(""), None);
}

#[test]
fn version_compatibility_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(abp_protocol::is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn version_compatibility_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn version_compatibility_invalid_returns_false() {
    assert!(!abp_protocol::is_compatible_version("garbage", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "garbage"));
}

// ===========================================================================
// Section 9: Encode-to-writer integration
// ===========================================================================

#[test]
fn encode_to_writer_single() {
    let env = Envelope::hello(mock_identity(), mock_capabilities());
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let line = String::from_utf8(buf).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"hello\""));
}

#[test]
fn encode_many_to_writer_multiple() {
    let envs = vec![
        Envelope::hello(mock_identity(), mock_capabilities()),
        Envelope::Fatal {
            ref_id: None,
            error: "err".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn encode_decode_roundtrip_via_writer() {
    let envs = vec![
        Envelope::hello(mock_identity(), mock_capabilities()),
        Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: mock_receipt(Uuid::nil(), Uuid::nil()),
        },
    ];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

// ===========================================================================
// Section 10: WorkOrder builder integration
// ===========================================================================

#[test]
fn work_order_builder_minimal() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
    assert!(wo.context.files.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
}

#[test]
fn work_order_builder_with_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    assert_eq!(wo.config.model, Some("gpt-4".into()));
}

#[test]
fn work_order_serializes_in_run_envelope() {
    let wo = WorkOrderBuilder::new("e2e task").model("claude-3").build();
    let env = Envelope::Run {
        id: "run-wo".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-wo");
            assert_eq!(work_order.task, "e2e task");
            assert_eq!(work_order.config.model, Some("claude-3".into()));
        }
        _ => panic!("expected Run envelope"),
    }
}

#[test]
fn work_order_with_budget_and_turns() {
    let wo = WorkOrderBuilder::new("limited task")
        .max_budget_usd(5.0)
        .max_turns(10)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(10));
}

// ===========================================================================
// Section 11: HostError types (unit-level checks)
// ===========================================================================

#[test]
fn host_error_display_fatal() {
    let err = abp_host::HostError::Fatal("crash occurred".into());
    let msg = format!("{err}");
    assert!(msg.contains("crash occurred"));
}

#[test]
fn host_error_display_violation() {
    let err = abp_host::HostError::Violation("out of order".into());
    let msg = format!("{err}");
    assert!(msg.contains("out of order"));
}

#[test]
fn host_error_display_timeout() {
    let err = abp_host::HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = format!("{err}");
    assert!(msg.contains("30"));
}

#[test]
fn host_error_display_exited() {
    let err = abp_host::HostError::Exited { code: Some(1) };
    let msg = format!("{err}");
    assert!(msg.contains("1"));
}

#[test]
fn host_error_display_sidecar_crashed() {
    let err = abp_host::HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed by OOM".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("137"));
    assert!(msg.contains("killed by OOM"));
}

// ===========================================================================
// Section 12: SidecarSpec construction
// ===========================================================================

#[test]
fn sidecar_spec_new() {
    let spec = abp_host::SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_with_args_and_env() {
    let mut spec = abp_host::SidecarSpec::new("python3");
    spec.args = vec!["host.py".into()];
    spec.env
        .insert("PYTHONPATH".into(), "/usr/lib/python".into());
    spec.cwd = Some("/tmp".into());

    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args.len(), 1);
    assert_eq!(spec.env.get("PYTHONPATH").unwrap(), "/usr/lib/python");
    assert_eq!(spec.cwd, Some("/tmp".into()));
}

#[test]
fn sidecar_spec_serializes() {
    let spec = abp_host::SidecarSpec::new("test-cmd");
    let json = serde_json::to_string(&spec).unwrap();
    assert!(json.contains("test-cmd"));
    let deserialized: abp_host::SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.command, "test-cmd");
}
