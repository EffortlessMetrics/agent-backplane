// SPDX-License-Identifier: MIT OR Apache-2.0
//! Golden file tests for the sidecar JSONL protocol.
//!
//! Validates serialization, deserialization, roundtrip fidelity, edge cases,
//! multi-line JSONL parsing, error handling, and tolerance for field ordering,
//! unicode content, and large payloads.

use std::collections::BTreeMap;
use std::io::BufReader;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn uuid1() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn sample_caps() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps
}

fn sample_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uuid1(),
            work_order_id: uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: ts(),
            finished_at: ts(),
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "sidecar:test".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: sample_caps(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 100, "output_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.001),
        },
        trace: vec![AgentEvent {
            ts: ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "file".into(),
            path: "output.txt".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file.txt".into()),
            git_status: Some("M file.txt".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. Serialize each envelope variant — verify exact JSON via insta snapshots
// ===========================================================================

#[test]
fn golden_serialize_hello() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: Some("3.5.0".into()),
            adapter_version: None,
        },
        sample_caps(),
    );
    insta::assert_json_snapshot!("golden_hello", serde_json::to_value(&env).unwrap());
}

#[test]
fn golden_serialize_hello_passthrough() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
        ExecutionMode::Passthrough,
    );
    insta::assert_json_snapshot!(
        "golden_hello_passthrough",
        serde_json::to_value(&env).unwrap()
    );
}

#[test]
fn golden_serialize_run() {
    let wo = WorkOrderBuilder::new("Write a unit test").build();
    let env = Envelope::Run {
        id: uuid1().to_string(),
        work_order: wo,
    };
    insta::assert_json_snapshot!("golden_run", serde_json::to_value(&env).unwrap(), {
        ".work_order.id" => "[uuid]"
    });
}

#[test]
fn golden_serialize_event_delta() {
    let env = Envelope::Event {
        ref_id: uuid1().to_string(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello, world!".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!("golden_event_delta", serde_json::to_value(&env).unwrap());
}

#[test]
fn golden_serialize_event_tool_call() {
    let env = Envelope::Event {
        ref_id: uuid1().to_string(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("tu_001".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs", "content": "fn main() {}"}),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(
        "golden_event_tool_call",
        serde_json::to_value(&env).unwrap()
    );
}

#[test]
fn golden_serialize_event_tool_result() {
    let env = Envelope::Event {
        ref_id: uuid1().to_string(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_001".into()),
                output: json!("fn main() {}"),
                is_error: false,
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(
        "golden_event_tool_result",
        serde_json::to_value(&env).unwrap()
    );
}

#[test]
fn golden_serialize_final() {
    let env = Envelope::Final {
        ref_id: uuid1().to_string(),
        receipt: sample_receipt(),
    };
    insta::assert_json_snapshot!("golden_final", serde_json::to_value(&env).unwrap());
}

#[test]
fn golden_serialize_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some(uuid1().to_string()),
        error: "out of memory".into(),
    };
    insta::assert_json_snapshot!("golden_fatal_with_ref", serde_json::to_value(&env).unwrap());
}

#[test]
fn golden_serialize_fatal_without_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "missing API key".into(),
    };
    insta::assert_json_snapshot!(
        "golden_fatal_without_ref",
        serde_json::to_value(&env).unwrap()
    );
}

// ===========================================================================
// 2. Deserialize known golden JSONL strings and verify fields
// ===========================================================================

#[test]
fn golden_deserialize_hello() {
    let line = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "test");
            assert!(backend.backend_version.is_none());
            assert!(capabilities.is_empty());
            assert!(matches!(mode, ExecutionMode::Mapped));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn golden_deserialize_run() {
    let wo = WorkOrderBuilder::new("test task").build();
    let original = Envelope::Run {
        id: uuid1().to_string(),
        work_order: wo,
    };
    let line = serde_json::to_string(&original).unwrap();
    let decoded = JsonlCodec::decode(&line).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, uuid1().to_string());
            assert_eq!(work_order.task, "test task");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn golden_deserialize_event() {
    let line = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-15T12:00:00Z","type":"assistant_delta","text":"hi"}}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(
                matches!(event.kind, AgentEventKind::AssistantDelta { ref text } if text == "hi")
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn golden_deserialize_final() {
    let original = Envelope::Final {
        ref_id: uuid1().to_string(),
        receipt: sample_receipt(),
    };
    let line = serde_json::to_string(&original).unwrap();
    let decoded = JsonlCodec::decode(&line).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, uuid1().to_string());
            assert!(matches!(receipt.outcome, Outcome::Complete));
            assert_eq!(receipt.meta.duration_ms, 1234);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn golden_deserialize_fatal() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "boom");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn golden_deserialize_fatal_with_ref() {
    let line = r#"{"t":"fatal","ref_id":"run-99","error":"timeout"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("run-99"));
            assert_eq!(error, "timeout");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ===========================================================================
// 3. Roundtrip: serialize → deserialize → compare
// ===========================================================================

fn roundtrip(env: &Envelope) {
    let json = serde_json::to_string(env).unwrap();
    let decoded: Envelope = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&decoded).unwrap();
    assert_eq!(json, json2, "roundtrip produced different JSON");
}

#[test]
fn roundtrip_hello() {
    roundtrip(&Envelope::hello(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("2.0".into()),
            adapter_version: Some("0.1".into()),
        },
        sample_caps(),
    ));
}

#[test]
fn roundtrip_run() {
    let wo = WorkOrderBuilder::new("roundtrip task").build();
    roundtrip(&Envelope::Run {
        id: uuid1().to_string(),
        work_order: wo,
    });
}

#[test]
fn roundtrip_event_all_kinds() {
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: json!("file.txt"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        AgentEventKind::Warning {
            message: "caution".into(),
        },
        AgentEventKind::Error {
            message: "oops".into(),
        },
    ];

    for kind in kinds {
        roundtrip(&Envelope::Event {
            ref_id: uuid1().to_string(),
            event: AgentEvent {
                ts: ts(),
                kind,
                ext: None,
            },
        });
    }
}

#[test]
fn roundtrip_final() {
    roundtrip(&Envelope::Final {
        ref_id: uuid1().to_string(),
        receipt: sample_receipt(),
    });
}

#[test]
fn roundtrip_fatal_with_ref() {
    roundtrip(&Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "err".into(),
    });
}

#[test]
fn roundtrip_fatal_without_ref() {
    roundtrip(&Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
    });
}

// ===========================================================================
// 4. Edge cases: missing optional fields, null values, empty arrays
// ===========================================================================

#[test]
fn edge_hello_empty_capabilities() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "bare".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    let json = serde_json::to_string(&env).unwrap();
    let decoded: Envelope = serde_json::from_str(&json).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn edge_hello_mode_defaults_to_mapped() {
    // Omit the "mode" field — should default to Mapped
    let line = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Hello { mode, .. } => assert!(matches!(mode, ExecutionMode::Mapped)),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn edge_fatal_null_ref_id() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"x"}"#;
    let env: Envelope = serde_json::from_str(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn edge_event_with_ext() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some({
                let mut m = BTreeMap::new();
                m.insert("raw_message".into(), json!({"vendor": true}));
                m
            }),
        },
    };
    roundtrip(&env);
}

#[test]
fn edge_event_ext_absent_vs_null() {
    // ext omitted entirely
    let line = r#"{"t":"event","ref_id":"r","event":{"ts":"2025-01-15T12:00:00Z","type":"warning","message":"w"}}"#;
    let env: Envelope = serde_json::from_str(line).unwrap();
    match env {
        Envelope::Event { event, .. } => assert!(event.ext.is_none()),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn edge_tool_call_null_ids() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        },
    };
    roundtrip(&env);
}

#[test]
fn edge_receipt_all_usage_null() {
    let mut receipt = sample_receipt();
    receipt.usage = UsageNormalized {
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    receipt.trace = vec![];
    receipt.artifacts = vec![];
    receipt.verification = VerificationReport {
        git_diff: None,
        git_status: None,
        harness_ok: false,
    };
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    roundtrip(&env);
}

// ===========================================================================
// 5. Multiple envelopes as JSONL (one per line) — parse multi-line input
// ===========================================================================

#[test]
fn jsonl_multi_line_parse() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    let event = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
    };
    let fatal = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "crash".into(),
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, event, fatal]).unwrap();
    let text = String::from_utf8(buf).unwrap();

    // Should be exactly 3 lines (each ending in \n)
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);

    // decode_stream should yield 3 envelopes
    let reader = BufReader::new(text.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 3);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
    assert!(matches!(envelopes[2], Envelope::Fatal { .. }));
}

#[test]
fn jsonl_blank_lines_skipped() {
    let input = format!(
        "{}\n\n{}\n\n",
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
        })
        .unwrap(),
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
        })
        .unwrap(),
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn jsonl_encode_produces_newline_terminated() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    assert!(!line.ends_with("\n\n"));
    // Exactly one newline at end
    assert_eq!(line.matches('\n').count(), 1);
}

// ===========================================================================
// 6. Invalid JSON handling — malformed lines produce clear errors
// ===========================================================================

#[test]
fn invalid_json_empty_string() {
    let err = JsonlCodec::decode("").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("JSON"), "error should mention JSON: {msg}");
}

#[test]
fn invalid_json_garbage() {
    let err = JsonlCodec::decode("not json at all").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("JSON"), "error should mention JSON: {msg}");
}

#[test]
fn invalid_json_truncated() {
    let err = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#).unwrap_err();
    assert!(
        err.to_string().contains("JSON"),
        "truncated JSON should be a JSON error"
    );
}

#[test]
fn invalid_json_missing_required_field() {
    // Fatal requires "error" field
    let err = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null}"#).unwrap_err();
    assert!(
        err.to_string().contains("JSON"),
        "missing field should be a JSON error"
    );
}

#[test]
fn invalid_json_in_stream() {
    let input = format!(
        "{}\nnot json\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"ok"}"#,
        r#"{"t":"fatal","ref_id":null,"error":"also ok"}"#,
    );
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

// ===========================================================================
// 7. Unknown envelope type ("t":"unknown") handling
// ===========================================================================

#[test]
fn unknown_envelope_type() {
    let err = JsonlCodec::decode(r#"{"t":"unknown","data":123}"#).unwrap_err();
    let msg = err.to_string();
    // serde should reject the unknown variant
    assert!(
        msg.contains("JSON"),
        "unknown envelope type should produce JSON error: {msg}"
    );
}

#[test]
fn unknown_envelope_type_misspelled() {
    let err = JsonlCodec::decode(r#"{"t":"helo","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#).unwrap_err();
    assert!(
        err.to_string().contains("JSON"),
        "misspelled type should produce JSON error"
    );
}

#[test]
fn missing_type_tag() {
    let err = JsonlCodec::decode(r#"{"error":"no tag"}"#).unwrap_err();
    assert!(
        err.to_string().contains("JSON"),
        "missing 't' tag should produce JSON error"
    );
}

// ===========================================================================
// 8. Field ordering doesn't matter for deserialization
// ===========================================================================

#[test]
fn field_order_fatal_reversed() {
    // Fields in different order than canonical
    let line = r#"{"error":"boom","ref_id":"r1","t":"fatal"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { ref_id, error } => {
            assert_eq!(ref_id.as_deref(), Some("r1"));
            assert_eq!(error, "boom");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn field_order_hello_shuffled() {
    let line = r#"{"capabilities":{},"mode":"passthrough","t":"hello","backend":{"id":"z","backend_version":null,"adapter_version":null},"contract_version":"abp/v0.1"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            mode,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend.id, "z");
            assert!(matches!(mode, ExecutionMode::Passthrough));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn field_order_event_shuffled() {
    let line = r#"{"event":{"ts":"2025-01-15T12:00:00Z","type":"warning","message":"watch out"},"t":"event","ref_id":"r1"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            assert!(
                matches!(event.kind, AgentEventKind::Warning { ref message } if message == "watch out")
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// ===========================================================================
// 9. Unicode in event content / error messages
// ===========================================================================

#[test]
fn unicode_in_error_message() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "エラー: 接続失敗 🔥".into(),
    };
    roundtrip(&env);
    let json = serde_json::to_string(&env).unwrap();
    let decoded: Envelope = serde_json::from_str(&json).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "エラー: 接続失敗 🔥"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn unicode_in_event_text() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "你好世界 🌍 مرحبا".into(),
            },
            ext: None,
        },
    };
    roundtrip(&env);
}

#[test]
fn unicode_in_tool_output() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("t1".into()),
                output: json!("Ñoño — «diacritics» and 'quotes'"),
                is_error: false,
            },
            ext: None,
        },
    };
    roundtrip(&env);
}

#[test]
fn unicode_escape_sequences_in_raw_json() {
    // JSON with unicode escape sequences should deserialize correctly
    let line = r#"{"t":"fatal","ref_id":null,"error":"\u00e9\u00e8\u00ea"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error, "éèê"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ===========================================================================
// 10. Very long payloads (large tool output, etc.)
// ===========================================================================

#[test]
fn large_tool_output() {
    let large_output = "x".repeat(100_000);
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("t1".into()),
                output: json!(large_output),
                is_error: false,
            },
            ext: None,
        },
    };
    roundtrip(&env);
}

#[test]
fn large_assistant_message() {
    let large_text = "word ".repeat(50_000);
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantMessage {
                text: large_text.clone(),
            },
            ext: None,
        },
    };
    let json = serde_json::to_string(&env).unwrap();
    let decoded: Envelope = serde_json::from_str(&json).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text.len(), large_text.len());
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn large_trace_in_receipt() {
    let mut receipt = sample_receipt();
    receipt.trace = (0..500)
        .map(|i| AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        })
        .collect();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    roundtrip(&env);
}

#[test]
fn large_jsonl_stream() {
    let mut buf = Vec::new();
    let envelopes: Vec<Envelope> = (0..200)
        .map(|i| Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{i}"),
                },
                ext: None,
            },
        })
        .collect();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 200);
}
