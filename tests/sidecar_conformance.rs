// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar protocol conformance test suite.
//!
//! Validates that JSONL protocol streams conform to the ABP sidecar contract:
//! handshake, event correlation, envelope ordering, error handling, and encoding.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use std::collections::BTreeMap;
use std::io::BufReader;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "conformance-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let now = Utc::now();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: Receipt {
            meta: RunMetadata {
                run_id: Uuid::new_v4(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: now,
                finished_at: now,
                duration_ms: 10,
            },
            backend: test_backend(),
            capabilities: test_capabilities(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        },
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

/// Encode envelopes into a single JSONL buffer.
fn encode_stream(envelopes: &[Envelope]) -> Vec<u8> {
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, envelopes).unwrap();
    buf
}

/// Decode a JSONL buffer into a vec of envelopes.
fn decode_all(buf: &[u8]) -> Vec<Result<Envelope, ProtocolError>> {
    let reader = BufReader::new(buf);
    JsonlCodec::decode_stream(reader).collect()
}

// ═════════════════════════════════════════════════════════════════════════
// 1. Handshake validation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn handshake_hello_must_be_first_envelope() {
    let buf = encode_stream(&[make_hello(), make_final("run-1")]);
    let envelopes: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(
        matches!(envelopes[0], Envelope::Hello { .. }),
        "first envelope must be Hello"
    );
}

#[test]
fn handshake_hello_must_include_contract_version() {
    let hello = make_hello();
    let json = serde_json::to_value(&hello).unwrap();
    let cv = json["contract_version"].as_str().unwrap();
    assert_eq!(cv, CONTRACT_VERSION);

    // Also validate via round-trip decode
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn handshake_non_hello_first_is_detectable() {
    // If a sidecar sends an event before hello, the host should detect it.
    let event = make_event("run-1", AgentEventKind::RunStarted {
        message: "oops".into(),
    });
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(
        !matches!(decoded, Envelope::Hello { .. }),
        "event envelope must not be confused with Hello"
    );
}

#[test]
fn handshake_hello_with_empty_capabilities_is_valid() {
    let hello = Envelope::hello(test_backend(), BTreeMap::new());
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 2. Event streaming: ref_id correlation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn event_ref_id_must_match_run_id() {
    let run_id = "run-abc";
    let event = make_event(run_id, AgentEventKind::AssistantMessage {
        text: "hi".into(),
    });
    if let Envelope::Event { ref_id, .. } = &event {
        assert_eq!(ref_id, run_id);
    } else {
        panic!("expected Event");
    }

    // A mismatched ref_id is structurally valid but semantically wrong.
    let wrong = make_event("run-OTHER", AgentEventKind::AssistantMessage {
        text: "hi".into(),
    });
    if let Envelope::Event { ref_id, .. } = &wrong {
        assert_ne!(ref_id, run_id, "mismatched ref_id must be detectable");
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_stream_all_ref_ids_consistent() {
    let run_id = "run-42";
    let events = vec![
        make_hello(),
        make_event(run_id, AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(run_id, AgentEventKind::AssistantDelta {
            text: "chunk1".into(),
        }),
        make_event(run_id, AgentEventKind::AssistantDelta {
            text: "chunk2".into(),
        }),
        make_event(run_id, AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        make_final(run_id),
    ];

    let buf = encode_stream(&events);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for env in &decoded[1..] {
        match env {
            Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => {
                assert_eq!(ref_id, run_id, "all envelopes must reference the run");
            }
            _ => {}
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 3. Final envelope: receipt with correct run_id
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn final_envelope_contains_receipt_with_contract_version() {
    let final_env = make_final("run-1");
    let line = JsonlCodec::encode(&final_env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        assert!(matches!(receipt.outcome, Outcome::Complete));
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_ref_id_matches_run() {
    let run_id = "run-xyz";
    let final_env = make_final(run_id);
    if let Envelope::Final { ref_id, .. } = &final_env {
        assert_eq!(ref_id, run_id);
    } else {
        panic!("expected Final");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 4. Fatal handling
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_terminates_stream() {
    let envelopes = vec![
        make_hello(),
        make_event("run-1", AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "out of memory".into(),
        },
    ];
    let buf = encode_stream(&envelopes);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // The last envelope must be fatal.
    assert!(matches!(decoded.last().unwrap(), Envelope::Fatal { .. }));
    // No Final should follow.
    assert!(!decoded.iter().any(|e| matches!(e, Envelope::Final { .. })));
}

#[test]
fn fatal_without_ref_id_is_valid() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "startup crash".into(),
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { ref_id, error } = decoded {
        assert!(ref_id.is_none());
        assert_eq!(error, "startup crash");
    } else {
        panic!("expected Fatal");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 5. Malformed JSONL handling
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn malformed_invalid_json_is_error() {
    let result = JsonlCodec::decode("{not valid json at all}");
    assert!(result.is_err());
}

#[test]
fn malformed_empty_lines_skipped_in_stream() {
    let hello_line = JsonlCodec::encode(&make_hello()).unwrap();
    let final_line = JsonlCodec::encode(&make_final("run-1")).unwrap();
    let input = format!("\n\n{hello_line}\n  \n\n{final_line}\n\n");

    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 2);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Final { .. }));
}

#[test]
fn malformed_line_in_stream_yields_error_item() {
    let hello_line = JsonlCodec::encode(&make_hello()).unwrap();
    let bad_line = "THIS IS NOT JSON\n";
    let final_line = JsonlCodec::encode(&make_final("run-1")).unwrap();
    let input = format!("{hello_line}{bad_line}{final_line}");

    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err(), "bad JSON line must produce an error");
    assert!(results[2].is_ok());
}

#[test]
fn malformed_oversized_line_still_parses() {
    // A 2MB error payload should still round-trip correctly.
    let big_error = "X".repeat(2 * 1024 * 1024);
    let fatal = Envelope::Fatal {
        ref_id: Some("run-big".into()),
        error: big_error.clone(),
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error.len(), 2 * 1024 * 1024);
    } else {
        panic!("expected Fatal");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 6. Encoding edge cases: UTF-8, emoji, special characters
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn encoding_utf8_emoji_in_task() {
    let wo = make_work_order("Fix the 🐛 bug in módule café");
    let env = Envelope::Run {
        id: "run-emoji".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert!(work_order.task.contains('🐛'));
        assert!(work_order.task.contains("café"));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn encoding_cjk_characters_roundtrip() {
    let event = make_event(
        "run-cjk",
        AgentEventKind::AssistantMessage {
            text: "你好世界 こんにちは 한국어".into(),
        },
    );
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert!(text.contains("你好世界"));
            assert!(text.contains("こんにちは"));
            assert!(text.contains("한국어"));
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn encoding_special_json_characters() {
    // Backslashes, quotes, newlines, tabs within string values.
    let text = "line1\nline2\ttab\\backslash\"quote";
    let event = make_event(
        "run-special",
        AgentEventKind::AssistantMessage { text: text.into() },
    );
    let line = JsonlCodec::encode(&event).unwrap();
    // JSONL must be a single line — embedded newlines must be escaped.
    assert_eq!(line.trim_end().matches('\n').count(), 0);
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text: decoded_text } = event.kind {
            assert_eq!(decoded_text, text);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn encoding_null_byte_in_task_text() {
    // Null bytes in JSON strings are escaped as \u0000.
    let task = "before\0after";
    let wo = make_work_order(task);
    let env = Envelope::Run {
        id: "run-null".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task, task);
    } else {
        panic!("expected Run");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 7. Timing: rapid-fire events, zero events before final
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn rapid_fire_many_events() {
    let run_id = "run-rapid";
    let mut envelopes = vec![make_hello()];
    for i in 0..500 {
        envelopes.push(make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            },
        ));
    }
    envelopes.push(make_final(run_id));

    let buf = encode_stream(&envelopes);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 502); // 1 hello + 500 events + 1 final
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded.last().unwrap(), Envelope::Final { .. }));
}

#[test]
fn zero_events_before_final() {
    let envelopes = vec![make_hello(), make_final("run-empty")];
    let buf = encode_stream(&envelopes);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 2);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Final { .. }));
}

// ═════════════════════════════════════════════════════════════════════════
// 8. Multiple runs: sequential runs maintain isolation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn sequential_runs_have_distinct_ref_ids() {
    let run1_id = "run-first";
    let run2_id = "run-second";

    let run1_events = vec![
        make_event(run1_id, AgentEventKind::RunStarted {
            message: "first".into(),
        }),
        make_event(run1_id, AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        make_final(run1_id),
    ];
    let run2_events = vec![
        make_event(run2_id, AgentEventKind::RunStarted {
            message: "second".into(),
        }),
        make_event(run2_id, AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        make_final(run2_id),
    ];

    // Validate no cross-contamination of ref_ids.
    for env in &run1_events {
        match env {
            Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => {
                assert_eq!(ref_id, run1_id);
                assert_ne!(ref_id, run2_id);
            }
            _ => {}
        }
    }
    for env in &run2_events {
        match env {
            Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => {
                assert_eq!(ref_id, run2_id);
                assert_ne!(ref_id, run1_id);
            }
            _ => {}
        }
    }
}

#[test]
fn sequential_runs_receipts_have_unique_run_ids() {
    let final1 = make_final("run-a");
    let final2 = make_final("run-b");

    let (r1, r2) = match (&final1, &final2) {
        (
            Envelope::Final { receipt: r1, .. },
            Envelope::Final { receipt: r2, .. },
        ) => (r1, r2),
        _ => panic!("expected Final"),
    };

    assert_ne!(r1.meta.run_id, r2.meta.run_id, "each run gets a unique run_id");
}

// ═════════════════════════════════════════════════════════════════════════
// 9. Large payloads
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn large_event_content_roundtrip() {
    let large_text = "A".repeat(1_000_000); // 1 MB
    let event = make_event(
        "run-large",
        AgentEventKind::AssistantMessage {
            text: large_text.clone(),
        },
    );
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text.len(), 1_000_000);
            assert_eq!(text, large_text);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_tool_call_input_roundtrip() {
    let big_input = serde_json::json!({
        "code": "x".repeat(500_000),
        "metadata": { "lines": 10000 },
    });
    let event = make_event(
        "run-tool",
        AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu-big".into()),
            parent_tool_use_id: None,
            input: big_input.clone(),
        },
    );
    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall { input, .. } = event.kind {
            assert_eq!(input["code"].as_str().unwrap().len(), 500_000);
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 10. Envelope ordering: events arrive before final
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_ordering_events_before_final() {
    let run_id = "run-order";
    let envelopes = vec![
        make_hello(),
        make_event(run_id, AgentEventKind::RunStarted {
            message: "begin".into(),
        }),
        make_event(run_id, AgentEventKind::AssistantMessage {
            text: "working...".into(),
        }),
        make_event(run_id, AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "refactored".into(),
        }),
        make_event(run_id, AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
        make_final(run_id),
    ];

    let buf = encode_stream(&envelopes);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Find the index of the Final envelope.
    let final_idx = decoded
        .iter()
        .position(|e| matches!(e, Envelope::Final { .. }))
        .expect("must have a Final envelope");

    // All Event envelopes must appear before the Final.
    for (i, env) in decoded.iter().enumerate() {
        if matches!(env, Envelope::Event { .. }) {
            assert!(i < final_idx, "Event at index {i} must precede Final at {final_idx}");
        }
    }
}

#[test]
fn envelope_ordering_hello_precedes_everything() {
    let run_id = "run-order2";
    let envelopes = vec![
        make_hello(),
        make_event(run_id, AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        make_final(run_id),
    ];

    let buf = encode_stream(&envelopes);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let hello_idx = decoded
        .iter()
        .position(|e| matches!(e, Envelope::Hello { .. }))
        .expect("must have Hello");
    assert_eq!(hello_idx, 0, "Hello must be at index 0");
}

// ═════════════════════════════════════════════════════════════════════════
// 11. Version negotiation in hello
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn hello_wrong_contract_version_is_detectable() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v99.0",
        "backend": { "id": "future-sidecar", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    let env: Envelope = serde_json::from_value(raw).unwrap();
    if let Envelope::Hello { contract_version, .. } = env {
        assert_ne!(contract_version, CONTRACT_VERSION);
        // Host should reject or warn about version mismatch.
        assert!(!abp_protocol::is_compatible_version(&contract_version, CONTRACT_VERSION));
    } else {
        panic!("expected Hello");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 12. Full protocol session simulation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn full_session_hello_events_final() {
    let run_id = "run-full-session";
    let envelopes = vec![
        make_hello(),
        make_event(run_id, AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
        make_event(run_id, AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        }),
        make_event(run_id, AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!("fn main() {}"),
            is_error: false,
        }),
        make_event(run_id, AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added logging".into(),
        }),
        make_event(run_id, AgentEventKind::RunCompleted {
            message: "all done".into(),
        }),
        make_final(run_id),
    ];

    let buf = encode_stream(&envelopes);
    let decoded: Vec<_> = decode_all(&buf)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 7);

    // Verify structure: Hello, 5 events, Final
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    for env in &decoded[1..6] {
        assert!(matches!(env, Envelope::Event { .. }));
    }
    assert!(matches!(decoded[6], Envelope::Final { .. }));

    // Verify all ref_ids match
    for env in &decoded[1..] {
        match env {
            Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => {
                assert_eq!(ref_id, run_id);
            }
            _ => panic!("unexpected envelope type in session body"),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 13. Discriminator tag field
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn discriminator_tag_is_t_not_type() {
    let hello = make_hello();
    let json = serde_json::to_value(&hello).unwrap();
    assert!(json.get("t").is_some(), "envelope must use 't' as discriminator");
    assert!(json.get("type").is_none(), "envelope must NOT use 'type'");
}

// ═════════════════════════════════════════════════════════════════════════
// 14. Extension field passthrough
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn event_ext_field_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"vendor": "data"}));

    let event = Envelope::Event {
        ref_id: "run-ext".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "with ext".into(),
            },
            ext: Some(ext.clone()),
        },
    };

    let line = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(event.ext.is_some());
        let decoded_ext = event.ext.unwrap();
        assert_eq!(decoded_ext["raw_message"], serde_json::json!({"vendor": "data"}));
    } else {
        panic!("expected Event");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 15. Receipt hashing in final envelope
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn final_receipt_hash_is_deterministic() {
    let now = Utc::now();
    let run_id = Uuid::nil();
    let make_receipt = || Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let hash1 = receipt_hash(&make_receipt()).unwrap();
    let hash2 = receipt_hash(&make_receipt()).unwrap();
    assert_eq!(hash1, hash2, "same receipt must produce same hash");
    assert_eq!(hash1.len(), 64, "SHA-256 hex digest is 64 chars");
}

// ═════════════════════════════════════════════════════════════════════════
// 16. Mixed valid/invalid stream resilience
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn stream_with_interleaved_garbage_lines() {
    let hello_line = JsonlCodec::encode(&make_hello()).unwrap();
    let event_line = JsonlCodec::encode(&make_event(
        "run-1",
        AgentEventKind::AssistantMessage { text: "ok".into() },
    ))
    .unwrap();
    let final_line = JsonlCodec::encode(&make_final("run-1")).unwrap();

    let input = format!(
        "{hello_line}GARBAGE LINE 1\n{event_line}{{\"incomplete\n{final_line}"
    );

    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    // Should have 5 items: hello(ok), garbage(err), event(ok), incomplete(err), final(ok)
    assert_eq!(results.len(), 5);

    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    let err_count = results.iter().filter(|r| r.is_err()).count();
    assert_eq!(ok_count, 3);
    assert_eq!(err_count, 2);
}
