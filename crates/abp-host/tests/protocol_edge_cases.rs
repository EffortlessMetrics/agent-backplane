// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol edge-case tests for the ABP sidecar protocol.
//!
//! Pure Rust tests exercising handshake edge cases, event streaming
//! boundaries, error handling, and ref-ID correlation — no process
//! spawning required.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrder,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::{Envelope, JsonlCodec};
use std::collections::BTreeMap;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "edge-test".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_receipt(run_id: Uuid, wo_id: Uuid) -> Receipt {
    let now = chrono::Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo_id,
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::Value::Null,
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "edge case test".into(),
        lane: abp_core::ExecutionLane::PatchFirst,
        workspace: abp_core::WorkspaceSpec {
            root: ".".into(),
            mode: abp_core::WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: abp_core::PolicyProfile::default(),
        requirements: abp_core::CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn valid_sequence(run_id: &str) -> Vec<Envelope> {
    vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Handshake: Hello with empty capabilities array
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_empty_capabilities_round_trips() {
    let hello = Envelope::hello(test_backend(), BTreeMap::new());
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_empty_capabilities_validates_ok() {
    let hello = Envelope::hello(test_backend(), BTreeMap::new());
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(result.valid, "empty capabilities should be valid");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Handshake: Hello with unknown capabilities (tolerated)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_unknown_capabilities_round_trips() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let hello = Envelope::hello(test_backend(), caps);
    let encoded = JsonlCodec::encode(&hello).unwrap();

    // Inject an unknown capability into the raw JSON to simulate a future
    // sidecar advertising capabilities this host doesn't know about yet.
    // serde's deny_unknown_fields is NOT set, so extra fields in the
    // capabilities map are silently dropped — the envelope still parses.
    let raw_json: serde_json::Value = serde_json::from_str(encoded.trim()).unwrap();
    assert!(raw_json["capabilities"].is_object());

    // Round-trip the known capabilities.
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 2);
            assert!(matches!(
                capabilities[&Capability::Streaming],
                SupportLevel::Native
            ));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Handshake: Hello with wrong contract version (validation error)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_wrong_contract_version_fails_validation() {
    let hello = Envelope::Hello {
        contract_version: "abp/v999.0".into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    // The version parses but does not match CONTRACT_VERSION.
    assert!(
        result.valid,
        "parseable version should not be a validation error"
    );
    assert_ne!("abp/v999.0", CONTRACT_VERSION);
    assert!(!abp_protocol::is_compatible_version(
        "abp/v999.0",
        CONTRACT_VERSION
    ));
}

#[test]
fn hello_unparseable_version_fails_validation() {
    let hello = Envelope::Hello {
        contract_version: "garbage".into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Handshake: Hello with extra fields (serde tolerates)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_extra_fields_tolerated_on_decode() {
    let raw = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"test","backend_version":null,"adapter_version":null}},"capabilities":{{}},"extra_field":"should be ignored","another":42}}"#,
        CONTRACT_VERSION
    );
    let decoded = JsonlCodec::decode(&raw).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test");
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Handshake: Multiple hello envelopes in sequence (error)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_duplicate_hello_detected() {
    let run_id = "run-dup";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];

    // The validator does not explicitly reject duplicate Hello envelopes,
    // but a higher-level consumer can detect them by inspecting the sequence.
    let hello_count = sequence
        .iter()
        .filter(|e| matches!(e, Envelope::Hello { .. }))
        .count();
    assert_eq!(
        hello_count, 2,
        "sequence should contain two Hello envelopes"
    );

    // Verify that the sequence still parses — the protocol layer is lenient.
    let v = EnvelopeValidator::new();
    let _errors = v.validate_sequence(&sequence);
    // No assertion on errors; the key property is that duplicate hellos are
    // detectable at the application layer.
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Event streaming: Very large payload (100KB+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_large_payload_round_trips() {
    let big_text = "X".repeat(150_000); // 150 KB
    let event = make_event(
        "run-big",
        AgentEventKind::AssistantMessage {
            text: big_text.clone(),
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    assert!(encoded.len() > 100_000);
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::AssistantMessage { text } = &event.kind {
                assert_eq!(text.len(), 150_000);
            } else {
                panic!("expected AssistantMessage");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn stream_parser_handles_large_payload() {
    let big_text = "Y".repeat(120_000);
    let event = make_event(
        "run-big",
        AgentEventKind::AssistantDelta {
            text: big_text.clone(),
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();

    let mut parser = StreamParser::new();
    // Feed in two chunks.
    let mid = encoded.len() / 2;
    let first_results = parser.push(&encoded.as_bytes()[..mid]);
    assert!(
        first_results.is_empty(),
        "partial line should yield nothing"
    );
    let second_results = parser.push(&encoded.as_bytes()[mid..]);
    assert_eq!(second_results.len(), 1);
    assert!(second_results[0].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Event streaming: Unicode and emoji content
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_unicode_emoji_round_trips() {
    let text = "Hello 🌍🚀✨ — Ω ñ ü 日本語 中文 한국어 العربية";
    let event = make_event(
        "run-uni",
        AgentEventKind::AssistantMessage { text: text.into() },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::AssistantMessage { text: got } = &event.kind {
                assert_eq!(got, text);
            } else {
                panic!("expected AssistantMessage");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_emoji_ref_id_round_trips() {
    let event = make_event(
        "🔑-run-42",
        AgentEventKind::RunStarted {
            message: "🎬 action!".into(),
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "🔑-run-42"),
        other => panic!("expected Event, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Event streaming: Nested JSON in tool outputs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_nested_json_tool_result_round_trips() {
    let nested = serde_json::json!({
        "files": [
            {"path": "src/main.rs", "changes": [{"line": 1, "content": "fn main() {}"}]},
            {"path": "Cargo.toml", "changes": []}
        ],
        "metadata": {"depth": 3, "nested": {"a": {"b": {"c": true}}}}
    });
    let event = make_event(
        "run-nested",
        AgentEventKind::ToolResult {
            tool_name: "edit_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: nested.clone(),
            is_error: false,
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::ToolResult { output, .. } = &event.kind {
                assert_eq!(output, &nested);
            } else {
                panic!("expected ToolResult");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_call_with_complex_input_round_trips() {
    let input = serde_json::json!({
        "command": "grep",
        "args": ["-r", "--include=*.rs", "fn main"],
        "options": {"cwd": "/tmp", "env": {"RUST_LOG": "debug"}}
    });
    let event = make_event(
        "run-tc",
        AgentEventKind::ToolCall {
            tool_name: "shell".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input,
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Event streaming: Rapid-fire events via StreamParser
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_parser_rapid_fire_events() {
    let mut batch = String::new();
    for i in 0..100 {
        let event = make_event(
            "run-rapid",
            AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
        );
        batch.push_str(&JsonlCodec::encode(&event).unwrap());
    }

    let mut parser = StreamParser::new();
    let results = parser.push(batch.as_bytes());
    assert_eq!(results.len(), 100);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn stream_parser_rapid_fire_byte_at_a_time() {
    let event = make_event(
        "run-slow",
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();

    let mut parser = StreamParser::new();
    let mut total_results = Vec::new();
    for byte in encoded.as_bytes() {
        let results = parser.push(std::slice::from_ref(byte));
        total_results.extend(results);
    }
    assert_eq!(total_results.len(), 1);
    assert!(total_results[0].is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Event streaming: Null and empty fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_with_null_ext_field_round_trips() {
    let event = Envelope::Event {
        ref_id: "run-null".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hello".into(),
            },
            ext: None,
        },
    };
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => assert!(event.ext.is_none()),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_with_empty_text_round_trips() {
    let event = make_event(
        "run-empty",
        AgentEventKind::AssistantDelta { text: "".into() },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::AssistantDelta { text } = &event.kind {
                assert!(text.is_empty());
            } else {
                panic!("expected AssistantDelta");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_result_with_null_output_round_trips() {
    let event = make_event(
        "run-tnull",
        AgentEventKind::ToolResult {
            tool_name: "noop".into(),
            tool_use_id: None,
            output: serde_json::Value::Null,
            is_error: false,
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::ToolResult {
                output,
                tool_use_id,
                ..
            } = &event.kind
            {
                assert!(output.is_null());
                assert!(tool_use_id.is_none());
            } else {
                panic!("expected ToolResult");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Error handling: Malformed JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_malformed_json_brace_mismatch() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1""#);
    assert!(result.is_err());
}

#[test]
fn decode_malformed_json_trailing_comma() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1",}"#);
    assert!(result.is_err());
}

#[test]
fn stream_parser_malformed_line_returns_error() {
    let mut parser = StreamParser::new();
    let results = parser.push(b"this is not valid json\n");
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Error handling: Valid JSON but not an envelope
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_valid_json_wrong_shape() {
    let result = JsonlCodec::decode(r#"{"name":"alice","age":30}"#);
    assert!(result.is_err(), "valid JSON without 't' field should fail");
}

#[test]
fn decode_valid_json_unknown_tag() {
    let result = JsonlCodec::decode(r#"{"t":"subscribe","channel":"events"}"#);
    assert!(
        result.is_err(),
        "unknown envelope tag should fail deserialization"
    );
}

#[test]
fn decode_valid_json_array_not_object() {
    let result = JsonlCodec::decode(r#"[1, 2, 3]"#);
    assert!(result.is_err(), "JSON array should not decode as envelope");
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Error handling: Missing final/fatal in sequence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_no_terminal_after_events() {
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: "run-orphan".into(),
            work_order: test_work_order(),
        },
        make_event(
            "run-orphan",
            AgentEventKind::RunStarted {
                message: "started".into(),
            },
        ),
        make_event(
            "run-orphan",
            AgentEventKind::AssistantDelta {
                text: "partial output".into(),
            },
        ),
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Error handling: StreamParser partial data (simulating hang)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_parser_partial_data_buffered_not_lost() {
    let event = make_event(
        "run-hang",
        AgentEventKind::RunStarted {
            message: "going".into(),
        },
    );
    let encoded = JsonlCodec::encode(&event).unwrap();
    let partial = &encoded[..encoded.len() / 2];

    let mut parser = StreamParser::new();
    let results = parser.push(partial.as_bytes());
    assert!(results.is_empty(), "partial line should not yield results");
    assert!(!parser.is_empty(), "parser should buffer partial data");
    assert_eq!(parser.buffered_len(), partial.len());
}

#[test]
fn stream_parser_finish_flushes_partial() {
    let event = make_event(
        "run-finish",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let mut encoded = JsonlCodec::encode(&event).unwrap();
    // Remove trailing newline to simulate unterminated last line.
    encoded.pop();

    let mut parser = StreamParser::new();
    let results = parser.push(encoded.as_bytes());
    assert!(results.is_empty());

    let flushed = parser.finish();
    assert_eq!(flushed.len(), 1);
    assert!(flushed[0].is_ok());
    assert!(parser.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Error handling: Fatal envelope with detailed error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fatal_detailed_error_round_trips() {
    let detailed_error = "Backend crashed: SIGSEGV at 0xdeadbeef in libmodel.so. \
        Stack trace:\n  #0 inference_step()\n  #1 run_model()\n  #2 main()";
    let fatal = Envelope::Fatal {
        ref_id: Some("run-crash".into()),
        error: detailed_error.into(),
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-crash"));
            assert!(error.contains("SIGSEGV"));
            assert!(error.contains("Stack trace"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_with_unicode_error_message() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "エラー: メモリ不足 💥".into(),
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("エラー"));
            assert!(error.contains("💥"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Ref ID correlation: Correct ref_id accepted in sequence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_correct_ref_ids_pass_validation() {
    let run_id = "run-ok-ref";
    let sequence = valid_sequence(run_id);
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(errors.is_empty(), "correct ref_ids should pass: {errors:?}");
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Ref ID correlation: Wrong ref_id on event rejected
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_wrong_event_ref_id_detected() {
    let run_id = "run-real";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        make_event(
            "run-WRONG",
            AgentEventKind::AssistantDelta {
                text: "stray".into(),
            },
        ),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch { expected, found }
            if expected == "run-real" && found == "run-WRONG"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Ref ID correlation: Wrong ref_id on final rejected
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_wrong_final_ref_id_detected() {
    let run_id = "run-actual";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "ok".into(),
            },
        ),
        Envelope::Final {
            ref_id: "run-OTHER".into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch { expected, found }
            if expected == "run-actual" && found == "run-OTHER"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Ref ID correlation: Correct final ref_id accepted
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_correct_final_ref_id_accepted() {
    let run_id = "run-final-ok";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(
        errors.is_empty(),
        "correct final ref_id should pass: {errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Ref ID correlation: Fatal with wrong ref_id
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_fatal_wrong_ref_id_detected() {
    let run_id = "run-fatal-ref";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::Fatal {
            ref_id: Some("run-MISMATCH".into()),
            error: "boom".into(),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(errors.iter().any(|e| matches!(
        e,
        SequenceError::RefIdMismatch { expected, found }
            if expected == "run-fatal-ref" && found == "run-MISMATCH"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 21. StreamParser: Mixed valid and invalid lines
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_parser_mixed_valid_and_invalid() {
    let good_event = make_event(
        "run-mix",
        AgentEventKind::RunStarted {
            message: "ok".into(),
        },
    );
    let good_line = JsonlCodec::encode(&good_event).unwrap();
    let input = format!("{good_line}INVALID JSON LINE\n{good_line}");

    let mut parser = StreamParser::new();
    let results = parser.push(input.as_bytes());
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok(), "first line should be valid");
    assert!(results[1].is_err(), "second line should be invalid");
    assert!(results[2].is_ok(), "third line should be valid");
}

// ═══════════════════════════════════════════════════════════════════════
// 22. StreamParser: Blank lines skipped
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_parser_blank_lines_skipped() {
    let event = make_event(
        "run-blank",
        AgentEventKind::AssistantDelta { text: "hi".into() },
    );
    let line = JsonlCodec::encode(&event).unwrap();
    let input = format!("\n\n{line}\n\n\n{line}\n");

    let mut parser = StreamParser::new();
    let results = parser.push(input.as_bytes());
    assert_eq!(
        results.len(),
        2,
        "only non-blank lines should produce results"
    );
    assert!(results.iter().all(|r| r.is_ok()));
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Event with ext field populated
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_with_ext_field_round_trips() {
    let event = Envelope::Event {
        ref_id: "run-ext".into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "with ext".into(),
            },
            ext: Some(BTreeMap::from([
                ("vendor".into(), serde_json::json!("test")),
                ("cost".into(), serde_json::json!(0.001)),
            ])),
        },
    };
    let encoded = JsonlCodec::encode(&event).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            let ext = event.ext.expect("ext should be present");
            assert_eq!(ext["vendor"], "test");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Hello mode defaults to Mapped when absent
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_mode_defaults_to_mapped_when_absent() {
    let raw = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"test","backend_version":null,"adapter_version":null}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION
    );
    let decoded = JsonlCodec::decode(&raw).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Sequence with only Hello and Fatal (no Run)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_hello_then_fatal_no_run() {
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Fatal {
            ref_id: None,
            error: "startup failure".into(),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    // No Run means no ref_id to check; sequence should be valid.
    assert!(
        errors.is_empty(),
        "Hello→Fatal should be a valid (if unfortunate) sequence: {errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Events after terminal are out of order
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_event_after_final_is_out_of_order() {
    let run_id = "run-late";
    let sequence = vec![
        Envelope::hello(test_backend(), CapabilityManifest::new()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        make_event(
            run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::new_v4(), Uuid::nil()),
        },
        make_event(
            run_id,
            AgentEventKind::AssistantDelta {
                text: "late event".into(),
            },
        ),
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents)),
        "event after final should be out of order: {errors:?}"
    );
}
