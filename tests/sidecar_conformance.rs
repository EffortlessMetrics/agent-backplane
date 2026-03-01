// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar protocol conformance test suite.
//!
//! Validates that JSONL protocol streams conform to the ABP sidecar contract:
//! handshake, event correlation, envelope ordering, error handling, and encoding.

use abp_core::*;
use abp_protocol::validate::EnvelopeValidator;
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
    let event = make_event(
        "run-1",
        AgentEventKind::RunStarted {
            message: "oops".into(),
        },
    );
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
    let event = make_event(
        run_id,
        AgentEventKind::AssistantMessage { text: "hi".into() },
    );
    if let Envelope::Event { ref_id, .. } = &event {
        assert_eq!(ref_id, run_id);
    } else {
        panic!("expected Event");
    }

    // A mismatched ref_id is structurally valid but semantically wrong.
    let wrong = make_event(
        "run-OTHER",
        AgentEventKind::AssistantMessage { text: "hi".into() },
    );
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
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "chunk1".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "chunk2".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
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
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
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
        make_event(
            run1_id,
            AgentEventKind::RunStarted {
                message: "first".into(),
            },
        ),
        make_event(
            run1_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        make_final(run1_id),
    ];
    let run2_events = vec![
        make_event(
            run2_id,
            AgentEventKind::RunStarted {
                message: "second".into(),
            },
        ),
        make_event(
            run2_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
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
        (Envelope::Final { receipt: r1, .. }, Envelope::Final { receipt: r2, .. }) => (r1, r2),
        _ => panic!("expected Final"),
    };

    assert_ne!(
        r1.meta.run_id, r2.meta.run_id,
        "each run gets a unique run_id"
    );
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
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "begin".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantMessage {
                text: "working...".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "refactored".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
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
            assert!(
                i < final_idx,
                "Event at index {i} must precede Final at {final_idx}"
            );
        }
    }
}

#[test]
fn envelope_ordering_hello_precedes_everything() {
    let run_id = "run-order2";
    let envelopes = vec![
        make_hello(),
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
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
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_ne!(contract_version, CONTRACT_VERSION);
        // Host should reject or warn about version mismatch.
        assert!(!abp_protocol::is_compatible_version(
            &contract_version,
            CONTRACT_VERSION
        ));
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
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "starting".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!("fn main() {}"),
                is_error: false,
            },
        ),
        make_event(
            run_id,
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added logging".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::RunCompleted {
                message: "all done".into(),
            },
        ),
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
    assert!(
        json.get("t").is_some(),
        "envelope must use 't' as discriminator"
    );
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
        assert_eq!(
            decoded_ext["raw_message"],
            serde_json::json!({"vendor": "data"})
        );
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

    let input = format!("{hello_line}GARBAGE LINE 1\n{event_line}{{\"incomplete\n{final_line}");

    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();

    // Should have 5 items: hello(ok), garbage(err), event(ok), incomplete(err), final(ok)
    assert_eq!(results.len(), 5);

    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    let err_count = results.iter().filter(|r| r.is_err()).count();
    assert_eq!(ok_count, 3);
    assert_eq!(err_count, 2);
}

// ═════════════════════════════════════════════════════════════════════════
// 17. Protocol Basics — Run envelope format
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn run_envelope_format_roundtrips() {
    let wo = make_work_order("conformance task");
    let run = Envelope::Run {
        id: "run-fmt".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    assert!(line.ends_with('\n'));

    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-fmt");
        assert_eq!(work_order.task, "conformance task");
    } else {
        panic!("expected Run, got {decoded:?}");
    }
}

#[test]
fn run_envelope_discriminator_is_t() {
    let wo = make_work_order("check tag");
    let run = Envelope::Run {
        id: "run-tag".into(),
        work_order: wo,
    };
    let json = serde_json::to_value(&run).unwrap();
    assert_eq!(json["t"].as_str(), Some("run"));
    assert!(json.get("type").is_none());
}

#[test]
fn run_envelope_preserves_work_order_fields() {
    let wo = WorkOrderBuilder::new("full fields")
        .root("/tmp/ws")
        .model("gpt-4")
        .max_turns(5)
        .build();
    let run = Envelope::Run {
        id: "run-fields".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.workspace.root, "/tmp/ws");
        assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(work_order.config.max_turns, Some(5));
    } else {
        panic!("expected Run");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 18. Envelope Validation — Missing required fields
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn missing_discriminator_tag_is_parse_error() {
    let raw = r#"{"contract_version":"abp/v0.1","backend":{"id":"x"},"capabilities":{}}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "missing 't' field must fail");
}

#[test]
fn missing_hello_backend_field_is_parse_error() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "missing backend field must fail");
}

#[test]
fn missing_event_ref_id_is_parse_error() {
    let raw = r#"{"t":"event","event":{"ts":"2024-01-01T00:00:00Z","type":"run_started","message":"go"}}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "missing ref_id on event must fail");
}

#[test]
fn missing_final_receipt_is_parse_error() {
    let raw = r#"{"t":"final","ref_id":"run-1"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "missing receipt on final must fail");
}

#[test]
fn missing_fatal_error_is_parse_error() {
    let raw = r#"{"t":"fatal","ref_id":"run-1"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "missing error on fatal must fail");
}

#[test]
fn missing_run_work_order_is_parse_error() {
    let raw = r#"{"t":"run","id":"run-1"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "missing work_order on run must fail");
}

// ═════════════════════════════════════════════════════════════════════════
// 19. Envelope Validation — Extra unknown fields tolerated
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn hello_with_extra_fields_is_tolerated() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "test", "backend_version": null, "adapter_version": null },
        "capabilities": {},
        "extra_field": "should be ignored",
        "another": 42
    });
    let env: Result<Envelope, _> = serde_json::from_value(raw);
    assert!(
        env.is_ok(),
        "extra unknown fields should be tolerated on hello"
    );
    assert!(matches!(env.unwrap(), Envelope::Hello { .. }));
}

#[test]
fn event_with_extra_fields_is_tolerated() {
    let raw = serde_json::json!({
        "t": "event",
        "ref_id": "run-1",
        "event": {
            "ts": "2024-01-01T00:00:00Z",
            "type": "assistant_message",
            "text": "hi",
        },
        "unknown_key": true
    });
    let env: Result<Envelope, _> = serde_json::from_value(raw);
    assert!(
        env.is_ok(),
        "extra unknown fields should be tolerated on event"
    );
}

#[test]
fn fatal_with_extra_fields_is_tolerated() {
    let raw = serde_json::json!({
        "t": "fatal",
        "ref_id": "run-1",
        "error": "boom",
        "debug_info": { "stack": "..." }
    });
    let env: Result<Envelope, _> = serde_json::from_value(raw);
    assert!(
        env.is_ok(),
        "extra unknown fields should be tolerated on fatal"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 20. Envelope Validation — Empty envelope body
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn empty_json_object_is_parse_error() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err(), "empty JSON object without 't' must fail");
}

#[test]
fn unknown_envelope_type_is_parse_error() {
    let raw = r#"{"t":"unknown_type","data":"foo"}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "unknown envelope type must fail");
}

#[test]
fn empty_string_is_skipped_in_stream() {
    let reader = BufReader::new("  \n\n  \n".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(results.is_empty(), "blank/whitespace lines must be skipped");
}

// ═════════════════════════════════════════════════════════════════════════
// 21. Handshake — Wrong first message type (all variants)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn first_message_is_run_not_hello() {
    let wo = make_work_order("should fail");
    let run = Envelope::Run {
        id: "run-nohello".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(
        !matches!(decoded, Envelope::Hello { .. }),
        "Run as first message must not be interpreted as Hello"
    );
}

#[test]
fn first_message_is_final_not_hello() {
    let final_env = make_final("run-nohello");
    let line = JsonlCodec::encode(&final_env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(
        !matches!(decoded, Envelope::Hello { .. }),
        "Final as first message must not be interpreted as Hello"
    );
}

#[test]
fn first_message_is_fatal_not_hello() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "init error".into(),
    };
    let line = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(
        !matches!(decoded, Envelope::Hello { .. }),
        "Fatal as first message must not be interpreted as Hello"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 22. EnvelopeValidator — single envelope validation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn validator_hello_valid() {
    let v = EnvelopeValidator::new();
    let result = v.validate(&make_hello());
    assert!(result.valid, "valid hello should pass: {:?}", result.errors);
}

#[test]
fn validator_hello_empty_backend_id_fails() {
    let v = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = v.validate(&hello);
    assert!(!result.valid, "empty backend.id should fail validation");
}

#[test]
fn validator_hello_invalid_version_fails() {
    let v = EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_backend(),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = v.validate(&hello);
    assert!(!result.valid, "unparseable contract_version should fail");
}

#[test]
fn validator_event_empty_ref_id_fails() {
    let v = EnvelopeValidator::new();
    let event = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
    };
    let result = v.validate(&event);
    assert!(!result.valid, "empty ref_id on event should fail");
}

#[test]
fn validator_final_empty_ref_id_fails() {
    let v = EnvelopeValidator::new();
    let final_env = Envelope::Final {
        ref_id: String::new(),
        receipt: ReceiptBuilder::new("test").build(),
    };
    let result = v.validate(&final_env);
    assert!(!result.valid, "empty ref_id on final should fail");
}

#[test]
fn validator_fatal_empty_error_fails() {
    let v = EnvelopeValidator::new();
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: String::new(),
    };
    let result = v.validate(&fatal);
    assert!(!result.valid, "empty error on fatal should fail");
}

#[test]
fn validator_run_empty_id_fails() {
    let v = EnvelopeValidator::new();
    let run = Envelope::Run {
        id: String::new(),
        work_order: make_work_order("test"),
    };
    let result = v.validate(&run);
    assert!(!result.valid, "empty id on run should fail");
}

#[test]
fn validator_run_empty_task_fails() {
    let v = EnvelopeValidator::new();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: make_work_order(""),
    };
    let result = v.validate(&run);
    assert!(!result.valid, "empty task on run should fail");
}

// ═════════════════════════════════════════════════════════════════════════
// 23. EnvelopeValidator — sequence validation
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn validator_sequence_valid_hello_run_events_final() {
    let v = EnvelopeValidator::new();
    let run_id = "run-seq";
    let seq = vec![
        make_hello(),
        Envelope::Run {
            id: run_id.into(),
            work_order: make_work_order("test"),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event(
            run_id,
            AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
        ),
        make_final(run_id),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors: {errors:?}"
    );
}

#[test]
fn validator_sequence_missing_hello() {
    let v = EnvelopeValidator::new();
    let run_id = "run-nohello";
    let seq = vec![
        Envelope::Run {
            id: run_id.into(),
            work_order: make_work_order("test"),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_final(run_id),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, abp_protocol::validate::SequenceError::MissingHello)),
        "should detect missing hello: {errors:?}"
    );
}

#[test]
fn validator_sequence_missing_terminal() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        Envelope::Run {
            id: "run-noterm".into(),
            work_order: make_work_order("test"),
        },
        make_event(
            "run-noterm",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, abp_protocol::validate::SequenceError::MissingTerminal)),
        "should detect missing terminal: {errors:?}"
    );
}

#[test]
fn validator_sequence_ref_id_mismatch() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        Envelope::Run {
            id: "run-a".into(),
            work_order: make_work_order("test"),
        },
        make_event(
            "run-b",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_final("run-a"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            abp_protocol::validate::SequenceError::RefIdMismatch { .. }
        )),
        "should detect ref_id mismatch: {errors:?}"
    );
}

#[test]
fn validator_sequence_fatal_ending_is_valid() {
    let v = EnvelopeValidator::new();
    let run_id = "run-fatal";
    let seq = vec![
        make_hello(),
        Envelope::Run {
            id: run_id.into(),
            work_order: make_work_order("test"),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Fatal {
            ref_id: Some(run_id.into()),
            error: "crash".into(),
        },
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors.is_empty(),
        "fatal ending should be valid: {errors:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 24. Edge Cases — Large work order
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn large_work_order_roundtrips() {
    let large_task = "X".repeat(500_000);
    let mut wo = make_work_order(&large_task);
    // Add many context files
    wo.context.files = (0..1000).map(|i| format!("src/file_{i}.rs")).collect();
    // Add snippets
    wo.context.snippets = vec![ContextSnippet {
        name: "big".into(),
        content: "Y".repeat(100_000),
    }];

    let run = Envelope::Run {
        id: "run-large-wo".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task.len(), 500_000);
        assert_eq!(work_order.context.files.len(), 1000);
        assert_eq!(work_order.context.snippets[0].content.len(), 100_000);
    } else {
        panic!("expected Run");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 25. Edge Cases — Empty trace in final receipt
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn final_receipt_with_empty_trace_roundtrips() {
    let receipt = ReceiptBuilder::new("test-empty-trace").build();
    assert!(receipt.trace.is_empty());

    let final_env = Envelope::Final {
        ref_id: "run-empty-trace".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&final_env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert!(
            receipt.trace.is_empty(),
            "empty trace must survive round-trip"
        );
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_empty_trace_hashes_correctly() {
    let receipt = ReceiptBuilder::new("test-hash")
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.trace.is_empty());
    assert!(receipt.receipt_sha256.is_some());
    // Verify the hash is valid by recomputing
    let hash = receipt_hash(&receipt).unwrap();
    assert_eq!(receipt.receipt_sha256.as_deref(), Some(hash.as_str()));
}

// ═════════════════════════════════════════════════════════════════════════
// 26. Edge Cases — Wrong ref_id on final
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn wrong_ref_id_on_final_is_structurally_valid_but_detectable() {
    let run_id = "run-correct";
    let final_env = make_final("run-WRONG");
    if let Envelope::Final { ref_id, .. } = &final_env {
        assert_ne!(
            ref_id, run_id,
            "wrong ref_id must be detectable by comparison"
        );
    } else {
        panic!("expected Final");
    }
}

#[test]
fn wrong_ref_id_detected_by_validator_sequence() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        make_hello(),
        Envelope::Run {
            id: "run-correct".into(),
            work_order: make_work_order("test"),
        },
        make_event(
            "run-correct",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_final("run-WRONG"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            abp_protocol::validate::SequenceError::RefIdMismatch { .. }
        )),
        "validator should catch final with wrong ref_id: {errors:?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════
// 27. Edge Cases — All AgentEventKind variants in protocol
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn all_agent_event_kinds_roundtrip_through_protocol() {
    let run_id = "run-all-kinds";
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "file.rs"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!("content"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "edited".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        AgentEventKind::Warning {
            message: "heads up".into(),
        },
        AgentEventKind::Error {
            message: "bad thing".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ];

    for kind in kinds {
        let event = make_event(run_id, kind);
        let line = JsonlCodec::encode(&event).unwrap();
        let decoded = JsonlCodec::decode(line.trim());
        assert!(
            decoded.is_ok(),
            "event kind must round-trip: {}",
            line.trim()
        );
        assert!(matches!(decoded.unwrap(), Envelope::Event { .. }));
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 28. Edge Cases — Passthrough execution mode
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn hello_passthrough_mode_roundtrips() {
    let hello = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_missing_mode_defaults_to_mapped() {
    // When "mode" is absent from JSON, serde default should give Mapped.
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "test", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    let env: Envelope = serde_json::from_value(raw).unwrap();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(
            mode,
            ExecutionMode::Mapped,
            "missing mode defaults to Mapped"
        );
    } else {
        panic!("expected Hello");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 29. Version negotiation helpers
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn compatible_versions_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(abp_protocol::is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!abp_protocol::is_compatible_version("abp/v2.0", "abp/v0.1"));
}

#[test]
fn invalid_version_strings_are_incompatible() {
    assert!(!abp_protocol::is_compatible_version("garbage", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "garbage"));
    assert!(!abp_protocol::is_compatible_version("", ""));
}

#[test]
fn parse_version_edge_cases() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v10.20"), Some((10, 20)));
    assert!(abp_protocol::parse_version("v0.1").is_none());
    assert!(abp_protocol::parse_version("abp/0.1").is_none());
    assert!(abp_protocol::parse_version("abp/v").is_none());
    assert!(abp_protocol::parse_version("").is_none());
}
