#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

//! Comprehensive integration tests for the abp-host crate:
//! JSONL protocol encoding/decoding, protocol state machine validation,
//! sidecar lifecycle management, event stream processing, and SidecarBackend.

use std::collections::BTreeMap;
use std::io::{BufReader, Write};
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_host::health::{HealthCheck, HealthMonitor, HealthReport, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntry, PoolEntryState, PoolStats, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{compute_delay, is_retryable, RetryConfig};
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_protocol::builder::EnvelopeBuilder;
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionRange};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_backend_identity(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn make_capabilities() -> CapabilityManifest {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).root("/tmp/test").build()
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

fn make_hello_envelope() -> Envelope {
    Envelope::hello(make_backend_identity("test-sidecar"), make_capabilities())
}

fn make_run_envelope(run_id: &str, task: &str) -> Envelope {
    Envelope::Run {
        id: run_id.into(),
        work_order: make_work_order(task),
    }
}

fn make_event_envelope(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn make_final_envelope(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: make_receipt("test-sidecar"),
    }
}

fn make_fatal_envelope(ref_id: Option<&str>, msg: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: msg.into(),
        error_code: None,
    }
}

// ===========================================================================
// JSONL Protocol Frame encoding/decoding (20+ tests)
// ===========================================================================

#[test]
fn encode_hello_decode_roundtrip() {
    let hello = make_hello_envelope();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            assert!(capabilities.contains_key(&Capability::ToolRead));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn encode_hello_contains_tag() {
    let hello = make_hello_envelope();
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(r#""t":"hello""#));
}

#[test]
fn encode_run_decode_roundtrip() {
    let wo = make_work_order("test task");
    let run = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-001");
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn encode_run_contains_tag() {
    let run = make_run_envelope("r1", "hello");
    let line = JsonlCodec::encode(&run).unwrap();
    assert!(line.contains(r#""t":"run""#));
}

#[test]
fn encode_event_assistant_message_roundtrip() {
    let env = make_event_envelope(
        "run-001",
        AgentEventKind::AssistantMessage {
            text: "Hello world".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-001");
            match event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello world"),
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_assistant_delta_roundtrip() {
    let env = make_event_envelope(
        "run-002",
        AgentEventKind::AssistantDelta { text: "tok".into() },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "tok"),
            _ => panic!("expected AssistantDelta"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_tool_call_roundtrip() {
    let env = make_event_envelope(
        "run-003",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert_eq!(input["path"], "src/main.rs");
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_tool_result_roundtrip() {
    let env = make_event_envelope(
        "run-004",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                output,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!is_error);
                assert_eq!(output["content"], "fn main() {}");
            }
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_file_changed_roundtrip() {
    let env = make_event_envelope(
        "run-005",
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added new function".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(summary, "Added new function");
            }
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_warning_roundtrip() {
    let env = make_event_envelope(
        "run-006",
        AgentEventKind::Warning {
            message: "disk almost full".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "disk almost full"),
            _ => panic!("expected Warning"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_error_roundtrip() {
    let env = make_event_envelope(
        "run-007",
        AgentEventKind::Error {
            message: "out of memory".into(),
            error_code: None,
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { message, .. } => assert_eq!(message, "out of memory"),
            _ => panic!("expected Error"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_command_executed_roundtrip() {
    let env = make_event_envelope(
        "run-008",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
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
            _ => panic!("expected CommandExecuted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_run_started_roundtrip() {
    let env = make_event_envelope(
        "run-009",
        AgentEventKind::RunStarted {
            message: "starting run".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::RunStarted { message } => assert_eq!(message, "starting run"),
            _ => panic!("expected RunStarted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_event_run_completed_roundtrip() {
    let env = make_event_envelope(
        "run-010",
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::RunCompleted { message } => assert_eq!(message, "done"),
            _ => panic!("expected RunCompleted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_final_decode_roundtrip() {
    let env = make_final_envelope("run-001");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-001");
            assert_eq!(receipt.backend.id, "test-sidecar");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn encode_final_contains_tag() {
    let env = make_final_envelope("run-001");
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn encode_fatal_decode_roundtrip() {
    let env = make_fatal_envelope(Some("run-001"), "something broke");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-001"));
            assert_eq!(error, "something broke");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn encode_fatal_no_ref_id() {
    let env = make_fatal_envelope(None, "unrecoverable");
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "unrecoverable");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn multiple_envelopes_on_single_stream() {
    let envelopes = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_event_envelope("r1", AgentEventKind::AssistantMessage { text: "hi".into() }),
        make_final_envelope("r1"),
    ];
    let batch = StreamingCodec::encode_batch(&envelopes);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 4);
    for r in &results {
        assert!(r.is_ok());
    }
    assert!(matches!(
        results[0].as_ref().unwrap(),
        Envelope::Hello { .. }
    ));
    assert!(matches!(results[1].as_ref().unwrap(), Envelope::Run { .. }));
    assert!(matches!(
        results[2].as_ref().unwrap(),
        Envelope::Event { .. }
    ));
    assert!(matches!(
        results[3].as_ref().unwrap(),
        Envelope::Final { .. }
    ));
}

#[test]
fn invalid_json_line_returns_error() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn empty_json_object_returns_error() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn decode_stream_skips_blank_lines() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn very_large_envelope_roundtrip() {
    let large_text = "x".repeat(1_100_000); // >1MB
    let env = make_event_envelope(
        "run-big",
        AgentEventKind::AssistantMessage {
            text: large_text.clone(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.len() > 1_000_000);
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 1_100_000),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn encode_hello_with_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        make_backend_identity("pass-sidecar"),
        make_capabilities(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn encode_to_writer_works() {
    let mut buf = Vec::new();
    let env = make_fatal_envelope(None, "writer test");
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("writer test"));
}

#[test]
fn encode_many_to_writer_works() {
    let mut buf = Vec::new();
    let envs = vec![
        make_fatal_envelope(None, "a"),
        make_fatal_envelope(None, "b"),
    ];
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 2);
}

// ===========================================================================
// Protocol state machine (20+ tests)
// ===========================================================================

#[test]
fn valid_sequence_hello_run_events_final() {
    let validator = EnvelopeValidator::new();
    let run_id = "run-001";
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope(run_id, "task"),
        make_event_envelope(
            run_id,
            AgentEventKind::AssistantMessage { text: "hi".into() },
        ),
        make_final_envelope(run_id),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

#[test]
fn missing_hello_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![make_run_envelope("r1", "task"), make_final_envelope("r1")];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingHello)));
}

#[test]
fn event_before_run_is_error() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_event_envelope("r1", AgentEventKind::AssistantMessage { text: "hi".into() }),
        make_run_envelope("r1", "task"),
        make_final_envelope("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
}

#[test]
fn run_before_hello_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_run_envelope("r1", "task"),
        make_hello_envelope(),
        make_final_envelope("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })));
}

#[test]
fn double_terminal_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_final_envelope("r1"),
        make_final_envelope("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MultipleTerminals)));
}

#[test]
fn fatal_terminates_sequence_cleanly() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_fatal_envelope(Some("r1"), "crash"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

#[test]
fn ref_id_mismatch_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_event_envelope(
            "WRONG",
            AgentEventKind::AssistantMessage { text: "x".into() },
        ),
        make_final_envelope("r1"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn ref_id_mismatch_in_final_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_final_envelope("WRONG"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn empty_sequence_errors() {
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[]);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingHello)));
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingTerminal)));
}

#[test]
fn missing_terminal_detected() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_event_envelope("r1", AgentEventKind::AssistantMessage { text: "x".into() }),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingTerminal)));
}

#[test]
fn event_after_terminal_is_out_of_order() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_run_envelope("r1", "task"),
        make_final_envelope("r1"),
        make_event_envelope(
            "r1",
            AgentEventKind::AssistantMessage {
                text: "late".into(),
            },
        ),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
}

#[test]
fn validate_hello_with_empty_backend_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "backend.id")));
}

#[test]
fn validate_hello_with_invalid_version() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "invalid-version".into(),
        backend: make_backend_identity("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
}

#[test]
fn validate_hello_warns_missing_optional_fields() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version")));
}

#[test]
fn validate_run_with_empty_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Run {
        id: "".into(),
        work_order: make_work_order("task"),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_event_with_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Event {
        ref_id: "".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "x".into() },
            ext: None,
        },
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_fatal_with_empty_error() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "".into(),
        error_code: None,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn validate_final_with_empty_ref_id() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Final {
        ref_id: "".into(),
        receipt: make_receipt("test"),
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[test]
fn fatal_with_no_ref_id_warns() {
    let validator = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    let result = validator.validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(
        |w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "ref_id")
    ));
}

#[test]
fn sequence_with_only_hello_and_fatal() {
    let validator = EnvelopeValidator::new();
    let seq = vec![
        make_hello_envelope(),
        make_fatal_envelope(None, "early crash"),
    ];
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

// ===========================================================================
// Sidecar process lifecycle (20+ tests)
// ===========================================================================

#[test]
fn lifecycle_initial_state_is_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_transition_uninitialized_to_starting() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Starting);
}

#[test]
fn lifecycle_transition_starting_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_transition_ready_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Running);
}

#[test]
fn lifecycle_transition_running_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_transition_running_to_stopping() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopping);
}

#[test]
fn lifecycle_transition_stopping_to_stopped() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

#[test]
fn lifecycle_failed_from_any_state() {
    for initial in [
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
    ] {
        let mut mgr = LifecycleManager::new();
        // Drive to the target state step-by-step.
        let path = match &initial {
            LifecycleState::Uninitialized => vec![],
            LifecycleState::Starting => vec![LifecycleState::Starting],
            LifecycleState::Ready => vec![LifecycleState::Starting, LifecycleState::Ready],
            LifecycleState::Running => vec![
                LifecycleState::Starting,
                LifecycleState::Ready,
                LifecycleState::Running,
            ],
            LifecycleState::Stopping => vec![
                LifecycleState::Starting,
                LifecycleState::Ready,
                LifecycleState::Stopping,
            ],
            LifecycleState::Stopped => vec![
                LifecycleState::Starting,
                LifecycleState::Ready,
                LifecycleState::Stopping,
                LifecycleState::Stopped,
            ],
            _ => vec![],
        };
        for s in path {
            mgr.transition(s, None).unwrap();
        }
        assert_eq!(*mgr.state(), initial);
        mgr.transition(LifecycleState::Failed, Some("test".into()))
            .unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Failed);
    }
}

#[test]
fn lifecycle_invalid_transition_uninitialized_to_running() {
    let mut mgr = LifecycleManager::new();
    let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_invalid_transition_starting_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_already_in_state_error() {
    let mut mgr = LifecycleManager::new();
    let err = mgr
        .transition(LifecycleState::Uninitialized, None)
        .unwrap_err();
    assert!(matches!(
        err,
        LifecycleError::AlreadyInState(LifecycleState::Uninitialized)
    ));
}

#[test]
fn lifecycle_history_tracks_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, Some("handshake done".into()))
        .unwrap();
    let history = mgr.history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].from, LifecycleState::Uninitialized);
    assert_eq!(history[0].to, LifecycleState::Starting);
    assert_eq!(history[0].reason.as_deref(), Some("boot"));
    assert_eq!(history[1].from, LifecycleState::Starting);
    assert_eq!(history[1].to, LifecycleState::Ready);
}

#[test]
fn lifecycle_uptime_none_before_ready() {
    let mgr = LifecycleManager::new();
    assert!(mgr.uptime().is_none());
}

#[test]
fn lifecycle_uptime_some_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert!(mgr.uptime().is_some());
}

#[test]
fn lifecycle_can_transition_checks() {
    let mut mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Ready));
    assert!(!mgr.can_transition(&LifecycleState::Running));
    assert!(mgr.can_transition(&LifecycleState::Failed)); // always
}

#[test]
fn lifecycle_display_states() {
    assert_eq!(
        format!("{}", LifecycleState::Uninitialized),
        "uninitialized"
    );
    assert_eq!(format!("{}", LifecycleState::Starting), "starting");
    assert_eq!(format!("{}", LifecycleState::Ready), "ready");
    assert_eq!(format!("{}", LifecycleState::Running), "running");
    assert_eq!(format!("{}", LifecycleState::Stopping), "stopping");
    assert_eq!(format!("{}", LifecycleState::Stopped), "stopped");
    assert_eq!(format!("{}", LifecycleState::Failed), "failed");
}

#[test]
fn lifecycle_default_is_new() {
    let mgr = LifecycleManager::default();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_full_happy_path() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 6);
}

#[test]
fn lifecycle_stopped_to_starting_invalid() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    let err = mgr.transition(LifecycleState::Starting, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

// ===========================================================================
// Event stream processing (20+ tests)
// ===========================================================================

#[test]
fn stream_parser_new_is_empty() {
    let parser = StreamParser::new();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

#[test]
fn stream_parser_default_is_empty() {
    let parser = StreamParser::default();
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_partial_line_buffered() {
    let mut parser = StreamParser::new();
    let partial = br#"{"t":"fatal","ref_id":null,"#;
    let results = parser.push(partial);
    assert!(results.is_empty());
    assert!(!parser.is_empty());
    assert!(parser.buffered_len() > 0);
}

#[test]
fn stream_parser_complete_line() {
    let mut parser = StreamParser::new();
    let line = br#"{"t":"fatal","ref_id":null,"error":"boom"}
"#;
    let results = parser.push(line);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn stream_parser_two_chunks() {
    let mut parser = StreamParser::new();
    let full = JsonlCodec::encode(&make_fatal_envelope(None, "two-chunk")).unwrap();
    let bytes = full.as_bytes();
    let mid = bytes.len() / 2;
    let r1 = parser.push(&bytes[..mid]);
    assert!(r1.is_empty());
    let r2 = parser.push(&bytes[mid..]);
    assert_eq!(r2.len(), 1);
    assert!(r2[0].is_ok());
}

#[test]
fn stream_parser_multiple_lines_in_one_chunk() {
    let mut parser = StreamParser::new();
    let a = JsonlCodec::encode(&make_fatal_envelope(None, "a")).unwrap();
    let b = JsonlCodec::encode(&make_fatal_envelope(None, "b")).unwrap();
    let combined = format!("{}{}", a, b);
    let results = parser.push(combined.as_bytes());
    assert_eq!(results.len(), 2);
}

#[test]
fn stream_parser_blank_lines_skipped() {
    let mut parser = StreamParser::new();
    let input = b"\n\n\n";
    let results = parser.push(input);
    assert!(results.is_empty());
}

#[test]
fn stream_parser_invalid_json_returns_error() {
    let mut parser = StreamParser::new();
    let input = b"not json at all\n";
    let results = parser.push(input);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn stream_parser_finish_flushes_remaining() {
    let mut parser = StreamParser::new();
    // Push a line without trailing newline
    let env = make_fatal_envelope(None, "flush-test");
    let line = JsonlCodec::encode(&env).unwrap();
    let trimmed = line.trim_end();
    parser.push(trimmed.as_bytes());
    assert!(!parser.is_empty());
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_reset_clears_buffer() {
    let mut parser = StreamParser::new();
    parser.push(b"partial data");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
}

#[test]
fn stream_parser_max_line_len_exceeded() {
    let mut parser = StreamParser::with_max_line_len(50);
    let big_line = format!(
        "{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{}\"}}\n",
        "x".repeat(100)
    );
    let results = parser.push(big_line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn text_message_events_preserved_through_codec() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello! Special chars: \n\t\"\\".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: event.clone(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event {
            event: decoded_event,
            ..
        } => match (&event.kind, &decoded_event.kind) {
            (
                AgentEventKind::AssistantMessage { text: orig },
                AgentEventKind::AssistantMessage { text: dec },
            ) => assert_eq!(orig, dec),
            _ => panic!("kind mismatch"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn tool_call_events_preserved_through_codec() {
    let input_json = serde_json::json!({
        "path": "src/main.rs",
        "content": "fn main() { println!(\"hello\"); }"
    });
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu-42".into()),
            parent_tool_use_id: Some("tu-41".into()),
            input: input_json.clone(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                input,
            } => {
                assert_eq!(tool_name, "write_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-42"));
                assert_eq!(parent_tool_use_id.as_deref(), Some("tu-41"));
                assert_eq!(input, input_json);
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn tool_result_error_preserved() {
    let env = make_event_envelope(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!({"stderr": "command not found"}),
            is_error: true,
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn file_change_events_preserved() {
    let env = make_event_envelope(
        "r1",
        AgentEventKind::FileChanged {
            path: "deeply/nested/file.txt".into(),
            summary: "created with 100 lines".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "deeply/nested/file.txt");
                assert_eq!(summary, "created with 100 lines");
            }
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn multiple_events_rapid_succession_ordering() {
    let mut parser = StreamParser::new();
    let mut lines = String::new();
    for i in 0..50 {
        let env = make_event_envelope(
            "r1",
            AgentEventKind::AssistantDelta {
                text: format!("token_{}", i),
            },
        );
        lines.push_str(&JsonlCodec::encode(&env).unwrap());
    }
    let results = parser.push(lines.as_bytes());
    assert_eq!(results.len(), 50);
    for (i, r) in results.iter().enumerate() {
        let env = r.as_ref().unwrap();
        match env {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::AssistantDelta { text } => {
                    assert_eq!(text, &format!("token_{}", i));
                }
                _ => panic!("expected AssistantDelta"),
            },
            _ => panic!("expected Event"),
        }
    }
}

#[test]
fn event_with_extension_data_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor_field": 42}),
    );
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "passthrough".into(),
        },
        ext: Some(ext),
    };
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            let ext = event.ext.unwrap();
            assert_eq!(ext["raw_message"]["vendor_field"], 42);
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn streaming_codec_batch_line_count() {
    let envs = vec![
        make_fatal_envelope(None, "a"),
        make_fatal_envelope(None, "b"),
        make_fatal_envelope(None, "c"),
    ];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(StreamingCodec::line_count(&batch), 3);
}

#[test]
fn streaming_codec_validate_jsonl_catches_errors() {
    let good = JsonlCodec::encode(&make_fatal_envelope(None, "ok")).unwrap();
    let input = format!("{}bad json here\n{}", good, good);
    let errors = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 2); // line 2 is bad (1-based)
}

// ===========================================================================
// SidecarBackend / Registry / Configuration integration (20+ tests)
// ===========================================================================

#[test]
fn sidecar_spec_new() {
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_with_args() {
    let mut spec = SidecarSpec::new("python");
    spec.args = vec!["host.py".into()];
    assert_eq!(spec.args.len(), 1);
}

#[test]
fn sidecar_spec_with_env() {
    let mut spec = SidecarSpec::new("node");
    spec.env.insert("API_KEY".into(), "secret".into());
    assert_eq!(spec.env.get("API_KEY").unwrap(), "secret");
}

#[test]
fn sidecar_spec_with_cwd() {
    let mut spec = SidecarSpec::new("node");
    spec.cwd = Some("/tmp/work".into());
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn sidecar_spec_serialization_roundtrip() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["host.js".into()];
    spec.cwd = Some("/tmp".into());
    let json = serde_json::to_string(&spec).unwrap();
    let deserialized: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.command, "node");
    assert_eq!(deserialized.args, vec!["host.js"]);
    assert_eq!(deserialized.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn sidecar_config_new() {
    let config = SidecarConfig::new("my-sidecar", "node");
    assert_eq!(config.name, "my-sidecar");
    assert_eq!(config.command, "node");
}

#[test]
fn sidecar_config_validate_success() {
    let config = SidecarConfig::new("test", "node");
    assert!(config.validate().is_ok());
}

#[test]
fn sidecar_config_validate_empty_name() {
    let config = SidecarConfig::new("", "node");
    assert!(config.validate().is_err());
}

#[test]
fn sidecar_config_validate_empty_command() {
    let config = SidecarConfig::new("test", "");
    assert!(config.validate().is_err());
}

#[test]
fn sidecar_config_to_spec() {
    let mut config = SidecarConfig::new("test", "node");
    config.args = vec!["host.js".into()];
    config.env.insert("KEY".into(), "VAL".into());
    let spec = config.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env.get("KEY").unwrap(), "VAL");
}

#[test]
fn registry_register_and_get() {
    let mut registry = SidecarRegistry::default();
    let config = SidecarConfig::new("node-sidecar", "node");
    registry.register(config).unwrap();
    assert!(registry.get("node-sidecar").is_some());
    assert_eq!(registry.get("node-sidecar").unwrap().command, "node");
}

#[test]
fn registry_register_duplicate_fails() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("test", "node"))
        .unwrap();
    let result = registry.register(SidecarConfig::new("test", "python"));
    assert!(result.is_err());
}

#[test]
fn registry_list() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("alpha", "node"))
        .unwrap();
    registry
        .register(SidecarConfig::new("beta", "python"))
        .unwrap();
    let names = registry.list();
    assert_eq!(names, vec!["alpha", "beta"]); // BTreeMap = sorted
}

#[test]
fn registry_remove() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("test", "node"))
        .unwrap();
    assert!(registry.remove("test"));
    assert!(registry.get("test").is_none());
    assert!(!registry.remove("test")); // second remove is no-op
}

#[test]
fn registry_get_nonexistent_returns_none() {
    let registry = SidecarRegistry::default();
    assert!(registry.get("nonexistent").is_none());
}

// ===========================================================================
// Pool management tests
// ===========================================================================

#[test]
fn pool_config_default() {
    let config = PoolConfig::default();
    assert_eq!(config.min_size, 1);
    assert_eq!(config.max_size, 4);
}

#[test]
fn pool_add_and_acquire() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.id, "s1");
    assert_eq!(entry.state, PoolEntryState::Busy);
}

#[test]
fn pool_acquire_empty_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_release_makes_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    pool.release(&entry.id);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_max_size_enforced() {
    let config = PoolConfig {
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3")); // over capacity
}

#[test]
fn pool_mark_failed() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.mark_failed("s1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
}

#[test]
fn pool_drain() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.drain("s1");
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_remove() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(pool.total_count(), 0);
}

#[test]
fn pool_stats() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.acquire(); // s1 becomes busy
    let stats = pool.stats();
    assert_eq!(stats.total, 2);
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.idle, 1);
}

#[test]
fn pool_stats_utilization() {
    let stats = PoolStats {
        total: 4,
        idle: 2,
        busy: 2,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn pool_stats_utilization_empty() {
    let stats = PoolStats {
        total: 0,
        idle: 0,
        busy: 0,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.0).abs() < f64::EPSILON);
}

// ===========================================================================
// Health monitoring tests
// ===========================================================================

#[test]
fn health_monitor_new_is_empty() {
    let monitor = HealthMonitor::new();
    assert_eq!(monitor.total_checks(), 0);
    assert!(!monitor.all_healthy());
}

#[test]
fn health_monitor_record_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, Some(Duration::from_millis(5)));
    assert_eq!(monitor.total_checks(), 1);
    assert!(monitor.all_healthy());
}

#[test]
fn health_monitor_record_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "timeout".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());
    let unhealthy = monitor.unhealthy_sidecars();
    assert_eq!(unhealthy.len(), 1);
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    let check = monitor.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_consecutive_failures_reset_on_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    monitor.record_check("s1", HealthStatus::Healthy, None);
    let check = monitor.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check("s1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    monitor.record_check("s1", HealthStatus::Healthy, None);
    // 3 healthy out of 4
    let pct = monitor.uptime_percentage("s1");
    assert!((pct - 75.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_uptime_percentage_unknown() {
    let monitor = HealthMonitor::new();
    assert!((monitor.uptime_percentage("nonexistent") - 0.0).abs() < f64::EPSILON);
}

#[test]
fn health_report_overall_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check("s2", HealthStatus::Healthy, None);
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Healthy));
}

#[test]
fn health_report_overall_degraded() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check(
        "s2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_report_overall_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s1", HealthStatus::Healthy, None);
    monitor.record_check(
        "s2",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

// ===========================================================================
// Process info tests
// ===========================================================================

#[test]
fn process_info_initial_state() {
    let spec = SidecarSpec::new("node");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(!info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_config_default() {
    let config = ProcessConfig::default();
    assert!(config.inherit_env);
    assert!(config.timeout.is_none());
    assert!(config.env_vars.is_empty());
}

#[test]
fn process_status_running() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 12345 };
    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_exited() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_status_killed() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_status_timed_out() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

// ===========================================================================
// Retry configuration and logic tests
// ===========================================================================

#[test]
fn retry_config_default() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert!(config.jitter_factor > 0.0);
}

#[test]
fn compute_delay_exponential() {
    let config = RetryConfig {
        jitter_factor: 0.0, // disable jitter for deterministic test
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
    };
    let d0 = compute_delay(&config, 0);
    let d1 = compute_delay(&config, 1);
    let d2 = compute_delay(&config, 2);
    assert_eq!(d0, Duration::from_millis(100));
    assert_eq!(d1, Duration::from_millis(200));
    assert_eq!(d2, Duration::from_millis(400));
}

#[test]
fn compute_delay_capped_at_max() {
    let config = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(300),
        ..RetryConfig::default()
    };
    let d5 = compute_delay(&config, 5);
    assert_eq!(d5, Duration::from_millis(300));
}

#[test]
fn is_retryable_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_timeout() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_exited() {
    let err = HostError::Exited { code: Some(1) };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_sidecar_crashed() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "OOM killed".into(),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_not_retryable_violation() {
    let err = HostError::Violation("bad protocol".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_fatal() {
    let err = HostError::Fatal("unrecoverable".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_protocol_error() {
    let err = HostError::Protocol(ProtocolError::Violation("bad".into()));
    assert!(!is_retryable(&err));
}

// ===========================================================================
// Version negotiation tests
// ===========================================================================

#[test]
fn parse_version_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn parse_version_invalid() {
    assert!(ProtocolVersion::parse("invalid").is_err());
}

#[test]
fn version_compatibility_same_major() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(v01.is_compatible(&v02));
}

#[test]
fn version_incompatible_different_major() {
    let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(!v0.is_compatible(&v1));
}

#[test]
fn negotiate_version_compatible() {
    let local = ProtocolVersion::parse("abp/v0.2").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.minor, 1); // min of 1 and 2
}

#[test]
fn negotiate_version_incompatible() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&local, &remote).is_err());
}

#[test]
fn version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    let v = ProtocolVersion { major: 0, minor: 2 };
    assert!(range.contains(&v));
    assert!(range.is_compatible(&v));
}

#[test]
fn version_range_out_of_range() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    let v = ProtocolVersion { major: 0, minor: 5 };
    assert!(!range.contains(&v));
}

// ===========================================================================
// HostError display & variant tests
// ===========================================================================

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "no node"));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("unexpected hello".into());
    let msg = err.to_string();
    assert!(msg.contains("unexpected hello"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("oom".into());
    let msg = err.to_string();
    assert!(msg.contains("oom"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    let msg = err.to_string();
    assert!(msg.contains("exited"));
}

#[test]
fn host_error_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"));
}

// ===========================================================================
// Envelope builder tests
// ===========================================================================

#[test]
fn envelope_builder_hello_minimal() {
    let env = EnvelopeBuilder::hello().backend("test").build().unwrap();
    match env {
        Envelope::Hello {
            backend,
            contract_version,
            mode,
            ..
        } => {
            assert_eq!(backend.id, "test");
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn envelope_builder_hello_missing_backend() {
    assert!(EnvelopeBuilder::hello().build().is_err());
}

#[test]
fn envelope_builder_fatal_with_ref_id() {
    let env = EnvelopeBuilder::fatal("bad thing")
        .ref_id("r1")
        .build()
        .unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("r1"));
            assert_eq!(error, "bad thing");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_builder_fatal_no_ref_id() {
    let env = EnvelopeBuilder::fatal("general error").build().unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "general error");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_builder_event_missing_ref_id() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "x".into() },
        ext: None,
    };
    assert!(EnvelopeBuilder::event(event).build().is_err());
}

#[test]
fn envelope_builder_final_missing_ref_id() {
    let receipt = make_receipt("test");
    assert!(EnvelopeBuilder::final_receipt(receipt).build().is_err());
}

#[test]
fn envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "rate limited",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    assert!(env.error_code().is_some());
}

#[test]
fn envelope_error_code_on_non_fatal_is_none() {
    let env = make_hello_envelope();
    assert!(env.error_code().is_none());
}

// ===========================================================================
// SidecarHello struct tests
// ===========================================================================

#[test]
fn sidecar_hello_serialization_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: make_backend_identity("test"),
        capabilities: make_capabilities(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.contract_version, CONTRACT_VERSION);
    assert_eq!(decoded.backend.id, "test");
    assert!(decoded.capabilities.contains_key(&Capability::ToolRead));
}

#[test]
fn sidecar_hello_empty_capabilities() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: BackendIdentity {
            id: "minimal".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: SidecarHello = serde_json::from_str(&json).unwrap();
    assert!(decoded.capabilities.is_empty());
}

// ===========================================================================
// Retry metadata tests
// ===========================================================================

#[test]
fn retry_metadata_to_receipt_metadata_empty() {
    let meta = abp_host::retry::RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(50),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(1));
    assert!(!map.contains_key("retry_failed_attempts"));
}

#[test]
fn retry_metadata_to_receipt_metadata_with_failures() {
    let meta = abp_host::retry::RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![
            abp_host::retry::RetryAttempt {
                attempt: 0,
                error: "spawn failed".into(),
                delay: Duration::from_millis(100),
            },
            abp_host::retry::RetryAttempt {
                attempt: 1,
                error: "timeout".into(),
                delay: Duration::from_millis(200),
            },
        ],
        total_duration: Duration::from_millis(500),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    assert!(map.contains_key("retry_failed_attempts"));
}

// ===========================================================================
// Protocol error tests
// ===========================================================================

#[test]
fn protocol_error_json_display() {
    let err = JsonlCodec::decode("invalid").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("bad".into());
    assert!(err.error_code().is_some());
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert!(err.error_code().is_some());
}
