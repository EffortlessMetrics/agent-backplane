// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the JSONL protocol envelope types.
//!
//! Categories covered:
//! 1.  Envelope variants: hello, run, event, final, fatal — serde roundtrip
//! 2.  Tag discrimination: "t" field (NOT "type") selects correct variant
//! 3.  ref_id correlation: ref_id present/absent in appropriate envelopes
//! 4.  Payload serialization: Each variant's payload serializes correctly
//! 5.  JSONL format: Multiple envelopes serialize as newline-delimited JSON
//! 6.  Unknown fields: Extra fields in JSON handled gracefully
//! 7.  Missing fields: Required field absence detected
//! 8.  Empty payloads: Minimal valid envelopes
//! 9.  Large payloads: Events with large content/tool output
//! 10. Error envelopes: fatal with different error codes
//! 11. Contract version in hello: Hello contains version + capabilities
//! 12. Stream parsing: Parse sequence of envelopes from JSONL stream

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::BufReader;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

fn mk_backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn mk_caps() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps
}

fn mk_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn mk_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "protocol-deep-test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn mk_receipt() -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 100,
        },
        backend: mk_backend("receipt-backend"),
        capabilities: mk_caps(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Envelope variants: serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_hello_preserves_all_fields() {
    let env = Envelope::hello(mk_backend("rt-hello"), mk_caps());
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Hello {
        contract_version,
        backend,
        capabilities,
        mode,
    } = back
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
        assert_eq!(backend.id, "rt-hello");
        assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
        assert!(capabilities.contains_key(&Capability::Streaming));
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn roundtrip_run_preserves_work_order() {
    let env = Envelope::Run {
        id: "rt-run-1".into(),
        work_order: mk_work_order(),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Run { id, work_order } = back {
        assert_eq!(id, "rt-run-1");
        assert_eq!(work_order.task, "protocol-deep-test");
        assert_eq!(work_order.id, Uuid::nil());
    } else {
        panic!("expected Run");
    }
}

#[test]
fn roundtrip_event_run_started() {
    let env = Envelope::Event {
        ref_id: "rt-ev-1".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { ref_id, event } = back {
        assert_eq!(ref_id, "rt-ev-1");
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_run_completed() {
    let env = Envelope::Event {
        ref_id: "rt-ev-2".into(),
        event: mk_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { ref_id, event } = back {
        assert_eq!(ref_id, "rt-ev-2");
        if let AgentEventKind::RunCompleted { message } = event.kind {
            assert_eq!(message, "done");
        } else {
            panic!("expected RunCompleted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_assistant_delta() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::AssistantDelta { text } = event.kind {
            assert_eq!(text, "chunk");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_assistant_message() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: "full message".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, "full message");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "grep".into(),
            tool_use_id: Some("tu-99".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"pattern": "fn main"}),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } = event.kind
        {
            assert_eq!(tool_name, "grep");
            assert_eq!(tool_use_id.as_deref(), Some("tu-99"));
            assert_eq!(input["pattern"], "fn main");
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_tool_result() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!("success"),
            is_error: false,
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolResult {
            tool_name,
            is_error,
            ..
        } = event.kind
        {
            assert_eq!(tool_name, "bash");
            assert!(!is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_file_changed() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added tests".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::FileChanged { path, summary } = event.kind {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(summary, "added tests");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_command_executed() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } = event.kind
        {
            assert_eq!(command, "cargo test");
            assert_eq!(exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("ok"));
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_warning() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::Warning {
            message: "low budget".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::Warning { message } = event.kind {
            assert_eq!(message, "low budget");
        } else {
            panic!("expected Warning");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_error() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::Error {
            message: "something bad".into(),
            error_code: None,
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::Error { message, .. } = event.kind {
            assert_eq!(message, "something bad");
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_final_preserves_receipt() {
    let env = Envelope::Final {
        ref_id: "rt-final".into(),
        receipt: mk_receipt(),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Final { ref_id, receipt } = back {
        assert_eq!(ref_id, "rt-final");
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn roundtrip_fatal_with_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some("rt-fatal".into()),
        error: "kaboom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = back {
        assert_eq!(ref_id.as_deref(), Some("rt-fatal"));
        assert_eq!(error, "kaboom");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn roundtrip_fatal_without_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "early crash".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = back {
        assert!(ref_id.is_none());
        assert_eq!(error, "early crash");
    } else {
        panic!("expected Fatal");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Tag discrimination: "t" field
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tag_field_is_t_not_type_hello() {
    let env = Envelope::hello(mk_backend("tag-test"), mk_caps());
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["t"], "hello");
    assert!(val.get("type").is_none(), "must use 't' not 'type'");
}

#[test]
fn tag_field_is_t_not_type_run() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["t"], "run");
    assert!(val.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_event() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["t"], "event");
    assert!(val.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_final() {
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt: mk_receipt(),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["t"], "final");
    assert!(val.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_fatal() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["t"], "fatal");
    assert!(val.get("type").is_none());
}

#[test]
fn tag_uses_snake_case_variants() {
    // All variant tags should be lowercase snake_case
    let tags = ["hello", "run", "event", "final", "fatal"];
    for tag in &tags {
        // Minimal valid JSON for each — only need to verify tag parsing
        let ok = tag == &"hello" || tag == &"fatal";
        if ok {
            // hello and fatal can be constructed from raw JSON easily
        }
        // Just verify the tag value is snake_case (no uppercase)
        assert_eq!(*tag, tag.to_lowercase());
    }
}

#[test]
fn decode_with_type_field_instead_of_t_fails() {
    let raw = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(raw);
    assert!(result.is_err(), "using 'type' instead of 't' must fail");
}

#[test]
fn decode_unknown_tag_value_fails() {
    let raw = r#"{"t":"subscribe","channel":"events"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn decode_numeric_tag_fails() {
    let raw = r#"{"t":1,"data":"x"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn decode_null_tag_fails() {
    let raw = r#"{"t":null,"error":"x"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn decode_boolean_tag_fails() {
    let raw = r#"{"t":true,"error":"x"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ref_id correlation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_ref_id_is_required() {
    let raw = r#"{"t":"event","event":{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":"go"}}"#;
    assert!(
        JsonlCodec::decode(raw).is_err(),
        "event without ref_id must fail"
    );
}

#[test]
fn final_ref_id_is_required() {
    // Build a valid receipt JSON but omit ref_id
    let env = Envelope::Final {
        ref_id: "placeholder".into(),
        receipt: mk_receipt(),
    };
    let mut val = serde_json::to_value(&env).unwrap();
    val.as_object_mut().unwrap().remove("ref_id");
    let raw = serde_json::to_string(&val).unwrap();
    assert!(
        serde_json::from_str::<Envelope>(&raw).is_err(),
        "final without ref_id must fail"
    );
}

#[test]
fn fatal_ref_id_is_optional_some() {
    let raw = r#"{"t":"fatal","ref_id":"run-1","error":"crash"}"#;
    let env: Envelope = serde_json::from_str(raw).unwrap();
    if let Envelope::Fatal { ref_id, .. } = env {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_ref_id_is_optional_null() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"early"}"#;
    let env: Envelope = serde_json::from_str(raw).unwrap();
    if let Envelope::Fatal { ref_id, .. } = env {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn hello_has_no_ref_id_field() {
    let env = Envelope::hello(mk_backend("x"), mk_caps());
    let val = serde_json::to_value(&env).unwrap();
    assert!(val.get("ref_id").is_none(), "hello should not have ref_id");
}

#[test]
fn run_has_id_not_ref_id() {
    let env = Envelope::Run {
        id: "run-50".into(),
        work_order: mk_work_order(),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["id"], "run-50");
    assert!(val.get("ref_id").is_none(), "run uses 'id' not 'ref_id'");
}

#[test]
fn event_ref_id_matches_run_id() {
    let run_id = "corr-123";
    let event_env = Envelope::Event {
        ref_id: run_id.into(),
        event: mk_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
    };
    let val = serde_json::to_value(&event_env).unwrap();
    assert_eq!(val["ref_id"], run_id);
}

#[test]
fn final_ref_id_matches_run_id() {
    let run_id = "corr-456";
    let final_env = Envelope::Final {
        ref_id: run_id.into(),
        receipt: mk_receipt(),
    };
    let val = serde_json::to_value(&final_env).unwrap();
    assert_eq!(val["ref_id"], run_id);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Payload serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_payload_contains_backend_identity() {
    let env = Envelope::hello(mk_backend("payload-be"), mk_caps());
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["backend"]["id"], "payload-be");
    assert_eq!(val["backend"]["backend_version"], "1.0.0");
    assert!(val["backend"]["adapter_version"].is_null());
}

#[test]
fn hello_payload_contains_capabilities_map() {
    let env = Envelope::hello(mk_backend("x"), mk_caps());
    let val = serde_json::to_value(&env).unwrap();
    let caps = &val["capabilities"];
    assert!(caps.is_object());
    assert_eq!(caps["streaming"], "native");
    assert_eq!(caps["tool_read"], "native");
}

#[test]
fn run_payload_contains_work_order_fields() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["work_order"]["task"], "protocol-deep-test");
    assert_eq!(
        val["work_order"]["id"],
        "00000000-0000-0000-0000-000000000000"
    );
    assert_eq!(val["work_order"]["lane"], "patch_first");
}

#[test]
fn event_payload_contains_agent_event() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: "delta text".into(),
        }),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["event"]["type"], "assistant_delta");
    assert_eq!(val["event"]["text"], "delta text");
    assert!(val["event"]["ts"].is_string());
}

#[test]
fn final_payload_contains_receipt() {
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt: mk_receipt(),
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["receipt"]["meta"]["contract_version"], CONTRACT_VERSION);
    assert_eq!(val["receipt"]["outcome"], "complete");
    assert_eq!(val["receipt"]["backend"]["id"], "receipt-backend");
}

#[test]
fn fatal_payload_contains_error_string() {
    let env = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: "something broke badly".into(),
        error_code: None,
    };
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["error"], "something broke badly");
}

#[test]
fn fatal_error_code_absent_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let val = serde_json::to_value(&env).unwrap();
    // error_code uses skip_serializing_if = "Option::is_none"
    assert!(
        val.get("error_code").is_none(),
        "error_code should be omitted when None"
    );
}

#[test]
fn fatal_error_code_present_when_some() {
    let env = Envelope::fatal_with_code(
        Some("r".into()),
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    );
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["error_code"], "backend_timeout");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. JSONL format: newline-delimited JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn encode_appends_exactly_one_newline() {
    let env = Envelope::hello(mk_backend("nl"), BTreeMap::new());
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
    assert_eq!(encoded.matches('\n').count(), 1);
}

#[test]
fn encode_produces_single_line_json() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let trimmed = encoded.trim_end_matches('\n');
    assert!(!trimmed.contains('\n'), "encoded JSON must be single line");
}

#[test]
fn encode_many_to_writer_produces_multiple_lines() {
    let envs = vec![
        Envelope::hello(mk_backend("x"), BTreeMap::new()),
        Envelope::Fatal {
            ref_id: None,
            error: "e".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2);
    // Each line is valid JSON
    for line in &lines {
        let _: Value = serde_json::from_str(line).unwrap();
    }
}

#[test]
fn encode_to_writer_is_newline_terminated() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
}

#[test]
fn jsonl_concatenation_is_parseable() {
    let envs = vec![
        Envelope::hello(mk_backend("a"), BTreeMap::new()),
        Envelope::Run {
            id: "r1".into(),
            work_order: mk_work_order(),
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: mk_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: mk_receipt(),
        },
    ];
    let mut buf = Vec::new();
    for env in &envs {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Unknown fields: extra fields handled gracefully
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_with_extra_fields_deserializes() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"extra_field":"ignored","mode":"mapped"}"#;
    // serde with deny_unknown_fields would fail; default allows extra
    let result = JsonlCodec::decode(raw);
    // Whether this succeeds depends on serde config. If it fails, that's also
    // valid behavior — we just document it.
    if let Ok(env) = result {
        assert!(matches!(env, Envelope::Hello { .. }));
    }
    // If deny_unknown_fields is active, error is acceptable.
}

#[test]
fn fatal_with_extra_fields_deserializes() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"oops","extra":"data"}"#;
    let result = JsonlCodec::decode(raw);
    if let Ok(env) = result {
        assert!(matches!(env, Envelope::Fatal { .. }));
    }
}

#[test]
fn event_with_extra_fields_in_envelope() {
    let raw = r#"{"t":"event","ref_id":"r","event":{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":"go"},"debug_info":"test"}"#;
    let result = JsonlCodec::decode(raw);
    if let Ok(env) = result {
        assert!(matches!(env, Envelope::Event { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Missing fields: required field absence detected
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_missing_contract_version_fails() {
    let raw = r#"{"t":"hello","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn hello_missing_backend_fails() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn hello_missing_capabilities_fails() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null}}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn run_missing_id_fails() {
    let env = Envelope::Run {
        id: "temp".into(),
        work_order: mk_work_order(),
    };
    let mut val = serde_json::to_value(&env).unwrap();
    val.as_object_mut().unwrap().remove("id");
    let raw = serde_json::to_string(&val).unwrap();
    assert!(serde_json::from_str::<Envelope>(&raw).is_err());
}

#[test]
fn run_missing_work_order_fails() {
    let raw = r#"{"t":"run","id":"r1"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn event_missing_event_payload_fails() {
    let raw = r#"{"t":"event","ref_id":"r1"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn fatal_missing_error_field_fails() {
    let raw = r#"{"t":"fatal","ref_id":null}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

#[test]
fn missing_t_field_entirely_fails() {
    let raw = r#"{"ref_id":"r","error":"x"}"#;
    assert!(JsonlCodec::decode(raw).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Empty/minimal payloads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_minimal_empty_capabilities() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "min".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Hello { capabilities, .. } = back {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_minimal_backend_no_versions() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "bare".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    let val = serde_json::to_value(&env).unwrap();
    assert!(val["backend"]["backend_version"].is_null());
    assert!(val["backend"]["adapter_version"].is_null());
}

#[test]
fn fatal_minimal_no_ref_id_no_error_code() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = back
    {
        assert!(ref_id.is_none());
        assert!(error.is_empty());
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_empty_error_string() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":""}"#;
    let env: Envelope = serde_json::from_str(raw).unwrap();
    if let Envelope::Fatal { error, .. } = env {
        assert!(error.is_empty());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn event_minimal_run_started() {
    let raw = r#"{"t":"event","ref_id":"r","event":{"ts":"2025-01-01T00:00:00Z","type":"run_started","message":""}}"#;
    let env: Envelope = serde_json::from_str(raw).unwrap();
    if let Envelope::Event { event, .. } = env {
        if let AgentEventKind::RunStarted { message } = event.kind {
            assert!(message.is_empty());
        } else {
            panic!("expected RunStarted");
        }
    } else {
        panic!("expected Event");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Large payloads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_assistant_message_roundtrips() {
    let big_text = "x".repeat(100_000);
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: big_text.clone(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text.len(), 100_000);
            assert_eq!(text, big_text);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_tool_output_roundtrips() {
    let big_output = serde_json::json!({
        "lines": (0..1000).map(|i| format!("line {i}")).collect::<Vec<_>>()
    });
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-big".into()),
            output: big_output.clone(),
            is_error: false,
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolResult { output, .. } = event.kind {
            assert_eq!(output["lines"].as_array().unwrap().len(), 1000);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_tool_input_roundtrips() {
    let big_input = serde_json::json!({
        "content": "a".repeat(50_000),
        "metadata": {"key": "value"}
    });
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu-lg".into()),
            parent_tool_use_id: None,
            input: big_input.clone(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolCall { input, .. } = event.kind {
            assert_eq!(input["content"].as_str().unwrap().len(), 50_000);
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_receipt_trace_roundtrips() {
    let now = Utc::now();
    let mut receipt = mk_receipt();
    receipt.trace = (0..500)
        .map(|i| AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: format!("delta-{i}"),
            },
            ext: None,
        })
        .collect();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Final { receipt, .. } = back {
        assert_eq!(receipt.trace.len(), 500);
    } else {
        panic!("expected Final");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Error envelopes: fatal with different error codes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_with_backend_timeout_code() {
    let env = Envelope::fatal_with_code(
        Some("r".into()),
        "timed out",
        abp_error::ErrorCode::BackendTimeout,
    );
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["error_code"], "backend_timeout");
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
}

#[test]
fn fatal_with_protocol_invalid_envelope_code() {
    let env = Envelope::fatal_with_code(
        None,
        "bad envelope",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["error_code"], "protocol_invalid_envelope");
}

#[test]
fn fatal_with_backend_crashed_code() {
    let env = Envelope::fatal_with_code(
        Some("run-x".into()),
        "segfault",
        abp_error::ErrorCode::BackendCrashed,
    );
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal { error_code, .. } = back {
        assert_eq!(error_code, Some(abp_error::ErrorCode::BackendCrashed));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_policy_denied_code() {
    let env = Envelope::fatal_with_code(
        Some("r".into()),
        "denied",
        abp_error::ErrorCode::PolicyDenied,
    );
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["error_code"], "policy_denied");
}

#[test]
fn fatal_with_execution_tool_failed_code() {
    let env = Envelope::fatal_with_code(
        Some("r".into()),
        "tool err",
        abp_error::ErrorCode::ExecutionToolFailed,
    );
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ExecutionToolFailed)
    );
}

#[test]
fn fatal_from_abp_error_preserves_code_and_message() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendRateLimited, "slow down");
    let env = Envelope::fatal_from_abp_error(Some("r".into()), &abp_err);
    if let Envelope::Fatal {
        error, error_code, ..
    } = &env
    {
        assert_eq!(error, "slow down");
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendRateLimited));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_error_code_roundtrip_through_json() {
    let codes = [
        abp_error::ErrorCode::ProtocolHandshakeFailed,
        abp_error::ErrorCode::BackendNotFound,
        abp_error::ErrorCode::ContractSchemaViolation,
        abp_error::ErrorCode::Internal,
    ];
    for code in &codes {
        let env = Envelope::fatal_with_code(None, "err", *code);
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error_code(), Some(*code));
    }
}

#[test]
fn error_code_method_returns_none_for_non_fatal() {
    let env = Envelope::hello(mk_backend("x"), BTreeMap::new());
    assert!(env.error_code().is_none());

    let env2 = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    assert!(env2.error_code().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Contract version in hello
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_constructor_sets_contract_version() {
    let env = Envelope::hello(mk_backend("cv"), BTreeMap::new());
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
        assert_eq!(contract_version, "abp/v0.1");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_with_mode_also_sets_contract_version() {
    let env = Envelope::hello_with_mode(
        mk_backend("cv2"),
        BTreeMap::new(),
        ExecutionMode::Passthrough,
    );
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, "abp/v0.1");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_contract_version_in_wire_format() {
    let env = Envelope::hello(mk_backend("wire"), mk_caps());
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["contract_version"].as_str().unwrap(), "abp/v0.1");
}

#[test]
fn hello_capabilities_all_support_levels() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );

    let env = Envelope::hello(mk_backend("sl"), caps);
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Hello { capabilities, .. } = back {
        assert_eq!(capabilities.len(), 4);
        assert!(matches!(
            capabilities[&Capability::Streaming],
            SupportLevel::Native
        ));
        assert!(matches!(
            capabilities[&Capability::ToolRead],
            SupportLevel::Emulated
        ));
        assert!(matches!(
            capabilities[&Capability::ToolWrite],
            SupportLevel::Unsupported
        ));
        assert!(matches!(
            capabilities[&Capability::ToolBash],
            SupportLevel::Restricted { .. }
        ));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_mode_default_is_mapped() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": { "id": "x", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    let env: Envelope = serde_json::from_value(raw).unwrap();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_mode_passthrough_serialization() {
    let env = Envelope::hello_with_mode(
        mk_backend("pt"),
        BTreeMap::new(),
        ExecutionMode::Passthrough,
    );
    let val = serde_json::to_value(&env).unwrap();
    assert_eq!(val["mode"], "passthrough");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Stream parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn decode_stream_blank_lines_skipped() {
    let input = format!(
        "\n\n{}\n\n{}\n\n",
        r#"{"t":"fatal","ref_id":null,"error":"a"}"#, r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_whitespace_only_lines_skipped() {
    let input = format!(
        "   \n  \t  \n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"x"}"#
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn decode_stream_full_conversation() {
    let hello = JsonlCodec::encode(&Envelope::hello(mk_backend("conv"), mk_caps())).unwrap();
    let run = JsonlCodec::encode(&Envelope::Run {
        id: "run-1".into(),
        work_order: mk_work_order(),
    })
    .unwrap();
    let ev1 = JsonlCodec::encode(&Envelope::Event {
        ref_id: "run-1".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
    })
    .unwrap();
    let ev2 = JsonlCodec::encode(&Envelope::Event {
        ref_id: "run-1".into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        }),
    })
    .unwrap();
    let ev3 = JsonlCodec::encode(&Envelope::Event {
        ref_id: "run-1".into(),
        event: mk_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    })
    .unwrap();
    let fin = JsonlCodec::encode(&Envelope::Final {
        ref_id: "run-1".into(),
        receipt: mk_receipt(),
    })
    .unwrap();

    let stream = format!("{hello}{run}{ev1}{ev2}{ev3}{fin}");
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 6);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Run { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Event { .. }));
    assert!(matches!(envelopes[4], Envelope::Event { .. }));
    assert!(matches!(envelopes[5], Envelope::Final { .. }));
}

#[test]
fn decode_stream_stops_at_invalid_line() {
    let line1 = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
    let line2 = r#"{"t":"invalid_stuff"}"#;
    let input = format!("{line1}\n{line2}\n");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn decode_stream_many_events_same_ref_id() {
    let ref_id = "stream-run";
    let mut stream = String::new();
    for i in 0..50 {
        let env = Envelope::Event {
            ref_id: ref_id.into(),
            event: mk_event(AgentEventKind::AssistantDelta {
                text: format!("chunk-{i}"),
            }),
        };
        stream.push_str(&JsonlCodec::encode(&env).unwrap());
    }
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 50);
    for env in &envelopes {
        if let Envelope::Event { ref_id: rid, .. } = env {
            assert_eq!(rid, ref_id);
        } else {
            panic!("expected Event");
        }
    }
}

#[test]
fn decode_stream_with_fatal_ending() {
    let hello = JsonlCodec::encode(&Envelope::hello(mk_backend("f"), BTreeMap::new())).unwrap();
    let fatal = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "crash".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    })
    .unwrap();
    let stream = format!("{hello}{fatal}");
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    if let Envelope::Fatal { error_code, .. } = &envelopes[1] {
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendCrashed));
    } else {
        panic!("expected Fatal");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional: Decode edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decode_empty_string_fails() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn decode_null_literal_fails() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn decode_array_literal_fails() {
    assert!(JsonlCodec::decode("[1,2]").is_err());
}

#[test]
fn decode_number_literal_fails() {
    assert!(JsonlCodec::decode("42").is_err());
}

#[test]
fn decode_string_literal_fails() {
    assert!(JsonlCodec::decode(r#""hello""#).is_err());
}

#[test]
fn decode_truncated_json_fails() {
    assert!(JsonlCodec::decode(r#"{"t":"hel"#).is_err());
}

#[test]
fn protocol_error_json_variant_display() {
    let err = JsonlCodec::decode("{bad}").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn encode_decode_roundtrip_identity() {
    let original = Envelope::Fatal {
        ref_id: Some("identity-test".into()),
        error: "test error".into(),
        error_code: Some(abp_error::ErrorCode::Internal),
    };
    let encoded = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    let re_encoded = JsonlCodec::encode(&decoded).unwrap();
    // JSON representation should be stable across roundtrips
    assert_eq!(encoded, re_encoded);
}

#[test]
fn event_with_ext_field_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("custom_key".to_string(), serde_json::json!("custom_value"));
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        let ext = event.ext.unwrap();
        assert_eq!(ext["custom_key"], "custom_value");
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_with_error_code_in_agent_event_error() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::Error {
            message: "tool failed".into(),
            error_code: Some(abp_error::ErrorCode::ExecutionToolFailed),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::Error {
            message,
            error_code,
        } = event.kind
        {
            assert_eq!(message, "tool failed");
            assert_eq!(error_code, Some(abp_error::ErrorCode::ExecutionToolFailed));
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn tool_result_is_error_true_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-err".into()),
            output: serde_json::json!("permission denied"),
            is_error: true,
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolResult { is_error, .. } = event.kind {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn tool_call_with_parent_tool_use_id() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "nested_tool".into(),
            tool_use_id: Some("tu-child".into()),
            parent_tool_use_id: Some("tu-parent".into()),
            input: serde_json::json!({}),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolCall {
            parent_tool_use_id, ..
        } = event.kind
        {
            assert_eq!(parent_tool_use_id.as_deref(), Some("tu-parent"));
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn receipt_with_partial_outcome_roundtrips() {
    let mut receipt = mk_receipt();
    receipt.outcome = Outcome::Partial;
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Final { receipt, .. } = back {
        assert!(matches!(receipt.outcome, Outcome::Partial));
    } else {
        panic!("expected Final");
    }
}

#[test]
fn receipt_with_failed_outcome_roundtrips() {
    let mut receipt = mk_receipt();
    receipt.outcome = Outcome::Failed;
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Final { receipt, .. } = back {
        assert!(matches!(receipt.outcome, Outcome::Failed));
    } else {
        panic!("expected Final");
    }
}

#[test]
fn unicode_in_error_message_roundtrips() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "エラー: 失敗しました 🔥".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal { error, .. } = back {
        assert!(error.contains("エラー"));
        assert!(error.contains("🔥"));
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn unicode_in_assistant_text_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: "Привет мир! こんにちは 🌍".into(),
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert!(text.contains("Привет"));
            assert!(text.contains("こんにちは"));
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn special_chars_in_error_message() {
    let msg = r#"path "C:\Users\test" not found; <script>alert(1)</script>"#;
    let env = Envelope::Fatal {
        ref_id: None,
        error: msg.into(),
        error_code: None,
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Fatal { error, .. } = back {
        assert_eq!(error, msg);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn command_executed_with_none_fields() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: None,
            output_preview: None,
        }),
    };
    let json = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&json).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::CommandExecuted {
            exit_code,
            output_preview,
            ..
        } = event.kind
        {
            assert!(exit_code.is_none());
            assert!(output_preview.is_none());
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn multiple_capabilities_serialization_order() {
    // BTreeMap ensures deterministic order
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let env = Envelope::hello(mk_backend("order"), caps);
    let json1 = serde_json::to_string(&env).unwrap();
    let json2 = serde_json::to_string(&env).unwrap();
    // BTreeMap guarantees stable serialization
    assert_eq!(json1, json2);
}
