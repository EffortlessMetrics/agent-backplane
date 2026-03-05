#![allow(clippy::useless_vec)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for abp-sidecar-proto covering envelope types,
//! JSONL parsing, serialization roundtrips, ref_id correlation,
//! version negotiation, error handling, and edge cases.

use abp_core::*;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_sidecar_proto::*;
use async_trait::async_trait;
use chrono::Utc;
use std::io::BufReader;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn identity(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.3.0".into()),
    }
}

fn default_identity() -> BackendIdentity {
    identity("comprehensive-test-sidecar")
}

fn caps() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m
}

fn empty_caps() -> CapabilityManifest {
    CapabilityManifest::new()
}

fn wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn default_wo() -> WorkOrder {
    wo("comprehensive test")
}

fn receipt(run_id: Uuid) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 50,
        },
        backend: default_identity(),
        capabilities: caps(),
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

fn ev(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn started() -> AgentEvent {
    ev(AgentEventKind::RunStarted {
        message: "started".into(),
    })
}

fn completed() -> AgentEvent {
    ev(AgentEventKind::RunCompleted {
        message: "done".into(),
    })
}

fn delta(text: &str) -> AgentEvent {
    ev(AgentEventKind::AssistantDelta { text: text.into() })
}

fn message(text: &str) -> AgentEvent {
    ev(AgentEventKind::AssistantMessage { text: text.into() })
}

fn tool_call(name: &str) -> AgentEvent {
    ev(AgentEventKind::ToolCall {
        tool_name: name.into(),
        tool_use_id: Some(format!("tc-{name}")),
        parent_tool_use_id: None,
        input: serde_json::json!({"arg": "value"}),
    })
}

fn tool_result(name: &str) -> AgentEvent {
    ev(AgentEventKind::ToolResult {
        tool_name: name.into(),
        tool_use_id: Some(format!("tr-{name}")),
        output: serde_json::json!({"ok": true}),
        is_error: false,
    })
}

fn file_changed(path: &str) -> AgentEvent {
    ev(AgentEventKind::FileChanged {
        path: path.into(),
        summary: "modified".into(),
    })
}

fn warning(msg: &str) -> AgentEvent {
    ev(AgentEventKind::Warning {
        message: msg.into(),
    })
}

fn error_event(msg: &str) -> AgentEvent {
    ev(AgentEventKind::Error {
        message: msg.into(),
        error_code: None,
    })
}

fn cmd_event(cmd: &str) -> AgentEvent {
    ev(AgentEventKind::CommandExecuted {
        command: cmd.into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    })
}

async fn drain(mut r: tokio::io::DuplexStream) -> String {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).await.unwrap();
    String::from_utf8(buf).unwrap()
}

fn decode_all(text: &str) -> Vec<Envelope> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| JsonlCodec::decode(l.trim()).unwrap())
        .collect()
}

fn run_input(run_id: &str, work_order: &WorkOrder) -> Vec<u8> {
    let env = Envelope::Run {
        id: run_id.into(),
        work_order: work_order.clone(),
    };
    JsonlCodec::encode(&env).unwrap().into_bytes()
}

// ===========================================================================
// Test handlers
// ===========================================================================

struct EchoHandler;

#[async_trait]
impl SidecarHandler for EchoHandler {
    async fn on_run(
        &self,
        _run_id: String,
        w: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        sender.send_event(message(&w.task)).await?;
        sender.send_final(receipt(w.id)).await?;
        Ok(())
    }
}

struct NEventHandler(usize);

#[async_trait]
impl SidecarHandler for NEventHandler {
    async fn on_run(
        &self,
        _run_id: String,
        w: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        for i in 0..self.0 {
            sender.send_event(delta(&format!("tok-{i}"))).await?;
        }
        sender.send_final(receipt(w.id)).await?;
        Ok(())
    }
}

struct NoopHandler;

#[async_trait]
impl SidecarHandler for NoopHandler {
    async fn on_run(
        &self,
        _run_id: String,
        w: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        sender.send_final(receipt(w.id)).await?;
        Ok(())
    }
}

struct FailHandler(String);

#[async_trait]
impl SidecarHandler for FailHandler {
    async fn on_run(
        &self,
        _run_id: String,
        _w: WorkOrder,
        _sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        Err(SidecarProtoError::Handler(self.0.clone()))
    }
}

struct MixedEventHandler;

#[async_trait]
impl SidecarHandler for MixedEventHandler {
    async fn on_run(
        &self,
        _run_id: String,
        w: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        sender.send_event(started()).await?;
        sender.send_event(tool_call("read_file")).await?;
        sender.send_event(tool_result("read_file")).await?;
        sender.send_event(file_changed("/src/main.rs")).await?;
        sender.send_event(delta("working...")).await?;
        sender.send_event(message("done processing")).await?;
        sender.send_event(completed()).await?;
        sender.send_final(receipt(w.id)).await?;
        Ok(())
    }
}

struct FatalAfterEventsHandler;

#[async_trait]
impl SidecarHandler for FatalAfterEventsHandler {
    async fn on_run(
        &self,
        _run_id: String,
        _w: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        sender.send_event(started()).await?;
        sender.send_event(delta("partial")).await?;
        Err(SidecarProtoError::Handler("mid-stream failure".into()))
    }
}

// ===========================================================================
// 1. HELLO ENVELOPE TESTS
// ===========================================================================

#[tokio::test]
async fn hello_has_correct_tag() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""t":"hello""#));
}

#[tokio::test]
async fn hello_contains_contract_version() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(CONTRACT_VERSION));
}

#[tokio::test]
async fn hello_contains_backend_identity() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains("comprehensive-test-sidecar"));
    assert!(text.contains("1.0.0"));
}

#[tokio::test]
async fn hello_roundtrip_preserves_fields() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let line = drain(r).await;
    let env = JsonlCodec::decode(line.trim()).unwrap();
    match env {
        Envelope::Hello {
            backend,
            contract_version,
            capabilities,
            mode,
        } => {
            assert_eq!(backend.id, "comprehensive-test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("0.3.0"));
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Emulated)
            ));
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_with_empty_capabilities() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), empty_caps())
        .await
        .unwrap();
    drop(w);
    let line = drain(r).await;
    let env = JsonlCodec::decode(line.trim()).unwrap();
    match env {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_with_minimal_identity() {
    let minimal = BackendIdentity {
        id: "bare".into(),
        backend_version: None,
        adapter_version: None,
    };
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, minimal, empty_caps()).await.unwrap();
    drop(w);
    let line = drain(r).await;
    let env = JsonlCodec::decode(line.trim()).unwrap();
    match env {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "bare");
            assert!(backend.backend_version.is_none());
            assert!(backend.adapter_version.is_none());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_ends_with_newline() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.ends_with('\n'));
}

#[tokio::test]
async fn hello_is_single_line() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert_eq!(text.lines().count(), 1);
}

#[tokio::test]
async fn hello_json_fields_present() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(v["t"], "hello");
    assert!(v.get("contract_version").is_some());
    assert!(v.get("backend").is_some());
    assert!(v.get("capabilities").is_some());
    assert!(v["backend"].get("id").is_some());
}

// ===========================================================================
// 2. RUN ENVELOPE TESTS
// ===========================================================================

#[tokio::test]
async fn run_envelope_tag() {
    let json = JsonlCodec::encode(&Envelope::Run {
        id: "r1".into(),
        work_order: default_wo(),
    })
    .unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[tokio::test]
async fn run_envelope_roundtrip() {
    let original_wo = default_wo();
    let json = JsonlCodec::encode(&Envelope::Run {
        id: "r-rt".into(),
        work_order: original_wo.clone(),
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "r-rt");
            assert_eq!(work_order.task, "comprehensive test");
            assert_eq!(work_order.id, original_wo.id);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[tokio::test]
async fn run_preserves_work_order_task() {
    let w = wo("special task with unicode: 日本語");
    let bytes = run_input("r-unicode", &w);
    let env = JsonlCodec::decode(std::str::from_utf8(&bytes).unwrap().trim()).unwrap();
    match env {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "special task with unicode: 日本語");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[tokio::test]
async fn run_preserves_work_order_lane() {
    let w = WorkOrderBuilder::new("lane test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let json = JsonlCodec::encode(&Envelope::Run {
        id: "r-lane".into(),
        work_order: w,
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Run { work_order, .. } => {
            assert!(matches!(work_order.lane, ExecutionLane::WorkspaceFirst));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ===========================================================================
// 3. EVENT ENVELOPE TESTS
// ===========================================================================

#[tokio::test]
async fn event_envelope_tag() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", started()).await.unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""t":"event""#));
}

#[tokio::test]
async fn event_preserves_ref_id() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "my-unique-ref-id", started())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""ref_id":"my-unique-ref-id""#));
}

#[tokio::test]
async fn event_roundtrip_run_started() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", started()).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_assistant_delta() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", delta("hello world"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello world"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_tool_call() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", tool_call("read_file"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-read_file"));
                assert_eq!(input["arg"], "value");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_tool_result() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", tool_result("write_file"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                output,
                ..
            } => {
                assert_eq!(tool_name, "write_file");
                assert!(!is_error);
                assert_eq!(output["ok"], true);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_file_changed() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", file_changed("/tmp/test.rs"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "/tmp/test.rs");
                assert_eq!(summary, "modified");
            }
            other => panic!("expected FileChanged, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_command_executed() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", cmd_event("cargo test"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
                assert_eq!(output_preview.as_deref(), Some("ok"));
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_warning() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", warning("budget low"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "budget low"),
            other => panic!("expected Warning, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_error() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", error_event("something failed"))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { message, .. } => assert_eq!(message, "something failed"),
            other => panic!("expected Error, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_roundtrip_run_completed() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", completed()).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_with_ext_data() {
    let mut ext = std::collections::BTreeMap::new();
    ext.insert("custom_key".into(), serde_json::json!("custom_value"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "with ext".into(),
        },
        ext: Some(ext),
    };
    let (mut w, r) = tokio::io::duplex(4096);
    send_event(&mut w, "r1", event).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert_eq!(ext["custom_key"], "custom_value");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// ===========================================================================
// 4. FINAL ENVELOPE TESTS
// ===========================================================================

#[tokio::test]
async fn final_envelope_tag() {
    let (mut w, r) = tokio::io::duplex(8192);
    send_final(&mut w, "r1", receipt(Uuid::nil()))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""t":"final""#));
}

#[tokio::test]
async fn final_preserves_ref_id() {
    let (mut w, r) = tokio::io::duplex(8192);
    send_final(&mut w, "unique-final-ref", receipt(Uuid::nil()))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""ref_id":"unique-final-ref""#));
}

#[tokio::test]
async fn final_roundtrip_preserves_receipt() {
    let run_id = Uuid::new_v4();
    let (mut w, r) = tokio::io::duplex(8192);
    send_final(&mut w, "r1", receipt(run_id)).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Final {
            ref_id,
            receipt: rcpt,
        } => {
            assert_eq!(ref_id, "r1");
            assert_eq!(rcpt.meta.run_id, run_id);
            assert_eq!(rcpt.outcome, Outcome::Complete);
            assert_eq!(rcpt.meta.contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn final_with_partial_outcome() {
    let mut r = receipt(Uuid::nil());
    r.outcome = Outcome::Partial;
    let json = JsonlCodec::encode(&Envelope::Final {
        ref_id: "r1".into(),
        receipt: r,
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Final { receipt: rcpt, .. } => {
            assert_eq!(rcpt.outcome, Outcome::Partial);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn final_with_failed_outcome() {
    let mut r = receipt(Uuid::nil());
    r.outcome = Outcome::Failed;
    let json = JsonlCodec::encode(&Envelope::Final {
        ref_id: "r1".into(),
        receipt: r,
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Final { receipt: rcpt, .. } => {
            assert_eq!(rcpt.outcome, Outcome::Failed);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn final_with_artifacts() {
    let mut r = receipt(Uuid::nil());
    r.artifacts = vec![ArtifactRef {
        kind: "patch".into(),
        path: "/tmp/changes.patch".into(),
    }];
    let json = JsonlCodec::encode(&Envelope::Final {
        ref_id: "r1".into(),
        receipt: r,
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Final { receipt: rcpt, .. } => {
            assert_eq!(rcpt.artifacts.len(), 1);
            assert_eq!(rcpt.artifacts[0].kind, "patch");
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn final_with_usage_data() {
    let mut r = receipt(Uuid::nil());
    r.usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.05),
    };
    let json = JsonlCodec::encode(&Envelope::Final {
        ref_id: "r1".into(),
        receipt: r,
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Final { receipt: rcpt, .. } => {
            assert_eq!(rcpt.usage.input_tokens, Some(100));
            assert_eq!(rcpt.usage.output_tokens, Some(200));
            assert_eq!(rcpt.usage.estimated_cost_usd, Some(0.05));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// ===========================================================================
// 5. FATAL ENVELOPE TESTS
// ===========================================================================

#[tokio::test]
async fn fatal_envelope_tag() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, Some("r1".into()), "error")
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""t":"fatal""#));
}

#[tokio::test]
async fn fatal_with_ref_id() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, Some("run-x".into()), "boom")
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-x".into()));
            assert_eq!(error, "boom");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn fatal_without_ref_id() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, None, "early crash").await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early crash");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn fatal_null_ref_id_in_json() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, None, "err").await.unwrap();
    drop(w);
    let text = drain(r).await;
    assert!(text.contains(r#""ref_id":null"#));
}

#[tokio::test]
async fn fatal_error_code_omitted_when_none() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, None, "err").await.unwrap();
    drop(w);
    let text = drain(r).await;
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert!(v.get("error_code").is_none());
}

#[tokio::test]
async fn fatal_roundtrip() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, Some("r1".into()), "test error")
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id, Some("r1".into()));
            assert_eq!(error, "test error");
            assert!(error_code.is_none());
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn fatal_with_special_chars_in_error() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, None, "error with \"quotes\" and \nnewlines")
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("quotes"));
            assert!(error.contains("newlines"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ===========================================================================
// 6. JSONL PARSING TESTS
// ===========================================================================

#[tokio::test]
async fn decode_single_line() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"test"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[tokio::test]
async fn decode_stream_multiple_lines() {
    let (mut w, r) = tokio::io::duplex(16384);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    send_event(&mut w, "r1", started()).await.unwrap();
    send_event(&mut w, "r1", delta("hello")).await.unwrap();
    send_final(&mut w, "r1", receipt(Uuid::nil()))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let reader = BufReader::new(text.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 4);
}

#[tokio::test]
async fn decode_stream_skips_empty_lines() {
    let input = format!(
        "\n  \n{}\n\n{}\n",
        r#"{"t":"fatal","ref_id":null,"error":"a"}"#, r#"{"t":"fatal","ref_id":null,"error":"b"}"#,
    );
    let reader = BufReader::new(input.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 2);
}

#[tokio::test]
async fn decode_fails_on_invalid_json() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[tokio::test]
async fn decode_fails_on_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[tokio::test]
async fn decode_fails_on_valid_json_wrong_schema() {
    let result = JsonlCodec::decode(r#"{"type":"hello"}"#);
    assert!(result.is_err());
}

#[tokio::test]
async fn decode_fails_missing_discriminator() {
    let result = JsonlCodec::decode(r#"{"ref_id":"r1","error":"boom"}"#);
    assert!(result.is_err());
}

#[tokio::test]
async fn decode_fails_wrong_discriminator_field() {
    // Uses "type" instead of "t"
    let result = JsonlCodec::decode(r#"{"type":"fatal","ref_id":null,"error":"boom"}"#);
    assert!(result.is_err());
}

#[tokio::test]
async fn decode_ignores_unknown_fields_in_envelope() {
    // Unknown fields should be ignored by serde
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom","unknown_field":"val"}"#;
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[tokio::test]
async fn encode_produces_newline_terminated() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.ends_with('\n'));
    assert_eq!(json.matches('\n').count(), 1);
}

#[tokio::test]
async fn encode_decode_is_identity_for_fatal() {
    let original = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "test".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("r1".into()));
            assert_eq!(error, "test");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ===========================================================================
// 7. REF_ID CORRELATION TESTS
// ===========================================================================

#[tokio::test]
async fn event_sender_ref_id_accessor() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "my-run-id");
    assert_eq!(sender.ref_id(), "my-run-id");
}

#[tokio::test]
async fn event_sender_ref_id_propagated_to_event_envelope() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run-42");
    sender.send_event(started()).await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn event_sender_ref_id_propagated_to_final_envelope() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run-42");
    sender.send_final(receipt(Uuid::nil())).await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn event_sender_ref_id_propagated_to_fatal_envelope() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run-42");
    sender.send_fatal("boom").await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id, Some("run-42".into())),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn all_envelopes_in_server_output_share_ref_id() {
    let w = default_wo();
    let input = run_input("consistent-ref", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(NEventHandler(3), default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    for env in &envs {
        match env {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "consistent-ref"),
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "consistent-ref"),
            Envelope::Hello { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}

#[tokio::test]
async fn ref_id_with_special_characters() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "run/with:special-chars_123");
    sender.send_event(started()).await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run/with:special-chars_123"),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn ref_id_with_uuid_format() {
    let uid = Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, uid.clone());
    sender.send_event(started()).await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, uid),
        other => panic!("expected Event, got {other:?}"),
    }
}

// ===========================================================================
// 8. VERSION NEGOTIATION TESTS
// ===========================================================================

#[tokio::test]
async fn hello_uses_current_contract_version() {
    let env = Envelope::hello(default_identity(), caps());
    match env {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, CONTRACT_VERSION),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn version_parsing_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(abp_protocol::parse_version("abp/v99.100"), Some((99, 100)));
}

#[tokio::test]
async fn version_parsing_invalid() {
    assert_eq!(abp_protocol::parse_version("invalid"), None);
    assert_eq!(abp_protocol::parse_version(""), None);
    assert_eq!(abp_protocol::parse_version("abp/v"), None);
    assert_eq!(abp_protocol::parse_version("abp/v1"), None);
    assert_eq!(abp_protocol::parse_version("v0.1"), None);
    assert_eq!(abp_protocol::parse_version("abp/x0.1"), None);
}

#[tokio::test]
async fn version_compatibility_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.99"));
    assert!(abp_protocol::is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[tokio::test]
async fn version_incompatible_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!abp_protocol::is_compatible_version("abp/v2.0", "abp/v1.0"));
}

#[tokio::test]
async fn version_incompatible_invalid_strings() {
    assert!(!abp_protocol::is_compatible_version("invalid", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v0.1", ""));
    assert!(!abp_protocol::is_compatible_version("", ""));
}

#[tokio::test]
async fn hello_version_is_compatible_with_contract() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert!(abp_protocol::is_compatible_version(
                &contract_version,
                CONTRACT_VERSION
            ));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ===========================================================================
// 9. INVALID/MALFORMED JSONL HANDLING
// ===========================================================================

#[tokio::test]
async fn server_rejects_plain_text() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(b"hello world\n" as &[u8], &mut w).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_rejects_truncated_json() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server
        .run_with_io(b"{\"t\":\"run\",\"id\":\n" as &[u8], &mut w)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_rejects_array_json() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(b"[1, 2, 3]\n" as &[u8], &mut w).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_rejects_null_json() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(b"null\n" as &[u8], &mut w).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_rejects_json_with_wrong_tag_value() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server
        .run_with_io(b"{\"t\":\"unknown_variant\"}\n" as &[u8], &mut w)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn decode_stream_reports_error_on_malformed_line() {
    let input = format!(
        "{}\nnot json\n",
        r#"{"t":"fatal","ref_id":null,"error":"ok"}"#,
    );
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

// ===========================================================================
// 10. EDGE CASES
// ===========================================================================

#[tokio::test]
async fn server_handles_empty_stdin() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(b"" as &[u8], &mut w).await;
    assert!(matches!(
        result.unwrap_err(),
        SidecarProtoError::StdinClosed
    ));
}

#[tokio::test]
async fn server_skips_blank_lines_before_run() {
    let w = default_wo();
    let run_line = JsonlCodec::encode(&Envelope::Run {
        id: "r-blank".into(),
        work_order: w,
    })
    .unwrap();
    let input = format!("\n  \n\t\n{run_line}");
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    server.run_with_io(input.as_bytes(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    assert!(text.contains(r#""t":"hello""#));
    assert!(text.contains(r#""t":"final""#));
}

#[tokio::test]
async fn server_only_blank_lines_returns_stdin_closed() {
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(b"\n\n  \n\n" as &[u8], &mut w).await;
    assert!(matches!(
        result.unwrap_err(),
        SidecarProtoError::StdinClosed
    ));
}

#[tokio::test]
async fn server_rejects_event_as_first_non_blank() {
    let line = JsonlCodec::encode(&Envelope::Event {
        ref_id: "r1".into(),
        event: started(),
    })
    .unwrap();
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(line.as_bytes(), &mut w).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("unexpected message"));
}

#[tokio::test]
async fn server_rejects_final_as_first_non_blank() {
    let line = JsonlCodec::encode(&Envelope::Final {
        ref_id: "r1".into(),
        receipt: receipt(Uuid::nil()),
    })
    .unwrap();
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(line.as_bytes(), &mut w).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_rejects_fatal_as_first_non_blank() {
    let line = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    })
    .unwrap();
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(line.as_bytes(), &mut w).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn server_rejects_hello_as_input() {
    let line = JsonlCodec::encode(&Envelope::hello(default_identity(), caps())).unwrap();
    let (mut w, _r) = tokio::io::duplex(4096);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let result = server.run_with_io(line.as_bytes(), &mut w).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("unexpected message"));
}

#[tokio::test]
async fn event_sender_on_closed_channel_event() {
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "r1");
    drop(rx);
    assert!(matches!(
        sender.send_event(started()).await.unwrap_err(),
        SidecarProtoError::ChannelClosed
    ));
}

#[tokio::test]
async fn event_sender_on_closed_channel_final() {
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "r1");
    drop(rx);
    assert!(matches!(
        sender.send_final(receipt(Uuid::nil())).await.unwrap_err(),
        SidecarProtoError::ChannelClosed
    ));
}

#[tokio::test]
async fn event_sender_on_closed_channel_fatal() {
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "r1");
    drop(rx);
    assert!(matches!(
        sender.send_fatal("err").await.unwrap_err(),
        SidecarProtoError::ChannelClosed
    ));
}

#[tokio::test]
async fn event_sender_clone_shares_channel() {
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "r1");
    let clone = sender.clone();
    sender.send_event(started()).await.unwrap();
    clone.send_event(completed()).await.unwrap();
    assert_eq!(rx.len(), 2);
}

#[tokio::test]
async fn empty_error_message_in_fatal() {
    let (mut w, r) = tokio::io::duplex(4096);
    send_fatal(&mut w, None, "").await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error, ""),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn long_error_message_in_fatal() {
    let long_msg = "x".repeat(10_000);
    let (mut w, r) = tokio::io::duplex(65536);
    send_fatal(&mut w, None, &long_msg).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 10_000),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn empty_ref_id_string() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "");
    assert_eq!(sender.ref_id(), "");
    sender.send_event(started()).await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, ""),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn unicode_in_ref_id() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "日本語-run-🚀");
    sender.send_event(started()).await.unwrap();
    let env = rx.try_recv().unwrap();
    match env {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "日本語-run-🚀"),
        other => panic!("expected Event, got {other:?}"),
    }
}

// ===========================================================================
// 11. PROTOCOL SEQUENCE VALIDATION TESTS
// ===========================================================================

#[tokio::test]
async fn server_full_sequence_hello_events_final() {
    let w = default_wo();
    let input = run_input("r-full", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(MixedEventHandler, default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    // hello + 7 events + final = 9
    assert_eq!(envs.len(), 9);
    assert!(matches!(envs[0], Envelope::Hello { .. }));
    for env in &envs[1..8] {
        assert!(matches!(env, Envelope::Event { .. }));
    }
    assert!(matches!(envs[8], Envelope::Final { .. }));
}

#[tokio::test]
async fn server_hello_always_first() {
    let w = default_wo();
    let input = run_input("r-h1st", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(EchoHandler, default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    assert!(matches!(envs[0], Envelope::Hello { .. }));
}

#[tokio::test]
async fn server_final_always_last_on_success() {
    let w = default_wo();
    let input = run_input("r-last", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(NEventHandler(4), default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    assert!(matches!(envs.last().unwrap(), Envelope::Final { .. }));
}

#[tokio::test]
async fn server_fatal_last_on_handler_error() {
    let w = default_wo();
    let input = run_input("r-fail", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(
        FailHandler("intentional".into()),
        default_identity(),
        caps(),
    );
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    assert_eq!(envs.len(), 2);
    assert!(matches!(envs[0], Envelope::Hello { .. }));
    match &envs[1] {
        Envelope::Fatal { error, .. } => assert!(error.contains("intentional")),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn server_drains_events_before_fatal() {
    let w = default_wo();
    let input = run_input("r-drain", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(FatalAfterEventsHandler, default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    // hello + 2 events + fatal = 4
    assert_eq!(envs.len(), 4);
    assert!(matches!(envs[0], Envelope::Hello { .. }));
    assert!(matches!(envs[1], Envelope::Event { .. }));
    assert!(matches!(envs[2], Envelope::Event { .. }));
    assert!(matches!(envs[3], Envelope::Fatal { .. }));
}

#[tokio::test]
async fn server_no_events_just_final() {
    let w = default_wo();
    let input = run_input("r-noev", &w);
    let (mut wr, r) = tokio::io::duplex(16384);
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    assert_eq!(envs.len(), 2);
    assert!(matches!(envs[0], Envelope::Hello { .. }));
    assert!(matches!(envs[1], Envelope::Final { .. }));
}

#[tokio::test]
async fn validator_accepts_valid_sequence() {
    let run_id = "r-valid";
    let seq = vec![
        Envelope::hello(default_identity(), caps()),
        Envelope::Run {
            id: run_id.into(),
            work_order: default_wo(),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: started(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: receipt(Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "should be valid: {errors:?}");
}

#[tokio::test]
async fn validator_detects_missing_hello() {
    let run_id = "r-no-hello";
    let seq = vec![
        Envelope::Run {
            id: run_id.into(),
            work_order: default_wo(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: receipt(Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[tokio::test]
async fn validator_detects_hello_not_first() {
    let run_id = "r-hello-late";
    let seq = vec![
        Envelope::Run {
            id: run_id.into(),
            work_order: default_wo(),
        },
        Envelope::hello(default_identity(), caps()),
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: receipt(Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&seq);
    assert!(
        errors.contains(&SequenceError::HelloNotFirst { position: 1 }),
        "errors: {errors:?}"
    );
}

#[tokio::test]
async fn validator_detects_double_terminal() {
    let run_id = "r-dbl";
    let seq = vec![
        Envelope::hello(default_identity(), caps()),
        Envelope::Run {
            id: run_id.into(),
            work_order: default_wo(),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: receipt(Uuid::nil()),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: receipt(Uuid::nil()),
        },
    ];
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

// ===========================================================================
// 12. MULTI-LINE STREAM TESTS
// ===========================================================================

#[tokio::test]
async fn multiple_events_streamed_in_order() {
    let (mut w, r) = tokio::io::duplex(8192);
    for i in 0..10 {
        send_event(&mut w, "r1", delta(&format!("word-{i}")))
            .await
            .unwrap();
    }
    drop(w);
    let text = drain(r).await;
    let envs = decode_all(&text);
    assert_eq!(envs.len(), 10);
    for (i, env) in envs.iter().enumerate() {
        match env {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::AssistantDelta { text } => {
                    assert_eq!(text, &format!("word-{i}"));
                }
                other => panic!("expected AssistantDelta, got {other:?}"),
            },
            other => panic!("expected Event, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn decode_stream_from_concatenated_output() {
    let (mut w, r) = tokio::io::duplex(16384);
    send_hello(&mut w, default_identity(), caps())
        .await
        .unwrap();
    for i in 0..5 {
        send_event(&mut w, "r1", delta(&format!("tok-{i}")))
            .await
            .unwrap();
    }
    send_final(&mut w, "r1", receipt(Uuid::nil()))
        .await
        .unwrap();
    drop(w);
    let text = drain(r).await;
    let reader = BufReader::new(text.as_bytes());
    let envs: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envs.len(), 7); // hello + 5 events + final
}

#[tokio::test]
async fn write_many_envelopes_to_writer() {
    let envs = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "c".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let reader = BufReader::new(text.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

// ===========================================================================
// 13. LARGE PAYLOAD HANDLING
// ===========================================================================

#[tokio::test]
async fn large_event_payload() {
    let big_text = "A".repeat(100_000);
    let (mut w, r) = tokio::io::duplex(256 * 1024);
    send_event(&mut w, "r1", message(&big_text)).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    match env {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 100_000),
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[tokio::test]
async fn large_tool_result_payload() {
    let big_json = serde_json::json!({
        "data": "X".repeat(50_000),
        "nested": {"a": 1, "b": 2},
    });
    let event = ev(AgentEventKind::ToolResult {
        tool_name: "big_tool".into(),
        tool_use_id: None,
        output: big_json,
        is_error: false,
    });
    let (mut w, r) = tokio::io::duplex(256 * 1024);
    send_event(&mut w, "r1", event).await.unwrap();
    drop(w);
    let text = drain(r).await;
    let env = JsonlCodec::decode(text.trim()).unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
}

#[tokio::test]
async fn many_events_server_stress() {
    let w = default_wo();
    let input = run_input("r-stress", &w);
    let (mut wr, r) = tokio::io::duplex(1024 * 1024);
    let server = SidecarServer::new(NEventHandler(100), default_identity(), caps());
    server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
    drop(wr);
    let text = drain(r).await;
    let envs = decode_all(&text);
    // hello + 100 events + final = 102
    assert_eq!(envs.len(), 102);
}

// ===========================================================================
// 14. ERROR TYPE TESTS
// ===========================================================================

#[tokio::test]
async fn error_display_handler() {
    let e = SidecarProtoError::Handler("oops".into());
    assert_eq!(e.to_string(), "handler error: oops");
}

#[tokio::test]
async fn error_display_stdin_closed() {
    let e = SidecarProtoError::StdinClosed;
    assert_eq!(e.to_string(), "stdin closed unexpectedly");
}

#[tokio::test]
async fn error_display_channel_closed() {
    let e = SidecarProtoError::ChannelClosed;
    assert_eq!(e.to_string(), "event channel closed");
}

#[tokio::test]
async fn error_display_unexpected_message() {
    let e = SidecarProtoError::UnexpectedMessage {
        expected: "run".into(),
        got: "hello".into(),
    };
    assert!(e.to_string().contains("unexpected message"));
    assert!(e.to_string().contains("run"));
    assert!(e.to_string().contains("hello"));
}

#[tokio::test]
async fn protocol_error_json_variant() {
    let result = JsonlCodec::decode("not json");
    let err = result.unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(err.to_string().contains("invalid JSON"));
}

// ===========================================================================
// 15. SIDECAR SERVER MISC
// ===========================================================================

#[tokio::test]
async fn server_debug_impl() {
    let server = SidecarServer::new(NoopHandler, default_identity(), caps());
    let debug = format!("{server:?}");
    assert!(debug.contains("SidecarServer"));
    assert!(debug.contains("comprehensive-test-sidecar"));
}

#[tokio::test]
async fn event_sender_debug_impl() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let sender = EventSender::new(tx, "r-dbg");
    let debug = format!("{sender:?}");
    assert!(debug.contains("EventSender"));
    assert!(debug.contains("r-dbg"));
}

#[tokio::test]
async fn default_on_cancel_succeeds() {
    let handler = EchoHandler;
    handler.on_cancel().await.unwrap();
}

#[tokio::test]
async fn server_with_different_identities() {
    for id in &["sidecar-a", "sidecar-b", "sidecar-c"] {
        let w = default_wo();
        let input = run_input(&format!("r-{id}"), &w);
        let (mut wr, r) = tokio::io::duplex(16384);
        let server = SidecarServer::new(NoopHandler, identity(id), caps());
        server.run_with_io(input.as_slice(), &mut wr).await.unwrap();
        drop(wr);
        let text = drain(r).await;
        assert!(text.contains(id));
    }
}

// ===========================================================================
// 16. SERIALIZATION ROUNDTRIP ACROSS ALL ENVELOPE TYPES
// ===========================================================================

#[tokio::test]
async fn roundtrip_all_envelope_types_via_codec() {
    let envelopes = vec![
        Envelope::hello(default_identity(), caps()),
        Envelope::Run {
            id: "r-rt-all".into(),
            work_order: default_wo(),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: started(),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: delta("tok"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: tool_call("read_file"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: tool_result("read_file"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: file_changed("/src/lib.rs"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: cmd_event("ls"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: warning("warn"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: error_event("err"),
        },
        Envelope::Event {
            ref_id: "r-rt-all".into(),
            event: completed(),
        },
        Envelope::Final {
            ref_id: "r-rt-all".into(),
            receipt: receipt(Uuid::nil()),
        },
        Envelope::Fatal {
            ref_id: Some("r-rt-all".into()),
            error: "boom".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "early".into(),
            error_code: None,
        },
    ];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let reader = BufReader::new(text.as_bytes());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), envelopes.len());

    // Verify discriminator tags match
    for (orig, dec) in envelopes.iter().zip(decoded.iter()) {
        let orig_val = serde_json::to_value(orig).unwrap();
        let dec_val = serde_json::to_value(dec).unwrap();
        assert_eq!(orig_val["t"], dec_val["t"]);
    }
}

#[tokio::test]
async fn encode_to_writer_roundtrip() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: delta("test"),
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let decoded = JsonlCodec::decode(text.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

// ===========================================================================
// 17. EXECUTION MODE TESTS
// ===========================================================================

#[tokio::test]
async fn hello_default_mode_is_mapped() {
    let env = Envelope::hello(default_identity(), caps());
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_with_passthrough_mode() {
    let env = Envelope::hello_with_mode(default_identity(), caps(), ExecutionMode::Passthrough);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ===========================================================================
// 18. DECODE RAW JSON STRINGS
// ===========================================================================

#[tokio::test]
async fn decode_raw_hello_json() {
    let json = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"raw"}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION
    );
    let env = JsonlCodec::decode(&json).unwrap();
    match env {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "raw"),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn decode_raw_fatal_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"raw boom"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error, "raw boom"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn decode_fatal_with_extra_fields() {
    let json = r#"{"t":"fatal","ref_id":"r1","error":"err","extra1":"val","extra2":42}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

// ===========================================================================
// 19. WORK ORDER BUILDER INTEGRATION
// ===========================================================================

#[tokio::test]
async fn work_order_builder_through_run_envelope() {
    let w = WorkOrderBuilder::new("build test")
        .lane(ExecutionLane::WorkspaceFirst)
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(1.0)
        .build();
    let json = JsonlCodec::encode(&Envelope::Run {
        id: "r-builder".into(),
        work_order: w.clone(),
    })
    .unwrap();
    let env = JsonlCodec::decode(json.trim()).unwrap();
    match env {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "build test");
            assert!(matches!(work_order.lane, ExecutionLane::WorkspaceFirst));
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
            assert_eq!(work_order.config.max_turns, Some(10));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ===========================================================================
// 20. ENCODE / DECODE IDENTITY
// ===========================================================================

#[tokio::test]
async fn all_tags_use_t_not_type() {
    let tags = vec![
        (
            "hello",
            JsonlCodec::encode(&Envelope::hello(default_identity(), caps())).unwrap(),
        ),
        (
            "run",
            JsonlCodec::encode(&Envelope::Run {
                id: "r".into(),
                work_order: default_wo(),
            })
            .unwrap(),
        ),
        (
            "event",
            JsonlCodec::encode(&Envelope::Event {
                ref_id: "r".into(),
                event: started(),
            })
            .unwrap(),
        ),
        (
            "final",
            JsonlCodec::encode(&Envelope::Final {
                ref_id: "r".into(),
                receipt: receipt(Uuid::nil()),
            })
            .unwrap(),
        ),
        (
            "fatal",
            JsonlCodec::encode(&Envelope::Fatal {
                ref_id: None,
                error: "e".into(),
                error_code: None,
            })
            .unwrap(),
        ),
    ];
    for (expected_tag, json) in &tags {
        let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(
            v["t"].as_str().unwrap(),
            *expected_tag,
            "tag mismatch for {expected_tag}"
        );
        assert!(
            v.get("type").is_none(),
            "should not have 'type' field for {expected_tag}"
        );
    }
}
