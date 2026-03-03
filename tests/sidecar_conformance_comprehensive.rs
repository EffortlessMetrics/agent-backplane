#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

//! Comprehensive sidecar protocol conformance tests.
//!
//! Validates JSONL envelope parsing, round-trip serialization, ref_id
//! correlation, contract versioning, capability declarations, all
//! `AgentEventKind` variants, receipt handling, error classification,
//! malformed input handling, multi-run sequences, and envelope ordering.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
    CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// =========================================================================
// Helpers
// =========================================================================

fn make_hello() -> Envelope {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        caps,
    )
}

fn make_run(id: &str) -> Envelope {
    let wo = WorkOrderBuilder::new("test task").build();
    Envelope::Run {
        id: id.to_string(),
        work_order: wo,
    }
}

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.to_string(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.to_string(),
        receipt,
    }
}

fn round_trip(envelope: &Envelope) -> Envelope {
    let json = JsonlCodec::encode(envelope).expect("encode");
    JsonlCodec::decode(json.trim()).expect("decode")
}

// =========================================================================
// 1. Parse each envelope type
// =========================================================================

#[test]
fn parse_hello_envelope() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"sc","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn parse_run_envelope() {
    let env = make_run("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { id, .. } if id == "run-1"));
}

#[test]
fn parse_event_envelope() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "hi".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn parse_final_envelope() {
    let env = make_final("run-1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { ref_id, .. } if ref_id == "run-1"));
}

#[test]
fn parse_fatal_envelope() {
    let json = r#"{"t":"fatal","ref_id":"run-1","error":"oops"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "oops"));
}

// =========================================================================
// 2. Verify "t" field discriminator (NOT "type")
// =========================================================================

#[test]
fn envelope_uses_t_discriminator() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#), "must use 't' not 'type'");
    assert!(!json.contains(r#""type":"hello""#));
}

#[test]
fn discriminator_t_for_run() {
    let env = make_run("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn discriminator_t_for_event() {
    let env = make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn discriminator_t_for_final() {
    let env = make_final("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn discriminator_t_for_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn type_discriminator_rejected() {
    let json = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    assert!(JsonlCodec::decode(json).is_err(), "\"type\" should be rejected; only \"t\" is valid");
}

// =========================================================================
// 3. Round-trip serialization for all variants
// =========================================================================

#[test]
fn round_trip_hello() {
    let hello = make_hello();
    let rt = round_trip(&hello);
    assert!(matches!(rt, Envelope::Hello { .. }));
}

#[test]
fn round_trip_run() {
    let run = make_run("rt-run");
    let rt = round_trip(&run);
    match rt {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "rt-run");
            assert_eq!(work_order.task, "test task");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn round_trip_event() {
    let env = make_event(
        "rt-run",
        AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "rt-run");
            assert!(
                matches!(event.kind, AgentEventKind::AssistantMessage { text } if text == "hello world")
            );
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn round_trip_final() {
    let env = make_final("rt-run");
    let rt = round_trip(&env);
    match rt {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "rt-run");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn round_trip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("rt-run".into()),
        error: "kaboom".into(),
        error_code: None,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("rt-run"));
            assert_eq!(error, "kaboom");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn round_trip_fatal_no_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    let rt = round_trip(&env);
    assert!(matches!(rt, Envelope::Fatal { ref_id: None, .. }));
}

// =========================================================================
// 4. ref_id correlation
// =========================================================================

#[test]
fn ref_id_correlates_event_to_run() {
    let run_id = "corr-001";
    let run = make_run(run_id);
    let event = make_event(
        run_id,
        AgentEventKind::AssistantDelta {
            text: "tok".into(),
        },
    );
    let fin = make_final(run_id);

    let run_json = JsonlCodec::encode(&run).unwrap();
    let evt_json = JsonlCodec::encode(&event).unwrap();
    let fin_json = JsonlCodec::encode(&fin).unwrap();

    let run_v: serde_json::Value = serde_json::from_str(run_json.trim()).unwrap();
    let evt_v: serde_json::Value = serde_json::from_str(evt_json.trim()).unwrap();
    let fin_v: serde_json::Value = serde_json::from_str(fin_json.trim()).unwrap();

    let run_id_val = run_v.get("id").unwrap().as_str().unwrap();
    assert_eq!(
        evt_v.get("ref_id").unwrap().as_str().unwrap(),
        run_id_val,
        "event ref_id must match run id"
    );
    assert_eq!(
        fin_v.get("ref_id").unwrap().as_str().unwrap(),
        run_id_val,
        "final ref_id must match run id"
    );
}

#[test]
fn fatal_ref_id_correlates_when_present() {
    let env = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "fail".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["ref_id"].as_str().unwrap(), "run-42");
}

// =========================================================================
// 5. Contract version validation in hello
// =========================================================================

#[test]
fn hello_has_current_contract_version() {
    let hello = make_hello();
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_contract_version_survives_round_trip() {
    let hello = make_hello();
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_contract_version_in_json() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(&format!(r#""contract_version":"{CONTRACT_VERSION}""#)));
}

#[test]
fn parse_hello_with_custom_version() {
    let json = r#"{"t":"hello","contract_version":"abp/v99.0","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, "abp/v99.0"),
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 6. Capability declarations in hello
// =========================================================================

#[test]
fn hello_capabilities_round_trip() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);

    let hello = Envelope::hello(
        BackendIdentity {
            id: "cap-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps.clone(),
    );

    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 3);
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Emulated)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolBash),
                Some(SupportLevel::Unsupported)
            ));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_empty_capabilities() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "empty-caps".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_many_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::ToolGlob, SupportLevel::Native);
    caps.insert(Capability::ToolGrep, SupportLevel::Native);
    caps.insert(Capability::McpClient, SupportLevel::Emulated);

    let hello = Envelope::hello(
        BackendIdentity {
            id: "many-caps".into(),
            backend_version: Some("2.0".into()),
            adapter_version: Some("0.3".into()),
        },
        caps.clone(),
    );

    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), caps.len());
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 7. Work order serialization in run envelope
// =========================================================================

#[test]
fn run_work_order_task_preserved() {
    let wo = WorkOrderBuilder::new("Fix the auth bug").build();
    let env = Envelope::Run {
        id: "wo-1".into(),
        work_order: wo,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "Fix the auth bug");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_work_order_model_config() {
    let wo = WorkOrderBuilder::new("task")
        .model("gpt-4o")
        .max_turns(5)
        .build();
    let env = Envelope::Run {
        id: "wo-2".into(),
        work_order: wo,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4o"));
            assert_eq!(work_order.config.max_turns, Some(5));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_work_order_id_is_uuid() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: "wo-3".into(),
        work_order: wo,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Run { work_order, .. } => {
            assert!(!work_order.id.is_nil());
        }
        _ => panic!("expected Run"),
    }
}

// =========================================================================
// 8. All AgentEvent kinds in event envelopes
// =========================================================================

#[test]
fn event_kind_run_started() {
    let env = make_event(
        "r",
        AgentEventKind::RunStarted {
            message: "Starting".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::RunStarted { message } if message == "Starting")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_run_completed() {
    let env = make_event(
        "r",
        AgentEventKind::RunCompleted {
            message: "Done".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::RunCompleted { message } if message == "Done")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_assistant_delta() {
    let env = make_event(
        "r",
        AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::AssistantDelta { text } if text == "token")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_assistant_message() {
    let env = make_event(
        "r",
        AgentEventKind::AssistantMessage {
            text: "full msg".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::AssistantMessage { text } if text == "full msg")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_tool_call() {
    let env = make_event(
        "r",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "foo.rs"}),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert_eq!(input["path"], "foo.rs");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_tool_result() {
    let env = make_event(
        "r",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_tool_result_error() {
    let env = make_event(
        "r",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!("permission denied"),
            is_error: true,
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_file_changed() {
    let env = make_event(
        "r",
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added main fn".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::FileChanged { path, summary }
                if path == "src/main.rs" && summary == "added main fn")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_command_executed() {
    let env = make_event(
        "r",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command, exit_code, ..
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_warning() {
    let env = make_event(
        "r",
        AgentEventKind::Warning {
            message: "approaching budget limit".into(),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::Warning { message } if message == "approaching budget limit")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_error() {
    let env = make_event(
        "r",
        AgentEventKind::Error {
            message: "model refused".into(),
            error_code: None,
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::Error { message, .. } if message == "model refused")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_kind_error_with_code() {
    let env = make_event(
        "r",
        AgentEventKind::Error {
            message: "backend not found".into(),
            error_code: Some(abp_error::ErrorCode::BackendNotFound),
        },
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { error_code, .. } => {
                assert_eq!(error_code, Some(abp_error::ErrorCode::BackendNotFound));
            }
            other => panic!("expected Error, got {other:?}"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_with_ext_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"role": "assistant"}),
    );

    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        _ => panic!("expected Event"),
    }
}

// =========================================================================
// 9. Receipt in final envelope
// =========================================================================

#[test]
fn final_receipt_outcome_preserved() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_receipt_backend_identity() {
    let receipt = ReceiptBuilder::new("sidecar:node")
        .backend_version("3.0")
        .outcome(Outcome::Complete)
        .build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.backend.id, "sidecar:node");
            assert_eq!(receipt.backend.backend_version.as_deref(), Some("3.0"));
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_receipt_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_receipt_with_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());

    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Final { receipt, .. } => {
            assert!(receipt.receipt_sha256.is_some());
            assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_receipt_failed_outcome() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        _ => panic!("expected Final"),
    }
}

// =========================================================================
// 10. Error classification in fatal envelope
// =========================================================================

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r".into()),
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    let rt = round_trip(&env);
    match rt {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "version mismatch");
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolVersionMismatch)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(
        None,
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    );
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
}

#[test]
fn fatal_no_error_code() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "unknown".into(),
        error_code: None,
    };
    let rt = round_trip(&env);
    match rt {
        Envelope::Fatal { error_code, .. } => assert!(error_code.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_error_code_not_serialized_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(
        !json.contains("error_code"),
        "error_code should be skipped when None"
    );
}

#[test]
fn fatal_from_abp_error() {
    let abp_err = abp_error::AbpError::new(
        abp_error::ErrorCode::BackendCrashed,
        "sidecar exited unexpectedly",
    );
    let env = Envelope::fatal_from_abp_error(Some("r".into()), &abp_err);
    match &env {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "sidecar exited unexpectedly");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendCrashed));
        }
        _ => panic!("expected Fatal"),
    }
}

// =========================================================================
// 11. Malformed JSONL handling
// =========================================================================

#[test]
fn malformed_not_json() {
    assert!(JsonlCodec::decode("this is not json").is_err());
}

#[test]
fn malformed_empty_string() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn malformed_json_but_no_t_field() {
    let json = r#"{"hello": "world"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_unknown_t_value() {
    let json = r#"{"t":"unknown_variant","data":123}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_hello_missing_backend() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_run_missing_work_order() {
    let json = r#"{"t":"run","id":"r-1"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_event_missing_event() {
    let json = r#"{"t":"event","ref_id":"r-1"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_final_missing_receipt() {
    let json = r#"{"t":"final","ref_id":"r-1"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_fatal_missing_error() {
    let json = r#"{"t":"fatal","ref_id":"r-1"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_wrong_type_for_field() {
    let json = r#"{"t":"fatal","ref_id":42,"error":"boom"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_truncated_json() {
    let json = r#"{"t":"fatal","ref_id":"r","err"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn malformed_null_body() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn malformed_array_instead_of_object() {
    assert!(JsonlCodec::decode("[1,2,3]").is_err());
}

// =========================================================================
// 12. Multiple runs in sequence
// =========================================================================

#[test]
fn multiple_runs_in_sequence_via_stream() {
    let hello = make_hello();
    let run1 = make_run("run-a");
    let evt1 = make_event(
        "run-a",
        AgentEventKind::AssistantMessage {
            text: "first".into(),
        },
    );
    let fin1 = make_final("run-a");

    let run2 = make_run("run-b");
    let evt2 = make_event(
        "run-b",
        AgentEventKind::AssistantMessage {
            text: "second".into(),
        },
    );
    let fin2 = make_final("run-b");

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(
        &mut buf,
        &[hello, run1, evt1, fin1, run2, evt2, fin2],
    )
    .unwrap();

    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 7);
    assert!(matches!(&envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(&envelopes[1], Envelope::Run { id, .. } if id == "run-a"));
    assert!(matches!(&envelopes[2], Envelope::Event { ref_id, .. } if ref_id == "run-a"));
    assert!(matches!(&envelopes[3], Envelope::Final { ref_id, .. } if ref_id == "run-a"));
    assert!(matches!(&envelopes[4], Envelope::Run { id, .. } if id == "run-b"));
    assert!(matches!(&envelopes[5], Envelope::Event { ref_id, .. } if ref_id == "run-b"));
    assert!(matches!(&envelopes[6], Envelope::Final { ref_id, .. } if ref_id == "run-b"));
}

#[test]
fn sequential_runs_ref_ids_independent() {
    let run1 = make_run("alpha");
    let run2 = make_run("beta");

    let j1 = JsonlCodec::encode(&run1).unwrap();
    let j2 = JsonlCodec::encode(&run2).unwrap();

    let d1 = JsonlCodec::decode(j1.trim()).unwrap();
    let d2 = JsonlCodec::decode(j2.trim()).unwrap();

    match (&d1, &d2) {
        (Envelope::Run { id: id1, .. }, Envelope::Run { id: id2, .. }) => {
            assert_ne!(id1, id2);
        }
        _ => panic!("expected two Run envelopes"),
    }
}

// =========================================================================
// 13. Envelope ordering: hello must come first
// =========================================================================

#[test]
fn validate_sequence_hello_first() {
    use abp_protocol::validate::EnvelopeValidator;

    let hello = make_hello();
    let run = make_run("r");
    let evt = make_event(
        "r",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let fin = make_final("r");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run, evt, fin]);
    assert!(errors.is_empty(), "valid sequence should have no errors: {errors:?}");
}

#[test]
fn validate_sequence_missing_hello() {
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    let run = make_run("r");
    let fin = make_final("r");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[run, fin]);
    assert!(
        errors.contains(&SequenceError::MissingHello),
        "should detect missing hello"
    );
}

#[test]
fn validate_sequence_hello_not_first() {
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    let hello = make_hello();
    let run = make_run("r");
    let fin = make_final("r");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[run, hello, fin]);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })),
        "should detect hello not first"
    );
}

#[test]
fn validate_sequence_missing_terminal() {
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    let hello = make_hello();
    let run = make_run("r");
    let evt = make_event(
        "r",
        AgentEventKind::AssistantMessage {
            text: "hi".into(),
        },
    );

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run, evt]);
    assert!(
        errors.contains(&SequenceError::MissingTerminal),
        "should detect missing terminal"
    );
}

#[test]
fn validate_sequence_ref_id_mismatch() {
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    let hello = make_hello();
    let run = make_run("run-good");
    let evt = make_event(
        "run-BAD",
        AgentEventKind::AssistantMessage { text: "hi".into() },
    );
    let fin = make_final("run-good");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run, evt, fin]);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })),
        "should detect ref_id mismatch"
    );
}

#[test]
fn validate_sequence_events_before_run() {
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    let hello = make_hello();
    let evt = make_event("r", AgentEventKind::AssistantMessage { text: "hi".into() });
    let run = make_run("r");
    let fin = make_final("r");

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, evt, run, fin]);
    assert!(
        errors.contains(&SequenceError::OutOfOrderEvents),
        "should detect out-of-order events"
    );
}

#[test]
fn validate_sequence_fatal_terminates() {
    use abp_protocol::validate::EnvelopeValidator;

    let hello = make_hello();
    let run = make_run("r");
    let fatal = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: "crash".into(),
        error_code: None,
    };

    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&[hello, run, fatal]);
    assert!(
        errors.is_empty(),
        "fatal should be a valid terminal: {errors:?}"
    );
}

// =========================================================================
// Bonus: execution mode in hello
// =========================================================================

#[test]
fn hello_default_mode_is_mapped() {
    let hello = make_hello();
    match hello {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let rt = round_trip(&hello);
    match rt {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// Bonus: decode_stream handles blank lines
// =========================================================================

#[test]
fn decode_stream_skips_blank_lines() {
    let input = format!(
        "{}\n\n{}\n",
        JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        })
        .unwrap()
        .trim(),
        JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        })
        .unwrap()
        .trim(),
    );
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

// =========================================================================
// Bonus: encode always ends with newline
// =========================================================================

#[test]
fn encode_always_newline_terminated() {
    let envs: Vec<Envelope> = vec![
        make_hello(),
        make_run("r"),
        make_event("r", AgentEventKind::AssistantMessage { text: "x".into() }),
        make_final("r"),
        Envelope::Fatal {
            ref_id: None,
            error: "e".into(),
            error_code: None,
        },
    ];
    for env in &envs {
        let json = JsonlCodec::encode(env).unwrap();
        assert!(
            json.ends_with('\n'),
            "all encoded envelopes must end with newline"
        );
    }
}
