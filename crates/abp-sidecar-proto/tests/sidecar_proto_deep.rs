// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for abp-sidecar-proto.

use abp_core::*;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::{Envelope, JsonlCodec};
use abp_sidecar_proto::*;
use async_trait::async_trait;
use chrono::Utc;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_identity() -> BackendIdentity {
    BackendIdentity {
        id: "deep-test-sidecar".into(),
        backend_version: Some("0.1.0".into()),
        adapter_version: Some("0.2.0".into()),
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m
}

fn test_work_order() -> WorkOrder {
    WorkOrderBuilder::new("deep test task").build()
}

fn test_receipt(run_id: Uuid) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 100,
        },
        backend: test_identity(),
        capabilities: test_capabilities(),
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

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn run_started_event() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "started".into(),
    })
}

fn run_completed_event() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    })
}

async fn drain_duplex(mut r: tokio::io::DuplexStream) -> String {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).await.unwrap();
    String::from_utf8(buf).unwrap()
}

fn build_run_input(run_id: &str, wo: &WorkOrder) -> Vec<u8> {
    let env = Envelope::Run {
        id: run_id.into(),
        work_order: wo.clone(),
    };
    JsonlCodec::encode(&env).unwrap().into_bytes()
}

fn decode_lines(text: &str) -> Vec<Envelope> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| JsonlCodec::decode(l.trim()).unwrap())
        .collect()
}

// ---------------------------------------------------------------------------
// Test handlers
// ---------------------------------------------------------------------------

struct MultiEventHandler {
    event_count: usize,
}

#[async_trait]
impl SidecarHandler for MultiEventHandler {
    async fn on_run(
        &self,
        _run_id: String,
        wo: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        for i in 0..self.event_count {
            sender
                .send_event(make_event(AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                }))
                .await?;
        }
        sender.send_final(test_receipt(wo.id)).await?;
        Ok(())
    }
}

struct NoEventHandler;

#[async_trait]
impl SidecarHandler for NoEventHandler {
    async fn on_run(
        &self,
        _run_id: String,
        wo: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        sender.send_final(test_receipt(wo.id)).await?;
        Ok(())
    }
}

struct FatalHandler;

#[async_trait]
impl SidecarHandler for FatalHandler {
    async fn on_run(
        &self,
        _run_id: String,
        _wo: WorkOrder,
        _sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        Err(SidecarProtoError::Handler("handler went boom".into()))
    }
}

// =========================================================================
// (a) Message type tests (10)
// =========================================================================

#[tokio::test]
async fn hello_message_construction_and_fields() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, test_identity(), test_capabilities())
        .await
        .unwrap();
    drop(w);
    let text = drain_duplex(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Hello {
            backend,
            contract_version,
            capabilities,
            mode,
        } => {
            assert_eq!(backend.id, "deep-test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("0.1.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("0.2.0"));
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert!(capabilities.contains_key(&Capability::Streaming));
            assert!(capabilities.contains_key(&Capability::ToolRead));
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn run_message_with_work_order() {
    let wo = test_work_order();
    let bytes = build_run_input("run-deep-1", &wo);
    let env = JsonlCodec::decode(std::str::from_utf8(&bytes).unwrap().trim()).unwrap();
    match env {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-deep-1");
            assert_eq!(work_order.task, "deep test task");
            assert_eq!(work_order.id, wo.id);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[tokio::test]
async fn event_message_with_agent_event() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "run-ev", run_started_event())
        .await
        .unwrap();
    drop(w);
    let text = drain_duplex(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-ev");
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn final_message_with_receipt() {
    let (mut w, r) = tokio::io::duplex(8192);
    let receipt = test_receipt(Uuid::nil());
    send_final(&mut w, "run-fin", receipt).await.unwrap();
    drop(w);
    let text = drain_duplex(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-fin");
            assert_eq!(receipt.outcome, Outcome::Complete);
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn fatal_message_with_error() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, Some("run-fat".into()), "something broke")
        .await
        .unwrap();
    drop(w);
    let text = drain_duplex(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-fat"));
            assert_eq!(error, "something broke");
            assert!(error_code.is_none());
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_serde_roundtrip() {
    let original = Envelope::hello(test_identity(), test_capabilities());
    let json = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match (&original, &decoded) {
        (
            Envelope::Hello {
                contract_version: cv1,
                backend: b1,
                ..
            },
            Envelope::Hello {
                contract_version: cv2,
                backend: b2,
                ..
            },
        ) => {
            assert_eq!(cv1, cv2);
            assert_eq!(b1.id, b2.id);
        }
        _ => panic!("roundtrip mismatch"),
    }
}

#[tokio::test]
async fn event_serde_roundtrip() {
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "/tmp/test.rs"}),
    });
    let original = Envelope::Event {
        ref_id: "run-rt".into(),
        event,
    };
    let json = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-rt");
            match event.kind {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    ..
                } => {
                    assert_eq!(tool_name, "read_file");
                    assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
                }
                other => panic!("unexpected event kind: {other:?}"),
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn message_discriminator_tags_correct() {
    let hello_json =
        JsonlCodec::encode(&Envelope::hello(test_identity(), test_capabilities())).unwrap();
    assert!(hello_json.contains(r#""t":"hello""#));

    let run_json = JsonlCodec::encode(&Envelope::Run {
        id: "r1".into(),
        work_order: test_work_order(),
    })
    .unwrap();
    assert!(run_json.contains(r#""t":"run""#));

    let event_json = JsonlCodec::encode(&Envelope::Event {
        ref_id: "r1".into(),
        event: run_started_event(),
    })
    .unwrap();
    assert!(event_json.contains(r#""t":"event""#));

    let final_json = JsonlCodec::encode(&Envelope::Final {
        ref_id: "r1".into(),
        receipt: test_receipt(Uuid::nil()),
    })
    .unwrap();
    assert!(final_json.contains(r#""t":"final""#));

    let fatal_json = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    })
    .unwrap();
    assert!(fatal_json.contains(r#""t":"fatal""#));
}

#[tokio::test]
async fn required_fields_present_in_serialized_hello() {
    let json = JsonlCodec::encode(&Envelope::hello(test_identity(), test_capabilities())).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(v.get("t").is_some());
    assert!(v.get("contract_version").is_some());
    assert!(v.get("backend").is_some());
    assert!(v.get("capabilities").is_some());
    let backend = v.get("backend").unwrap();
    assert!(backend.get("id").is_some());
}

#[tokio::test]
async fn optional_fields_handled_on_fatal() {
    // Fatal with no ref_id
    let json1 = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    })
    .unwrap();
    let v1: serde_json::Value = serde_json::from_str(json1.trim()).unwrap();
    assert!(v1.get("ref_id").unwrap().is_null());

    // Fatal with ref_id
    let json2 = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: Some("run-x".into()),
        error: "boom".into(),
        error_code: None,
    })
    .unwrap();
    let v2: serde_json::Value = serde_json::from_str(json2.trim()).unwrap();
    assert_eq!(v2.get("ref_id").unwrap().as_str(), Some("run-x"));

    // error_code is skipped when None
    assert!(v1.get("error_code").is_none());
}

// =========================================================================
// (b) Protocol sequence tests (10)
// =========================================================================

#[tokio::test]
async fn valid_full_conversation_sequence() {
    let wo = test_work_order();
    let input = build_run_input("run-full", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let handler = MultiEventHandler { event_count: 3 };
    let server = SidecarServer::new(handler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);

    // hello, 3 events, final = 5 envelopes
    assert_eq!(envelopes.len(), 5);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Event { .. }));
    assert!(matches!(envelopes[4], Envelope::Final { .. }));
}

#[tokio::test]
async fn hello_must_be_first_message_in_output() {
    let wo = test_work_order();
    let input = build_run_input("run-h1st", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let server = SidecarServer::new(NoEventHandler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
}

#[tokio::test]
async fn server_expects_run_after_sending_hello() {
    // Send a hello envelope as input — server should reject since it expects Run
    let hello = Envelope::hello(test_identity(), test_capabilities());
    let input = JsonlCodec::encode(&hello).unwrap().into_bytes();
    let (mut w, _r) = tokio::io::duplex(4096);

    let server = SidecarServer::new(NoEventHandler, test_identity(), test_capabilities());
    let result = server.run_with_io(input.as_slice(), &mut w).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("unexpected message"));
}

#[tokio::test]
async fn events_follow_run_in_handler_output() {
    let wo = test_work_order();
    let input = build_run_input("run-efr", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let handler = MultiEventHandler { event_count: 2 };
    let server = SidecarServer::new(handler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);
    // After hello, all middle envelopes should be events
    for env in &envelopes[1..envelopes.len() - 1] {
        assert!(matches!(env, Envelope::Event { .. }));
    }
}

#[tokio::test]
async fn final_terminates_sequence() {
    let wo = test_work_order();
    let input = build_run_input("run-term", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let handler = MultiEventHandler { event_count: 1 };
    let server = SidecarServer::new(handler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);
    let last = envelopes.last().unwrap();
    assert!(matches!(last, Envelope::Final { .. }));
}

#[tokio::test]
async fn multiple_events_before_final() {
    let wo = test_work_order();
    let input = build_run_input("run-multi", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let handler = MultiEventHandler { event_count: 5 };
    let server = SidecarServer::new(handler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);
    // hello + 5 events + final = 7
    assert_eq!(envelopes.len(), 7);
    let event_count = envelopes
        .iter()
        .filter(|e| matches!(e, Envelope::Event { .. }))
        .count();
    assert_eq!(event_count, 5);
}

#[tokio::test]
async fn no_events_before_final() {
    let wo = test_work_order();
    let input = build_run_input("run-empty", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let server = SidecarServer::new(NoEventHandler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);
    // hello + final = 2, no events
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Final { .. }));
}

#[tokio::test]
async fn double_final_detected_by_sequence_validator() {
    let run_id = "run-dbl-fin";
    let sequence = vec![
        Envelope::hello(test_identity(), test_capabilities()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::nil()),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[tokio::test]
async fn hello_not_first_detected_by_sequence_validator() {
    // Hello at position 1 instead of 0 should be flagged
    let run_id = "run-hello-pos";
    let sequence = vec![
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::hello(test_identity(), test_capabilities()),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors.contains(&SequenceError::HelloNotFirst { position: 1 }),
        "hello not at position 0 should be flagged: {errors:?}"
    );
}

#[tokio::test]
async fn fatal_can_come_at_any_time_after_hello() {
    // Fatal immediately after hello (no run) — server side sends fatal on error
    let wo = test_work_order();
    let input = build_run_input("run-fatal-any", &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let server = SidecarServer::new(FatalHandler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);
    // hello + fatal
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

// =========================================================================
// (c) Interop with protocol crate (5)
// =========================================================================

#[tokio::test]
async fn sidecar_proto_types_align_with_protocol_envelope() {
    // EventSender produces Envelope variants that can be encoded by JsonlCodec
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run-interop");

    sender.send_event(run_started_event()).await.unwrap();
    sender.send_final(test_receipt(Uuid::nil())).await.unwrap();
    sender.send_fatal("test error").await.unwrap();

    // Each envelope should serialize cleanly
    while let Ok(envelope) = rx.try_recv() {
        let json = JsonlCodec::encode(&envelope).unwrap();
        assert!(json.ends_with('\n'));
        let roundtrip = JsonlCodec::decode(json.trim()).unwrap();
        // Check discriminator tag preserved
        let orig_tag = serde_json::to_value(&envelope).unwrap()["t"].clone();
        let rt_tag = serde_json::to_value(&roundtrip).unwrap()["t"].clone();
        assert_eq!(orig_tag, rt_tag);
    }
}

#[tokio::test]
async fn envelope_wrapping_unwrapping() {
    // Write envelopes via sidecar-proto free functions, read back via JsonlCodec
    let (mut w, r) = tokio::io::duplex(16384);

    send_hello(&mut w, test_identity(), test_capabilities())
        .await
        .unwrap();
    send_event(&mut w, "run-wrap", run_started_event())
        .await
        .unwrap();
    send_final(&mut w, "run-wrap", test_receipt(Uuid::nil()))
        .await
        .unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    // Decode using protocol crate's decode_stream
    let reader = std::io::BufReader::new(text.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 3);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
    assert!(matches!(envelopes[2], Envelope::Final { .. }));
}

#[tokio::test]
async fn message_type_conversion_event_sender_to_envelope() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run-conv");

    let tool_event = make_event(AgentEventKind::ToolResult {
        tool_name: "write_file".into(),
        tool_use_id: Some("tr-1".into()),
        output: serde_json::json!({"ok": true}),
        is_error: false,
    });

    sender.send_event(tool_event).await.unwrap();
    let envelope = rx.try_recv().unwrap();

    // Verify the envelope wraps the event correctly with ref_id
    match envelope {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-conv");
            match event.kind {
                AgentEventKind::ToolResult {
                    tool_name,
                    is_error,
                    ..
                } => {
                    assert_eq!(tool_name, "write_file");
                    assert!(!is_error);
                }
                other => panic!("expected ToolResult, got {other:?}"),
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn contract_version_matching_in_hello() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, test_identity(), test_capabilities())
        .await
        .unwrap();
    drop(w);
    let text = drain_duplex(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert!(abp_protocol::is_compatible_version(
                &contract_version,
                CONTRACT_VERSION
            ));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn ref_id_propagation_through_server() {
    let wo = test_work_order();
    let run_id = "run-refid-prop";
    let input = build_run_input(run_id, &wo);
    let (mut w, r) = tokio::io::duplex(16384);

    let handler = MultiEventHandler { event_count: 2 };
    let server = SidecarServer::new(handler, test_identity(), test_capabilities());
    server.run_with_io(input.as_slice(), &mut w).await.unwrap();
    drop(w);

    let text = drain_duplex(r).await;
    let envelopes = decode_lines(&text);

    // All event and final envelopes should carry the same ref_id
    for env in &envelopes {
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, run_id),
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, run_id),
            Envelope::Hello { .. } => {} // no ref_id on hello
            other => panic!("unexpected envelope type: {other:?}"),
        }
    }
}

// =========================================================================
// Bonus edge-case tests
// =========================================================================

#[tokio::test]
async fn event_sender_ref_id_accessor() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "my-run-id");
    assert_eq!(sender.ref_id(), "my-run-id");
}

#[tokio::test]
async fn event_sender_channel_closed_errors() {
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run-closed");
    drop(rx); // close receiver

    let result_event = sender.send_event(run_started_event()).await;
    assert!(result_event.is_err());

    let result_final = sender.send_final(test_receipt(Uuid::nil())).await;
    assert!(result_final.is_err());

    let result_fatal = sender.send_fatal("boom").await;
    assert!(result_fatal.is_err());
}

#[tokio::test]
async fn server_rejects_invalid_json_input() {
    let input = b"this is not json at all\n";
    let (mut w, _r) = tokio::io::duplex(4096);

    let server = SidecarServer::new(NoEventHandler, test_identity(), test_capabilities());
    let result = server.run_with_io(input.as_slice(), &mut w).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_handles_empty_stdin_gracefully() {
    let input: &[u8] = b"";
    let (mut w, _r) = tokio::io::duplex(4096);

    let server = SidecarServer::new(NoEventHandler, test_identity(), test_capabilities());
    let result = server.run_with_io(input, &mut w).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        SidecarProtoError::StdinClosed
    ));
}

#[tokio::test]
async fn sequence_validator_accepts_valid_sidecar_output() {
    let run_id = "run-valid-seq";
    let sequence = vec![
        Envelope::hello(test_identity(), test_capabilities()),
        Envelope::Run {
            id: run_id.into(),
            work_order: test_work_order(),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: run_started_event(),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: run_completed_event(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: test_receipt(Uuid::nil()),
        },
    ];

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors.is_empty(),
        "valid sequence should have no errors: {errors:?}"
    );
}
