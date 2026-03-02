// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the JSONL sidecar protocol.
//!
//! Covers: envelope serde for all 5 types, "t" tag discrimination, hello with
//! capabilities, run with work order, event variants, final with receipt, fatal
//! with error info, ref_id correlation, invalid envelope handling, multi-event
//! sequences, protocol version negotiation, ordering constraints, large payloads,
//! unicode, concurrent streams, stream parser, builder API, and batch processing.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    ExecutionMode, Outcome, ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{ProtocolVersion, VersionError, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ===========================================================================
// Helpers
// ===========================================================================

fn backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn hello() -> Envelope {
    Envelope::hello(backend("e2e-sidecar"), CapabilityManifest::new())
}

fn hello_caps(caps: CapabilityManifest) -> Envelope {
    Envelope::hello(backend("e2e-sidecar"), caps)
}

fn run_envelope() -> (String, Envelope) {
    let wo = WorkOrderBuilder::new("e2e task").build();
    let id = wo.id.to_string();
    (id.clone(), Envelope::Run { id, work_order: wo })
}

fn event_msg(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

fn event_delta(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.into() },
            ext: None,
        },
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("e2e-mock")
            .outcome(Outcome::Complete)
            .build(),
    }
}

fn fatal_env(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn roundtrip(env: &Envelope) -> Envelope {
    let line = JsonlCodec::encode(env).unwrap();
    JsonlCodec::decode(line.trim()).unwrap()
}

fn encode_all(envs: &[Envelope]) -> String {
    let mut buf = String::new();
    for e in envs {
        buf.push_str(&JsonlCodec::encode(e).unwrap());
    }
    buf
}

fn decode_all(jsonl: &str) -> Vec<Envelope> {
    let reader = BufReader::new(jsonl.as_bytes());
    JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

// ===========================================================================
// 1. Envelope serialization/deserialization for all 5 types
// ===========================================================================

#[test]
fn e2e_hello_serde_roundtrip() {
    let env = hello();
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Hello {
            contract_version,
            backend: b,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(b.id, "e2e-sidecar");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn e2e_run_serde_roundtrip() {
    let (id, env) = run_envelope();
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Run {
            id: rid,
            work_order,
        } => {
            assert_eq!(rid, id);
            assert_eq!(work_order.task, "e2e task");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn e2e_event_serde_roundtrip() {
    let env = event_msg("run-42", "hello e2e");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-42");
            match event.kind {
                AgentEventKind::AssistantMessage { text } => assert_eq!(text, "hello e2e"),
                _ => panic!("expected AssistantMessage"),
            }
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_final_serde_roundtrip() {
    let env = final_env("run-42");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn e2e_fatal_serde_roundtrip() {
    let env = fatal_env(Some("run-42"), "crash");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-42"));
            assert_eq!(error, "crash");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// 2. The "t" tag field discrimination works correctly
// ===========================================================================

#[test]
fn e2e_hello_has_t_hello_tag() {
    let json = JsonlCodec::encode(&hello()).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn e2e_run_has_t_run_tag() {
    let (_, env) = run_envelope();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn e2e_event_has_t_event_tag() {
    let json = JsonlCodec::encode(&event_msg("r", "m")).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn e2e_final_has_t_final_tag() {
    let json = JsonlCodec::encode(&final_env("r")).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn e2e_fatal_has_t_fatal_tag() {
    let json = JsonlCodec::encode(&fatal_env(None, "err")).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn e2e_tag_is_t_not_type() {
    // Protocol envelope uses "t", not "type"
    let json = JsonlCodec::encode(&hello()).unwrap();
    assert!(json.contains(r#""t":"#));
    // The only "type" that should appear is inside event payloads, not at top level
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(val.get("t").is_some());
}

#[test]
fn e2e_event_inner_uses_type_tag() {
    // AgentEventKind uses #[serde(tag = "type")]
    let json = JsonlCodec::encode(&event_msg("r", "m")).unwrap();
    let val: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    let event_obj = val.get("event").unwrap();
    assert!(event_obj.get("type").is_some());
    assert_eq!(event_obj["type"], "assistant_message");
}

#[test]
fn e2e_unknown_tag_value_is_error() {
    let json = r#"{"t":"banana","data":1}"#;
    assert!(matches!(
        JsonlCodec::decode(json).unwrap_err(),
        ProtocolError::Json(_)
    ));
}

#[test]
fn e2e_missing_tag_is_error() {
    let json = r#"{"ref_id":null,"error":"no tag"}"#;
    assert!(matches!(
        JsonlCodec::decode(json).unwrap_err(),
        ProtocolError::Json(_)
    ));
}

// ===========================================================================
// 3. Hello envelope with capabilities
// ===========================================================================

#[test]
fn e2e_hello_empty_capabilities() {
    match roundtrip(&hello()) {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn e2e_hello_native_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let env = hello_caps(caps);
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 3);
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Native)
            ));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn e2e_hello_all_capability_variants() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::ToolGlob, SupportLevel::Native);
    caps.insert(Capability::ToolGrep, SupportLevel::Native);
    caps.insert(Capability::ToolWebSearch, SupportLevel::Unsupported);
    caps.insert(Capability::HooksPreToolUse, SupportLevel::Native);
    caps.insert(Capability::SessionResume, SupportLevel::Emulated);
    caps.insert(Capability::Checkpointing, SupportLevel::Unsupported);
    caps.insert(Capability::McpClient, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Native);
    caps.insert(Capability::ImageInput, SupportLevel::Emulated);
    caps.insert(
        Capability::ToolAskUser,
        SupportLevel::Restricted {
            reason: "disabled in CI".into(),
        },
    );
    let env = hello_caps(caps.clone());
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), caps.len());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn e2e_hello_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        backend("pt-sidecar"),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    match roundtrip(&env) {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn e2e_hello_default_mode_is_mapped() {
    match roundtrip(&hello()) {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn e2e_hello_missing_mode_defaults_to_mapped() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"t","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    match JsonlCodec::decode(json).unwrap() {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

// ===========================================================================
// 4. Run envelope with WorkOrder payload
// ===========================================================================

#[test]
fn e2e_run_preserves_task() {
    let wo = WorkOrderBuilder::new("Fix authentication bug").build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "Fix authentication bug");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn e2e_run_preserves_config_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn e2e_run_preserves_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(25).build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.max_turns, Some(25));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn e2e_run_id_matches_work_order_id() {
    let wo = WorkOrderBuilder::new("task").build();
    let expected_id = wo.id.to_string();
    let env = Envelope::Run {
        id: expected_id.clone(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, expected_id);
            assert_eq!(work_order.id.to_string(), expected_id);
        }
        _ => panic!("expected Run"),
    }
}

// ===========================================================================
// 5. Event envelope with all AgentEvent variants
// ===========================================================================

#[test]
fn e2e_event_run_started() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_run_completed() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_assistant_delta() {
    let env = event_delta("r", "token");
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "token"),
            _ => panic!("expected AssistantDelta"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_assistant_message() {
    let env = event_msg("r", "full message");
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "full message"),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("tc-99".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "foo.rs", "content": "fn main(){}"}),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "write_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-99"));
                assert_eq!(input["path"], "foo.rs");
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_tool_result() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!("exit code 0"),
                is_error: false,
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_tool_result_error() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: None,
                output: serde_json::json!("permission denied"),
                is_error: true,
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_file_changed() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Added error handling".into(),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(summary, "Added error handling");
            }
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_command_executed() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("test result: ok".into()),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command, exit_code, ..
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
            }
            _ => panic!("expected CommandExecuted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_warning() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "rate limit".into(),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_error() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: Some(abp_error::ErrorCode::Internal),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error {
                message,
                error_code,
            } => {
                assert_eq!(message, "something broke");
                assert_eq!(error_code, Some(abp_error::ErrorCode::Internal));
            }
            _ => panic!("expected Error"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_with_ext_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"vendor": "data"}));
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            let e = event.ext.unwrap();
            assert_eq!(e["raw_message"], serde_json::json!({"vendor": "data"}));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_event_without_ext_omits_field() {
    let json = JsonlCodec::encode(&event_msg("r", "no ext")).unwrap();
    assert!(!json.contains("raw_message"));
}

// ===========================================================================
// 6. Final envelope with Receipt
// ===========================================================================

#[test]
fn e2e_final_receipt_outcome_complete() {
    let env = final_env("r1");
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Complete),
        _ => panic!("expected Final"),
    }
}

#[test]
fn e2e_final_receipt_outcome_partial() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        _ => panic!("expected Final"),
    }
}

#[test]
fn e2e_final_receipt_outcome_failed() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        _ => panic!("expected Final"),
    }
}

#[test]
fn e2e_final_receipt_preserves_backend() {
    let env = final_env("r");
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.backend.id, "e2e-mock"),
        _ => panic!("expected Final"),
    }
}

#[test]
fn e2e_final_receipt_with_hash() {
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
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert!(receipt.receipt_sha256.is_some());
            assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
        }
        _ => panic!("expected Final"),
    }
}

// ===========================================================================
// 7. Fatal envelope with error info
// ===========================================================================

#[test]
fn e2e_fatal_with_ref_id() {
    let env = fatal_env(Some("run-x"), "timeout");
    match roundtrip(&env) {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-x"));
            assert_eq!(error, "timeout");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn e2e_fatal_without_ref_id() {
    let env = fatal_env(None, "startup failure");
    match roundtrip(&env) {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup failure");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn e2e_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r".into()),
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    match roundtrip(&env) {
        Envelope::Fatal { error_code, .. } => {
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolVersionMismatch)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn e2e_fatal_without_error_code_omits_field() {
    let json = JsonlCodec::encode(&fatal_env(None, "err")).unwrap();
    assert!(!json.contains("error_code"));
}

#[test]
fn e2e_fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(None, "e", abp_error::ErrorCode::BackendCrashed);
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendCrashed));
}

#[test]
fn e2e_fatal_from_abp_error() {
    let err = abp_error::AbpError::new(
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
        "bad envelope",
    );
    let env = Envelope::fatal_from_abp_error(Some("r".into()), &err);
    match &env {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "bad envelope");
            assert_eq!(
                *error_code,
                Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn e2e_non_fatal_error_code_is_none() {
    assert!(hello().error_code().is_none());
}

// ===========================================================================
// 8. ref_id correlation across envelopes
// ===========================================================================

#[test]
fn e2e_ref_id_correlation_valid_sequence() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        hello(),
        run_env,
        event_msg(&run_id, "e1"),
        event_msg(&run_id, "e2"),
        final_env(&run_id),
    ]);
    assert!(errs.is_empty(), "expected no errors: {errs:?}");
}

#[test]
fn e2e_ref_id_mismatch_in_event() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        hello(),
        run_env,
        event_msg("wrong-id", "bad"),
        final_env(&run_id),
    ]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn e2e_ref_id_mismatch_in_final() {
    let (_, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[hello(), run_env, final_env("other-run")]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn e2e_ref_id_mismatch_in_fatal() {
    let (_, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs =
        validator.validate_sequence(&[hello(), run_env, fatal_env(Some("wrong-ref"), "error")]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn e2e_ref_id_fatal_none_no_mismatch() {
    let (_, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[hello(), run_env, fatal_env(None, "error")]);
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// ===========================================================================
// 9. Invalid envelope handling (missing fields, wrong types)
// ===========================================================================

#[test]
fn e2e_invalid_json_is_error() {
    assert!(matches!(
        JsonlCodec::decode("not json").unwrap_err(),
        ProtocolError::Json(_)
    ));
}

#[test]
fn e2e_empty_string_is_error() {
    assert!(matches!(
        JsonlCodec::decode("").unwrap_err(),
        ProtocolError::Json(_)
    ));
}

#[test]
fn e2e_valid_json_but_wrong_structure() {
    let json = r#"{"t":"hello"}"#; // missing required fields
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn e2e_numeric_tag_is_error() {
    let json = r#"{"t":42,"error":"e"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn e2e_null_tag_is_error() {
    let json = r#"{"t":null,"error":"e"}"#;
    assert!(JsonlCodec::decode(json).is_err());
}

#[test]
fn e2e_empty_contract_version_validation() {
    let env = Envelope::Hello {
        contract_version: String::new(),
        backend: backend("t"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
    assert!(result.errors.iter().any(
        |e| matches!(e, ValidationError::EmptyField { field } if field == "contract_version")
    ));
}

#[test]
fn e2e_invalid_contract_version_format() {
    let env = Envelope::Hello {
        contract_version: "v0.1".into(),
        backend: backend("t"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn e2e_empty_backend_id_validation() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
}

#[test]
fn e2e_empty_run_id_validation() {
    let wo = WorkOrderBuilder::new("task").build();
    let env = Envelope::Run {
        id: String::new(),
        work_order: wo,
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
}

#[test]
fn e2e_empty_task_validation() {
    let wo = WorkOrderBuilder::new("").build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(!result.valid);
}

#[test]
fn e2e_empty_ref_id_in_event_validation() {
    let result = EnvelopeValidator::new().validate(&event_msg("", "m"));
    assert!(!result.valid);
}

#[test]
fn e2e_empty_ref_id_in_final_validation() {
    let result = EnvelopeValidator::new().validate(&final_env(""));
    assert!(!result.valid);
}

#[test]
fn e2e_empty_error_in_fatal_validation() {
    let result = EnvelopeValidator::new().validate(&fatal_env(Some("r"), ""));
    assert!(!result.valid);
}

#[test]
fn e2e_null_ref_id_in_fatal_is_warning() {
    let result = EnvelopeValidator::new().validate(&fatal_env(None, "err"));
    assert!(result.valid); // warning, not error
    assert!(result.warnings.iter().any(
        |w| matches!(w, ValidationWarning::MissingOptionalField { field } if field == "ref_id")
    ));
}

#[test]
fn e2e_missing_backend_version_is_warning() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&env);
    assert!(result.valid);
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "backend.backend_version"
    )));
}

// ===========================================================================
// 10. Multiple events in sequence
// ===========================================================================

#[test]
fn e2e_multi_event_jsonl_stream() {
    let (run_id, run_env) = run_envelope();
    let envs: Vec<Envelope> = vec![
        hello(),
        run_env,
        event_msg(&run_id, "e1"),
        event_delta(&run_id, "tok1"),
        event_delta(&run_id, "tok2"),
        event_msg(&run_id, "e2"),
        final_env(&run_id),
    ];
    let jsonl = encode_all(&envs);
    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), 7);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Event { .. }));
    assert!(matches!(decoded[4], Envelope::Event { .. }));
    assert!(matches!(decoded[5], Envelope::Event { .. }));
    assert!(matches!(decoded[6], Envelope::Final { .. }));
}

#[test]
fn e2e_hundred_events_roundtrip() {
    let (run_id, run_env) = run_envelope();
    let mut envs = vec![hello(), run_env];
    for i in 0..100 {
        envs.push(event_delta(&run_id, &format!("t{i}")));
    }
    envs.push(final_env(&run_id));
    let jsonl = encode_all(&envs);
    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), 103);
}

#[test]
fn e2e_stream_parser_multi_event() {
    let (run_id, _) = run_envelope();
    let mut parser = StreamParser::new();
    let mut all = Vec::new();
    for i in 0..10 {
        let env = event_delta(&run_id, &format!("t{i}"));
        let line = JsonlCodec::encode(&env).unwrap();
        all.extend(parser.push(line.as_bytes()));
    }
    assert_eq!(all.len(), 10);
    assert!(all.iter().all(|r| r.is_ok()));
}

#[test]
fn e2e_stream_parser_partial_then_complete() {
    let env = event_msg("r", "partial test");
    let line = JsonlCodec::encode(&env).unwrap();
    let bytes = line.as_bytes();
    let (a, b) = bytes.split_at(bytes.len() / 2);

    let mut parser = StreamParser::new();
    assert!(parser.push(a).is_empty());
    let results = parser.push(b);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn e2e_stream_parser_finish_flushes() {
    let env = fatal_env(None, "pending");
    let line = JsonlCodec::encode(&env).unwrap();
    let trimmed = line.trim_end();

    let mut parser = StreamParser::new();
    assert!(parser.push(trimmed.as_bytes()).is_empty());
    let results = parser.finish();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn e2e_stream_parser_reset() {
    let mut parser = StreamParser::new();
    parser.push(b"partial data");
    assert!(!parser.is_empty());
    parser.reset();
    assert!(parser.is_empty());
    assert_eq!(parser.buffered_len(), 0);
}

// ===========================================================================
// 11. Protocol version negotiation
// ===========================================================================

#[test]
fn e2e_parse_version_current() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn e2e_parse_version_roundtrip() {
    let v = ProtocolVersion::parse("abp/v2.5").unwrap();
    assert_eq!(v.to_string(), "abp/v2.5");
}

#[test]
fn e2e_parse_version_invalid_format() {
    assert!(matches!(
        ProtocolVersion::parse("invalid"),
        Err(VersionError::InvalidFormat)
    ));
}

#[test]
fn e2e_parse_version_missing_prefix() {
    assert!(ProtocolVersion::parse("v0.1").is_err());
}

#[test]
fn e2e_version_compatibility_same_major() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(v01.is_compatible(&v02));
}

#[test]
fn e2e_version_compatibility_different_major() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(!v01.is_compatible(&v10));
}

#[test]
fn e2e_negotiate_version_same_major() {
    let local = ProtocolVersion::parse("abp/v0.2").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result, remote); // min of the two
}

#[test]
fn e2e_negotiate_version_incompatible() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(matches!(
        negotiate_version(&local, &remote),
        Err(VersionError::Incompatible { .. })
    ));
}

#[test]
fn e2e_version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.5").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.3").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.6").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.0").unwrap()));
}

#[test]
fn e2e_version_range_compatible() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.5").unwrap(),
    };
    assert!(range.is_compatible(&ProtocolVersion::parse("abp/v0.3").unwrap()));
    assert!(!range.is_compatible(&ProtocolVersion::parse("abp/v1.0").unwrap()));
}

#[test]
fn e2e_protocol_version_current() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn e2e_free_fn_parse_version() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
    assert!(abp_protocol::parse_version("invalid").is_none());
    assert!(abp_protocol::parse_version("").is_none());
}

#[test]
fn e2e_free_fn_is_compatible_version() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("bad", "abp/v0.1"));
}

// ===========================================================================
// 12. Envelope ordering constraints (hello must be first)
// ===========================================================================

#[test]
fn e2e_missing_hello_sequence_error() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[run_env, event_msg(&run_id, "m"), final_env(&run_id)]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
}

#[test]
fn e2e_hello_not_first_sequence_error() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[run_env, hello(), final_env(&run_id)]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

#[test]
fn e2e_event_before_run_is_out_of_order() {
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        hello(),
        event_msg("r", "early"),
        fatal_env(Some("r"), "abort"),
    ]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents))
    );
}

#[test]
fn e2e_missing_terminal_sequence_error() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[hello(), run_env, event_msg(&run_id, "m")]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn e2e_multiple_terminals_sequence_error() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[
        hello(),
        run_env,
        final_env(&run_id),
        fatal_env(Some(&run_id), "also"),
    ]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals))
    );
}

#[test]
fn e2e_empty_sequence_errors() {
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[]);
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
    assert!(
        errs.iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn e2e_valid_minimal_sequence() {
    let (run_id, run_env) = run_envelope();
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[hello(), run_env, final_env(&run_id)]);
    assert!(errs.is_empty(), "expected no errors: {errs:?}");
}

#[test]
fn e2e_valid_hello_then_fatal_no_run() {
    let validator = EnvelopeValidator::new();
    let errs = validator.validate_sequence(&[hello(), fatal_env(None, "startup error")]);
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// ===========================================================================
// 13. Large payload envelopes
// ===========================================================================

#[test]
fn e2e_large_message_roundtrip() {
    let big = "X".repeat(200_000);
    let env = event_msg("r", &big);
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 200_000),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_large_tool_input_roundtrip() {
    let big_json = serde_json::json!({
        "content": "Y".repeat(100_000),
        "metadata": { "size": 100_000 }
    });
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: big_json.clone(),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input["metadata"]["size"], 100_000);
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_large_receipt_trace_roundtrip() {
    let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for i in 0..500 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("t{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.trace.len(), 500),
        _ => panic!("expected Final"),
    }
}

#[test]
fn e2e_large_stream_parser_multi_chunk() {
    let mut parser = StreamParser::new();
    let mut total = 0;
    for i in 0..50 {
        let env = event_msg("r", &format!("event-{i}-{}", "Z".repeat(1000)));
        let line = JsonlCodec::encode(&env).unwrap();
        total += parser.push(line.as_bytes()).len();
    }
    assert_eq!(total, 50);
}

// ===========================================================================
// 14. Unicode in envelope payloads
// ===========================================================================

#[test]
fn e2e_unicode_japanese_roundtrip() {
    let env = event_msg("r", "こんにちは世界");
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "こんにちは世界"),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_unicode_emoji_roundtrip() {
    let env = event_msg("r", "🚀🔥💻🎉");
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "🚀🔥💻🎉"),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_unicode_arabic_roundtrip() {
    let env = fatal_env(None, "خطأ: ملف غير موجود");
    match roundtrip(&env) {
        Envelope::Fatal { error, .. } => assert_eq!(error, "خطأ: ملف غير موجود"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn e2e_unicode_mixed_scripts_roundtrip() {
    let text = "English 日本語 العربية Ελληνικά 🌍";
    let env = event_msg("r", text);
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text: t } => assert_eq!(t, text),
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_special_chars_escaped_in_jsonl() {
    let env = event_msg("r", "line1\nline2\ttab\"quote\\back");
    let json = JsonlCodec::encode(&env).unwrap();
    // JSONL: exactly one newline at the end
    assert_eq!(json.matches('\n').count(), 1);
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains('\n'));
                assert!(text.contains('\t'));
                assert!(text.contains('"'));
                assert!(text.contains('\\'));
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn e2e_unicode_in_backend_id() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidécar-ñ-über".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    match roundtrip(&env) {
        Envelope::Hello { backend: b, .. } => assert_eq!(b.id, "sidécar-ñ-über"),
        _ => panic!("expected Hello"),
    }
}

// ===========================================================================
// 15. Concurrent envelope streams
// ===========================================================================

#[test]
fn e2e_interleaved_streams_decode_independently() {
    // Two independent run streams interleaved in the same JSONL buffer
    let (id_a, run_a) = run_envelope();
    let (id_b, run_b) = run_envelope();

    let mut jsonl = String::new();
    jsonl.push_str(&JsonlCodec::encode(&hello()).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&run_a).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&run_b).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&event_msg(&id_a, "a1")).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&event_msg(&id_b, "b1")).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&event_msg(&id_a, "a2")).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&final_env(&id_a)).unwrap());
    jsonl.push_str(&JsonlCodec::encode(&final_env(&id_b)).unwrap());

    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), 8);

    // Filter events for stream A
    let a_events: Vec<_> = decoded
        .iter()
        .filter(|e| matches!(e, Envelope::Event { ref_id, .. } if ref_id == &id_a))
        .collect();
    assert_eq!(a_events.len(), 2);

    // Filter events for stream B
    let b_events: Vec<_> = decoded
        .iter()
        .filter(|e| matches!(e, Envelope::Event { ref_id, .. } if ref_id == &id_b))
        .collect();
    assert_eq!(b_events.len(), 1);
}

#[test]
fn e2e_stream_parser_interleaved_byte_chunks() {
    let env_a = event_msg("stream-a", "msg-a");
    let env_b = event_msg("stream-b", "msg-b");
    let line_a = JsonlCodec::encode(&env_a).unwrap();
    let line_b = JsonlCodec::encode(&env_b).unwrap();

    let mut parser = StreamParser::new();
    // Feed partial A, then partial B, then rest of A, then rest of B
    let (a1, a2) = line_a.as_bytes().split_at(line_a.len() / 2);
    let (b1, b2) = line_b.as_bytes().split_at(line_b.len() / 2);

    // Partial A -> nothing yet
    assert!(parser.push(a1).is_empty());
    // Complete A + partial B in one push
    let mut combined = Vec::new();
    combined.extend_from_slice(a2);
    combined.extend_from_slice(b1);
    let results = parser.push(&combined);
    assert_eq!(results.len(), 1); // only A completes
    assert!(results[0].is_ok());

    // Complete B
    let results = parser.push(b2);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[test]
fn e2e_concurrent_ref_ids_in_batch() {
    let envs: Vec<Envelope> = (0..10)
        .map(|i| event_msg(&format!("run-{i}"), &format!("msg-{i}")))
        .collect();
    let jsonl = encode_all(&envs);
    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), 10);
    for (i, env) in decoded.iter().enumerate() {
        match env {
            Envelope::Event { ref_id, .. } => {
                assert_eq!(ref_id, &format!("run-{i}"));
            }
            _ => panic!("expected Event"),
        }
    }
}

// ===========================================================================
// Additional coverage: JSONL codec edge cases
// ===========================================================================

#[test]
fn e2e_encode_ends_with_exactly_one_newline() {
    let json = JsonlCodec::encode(&hello()).unwrap();
    assert!(json.ends_with('\n'));
    assert_eq!(json.matches('\n').count(), 1);
}

#[test]
fn e2e_decode_stream_skips_empty_lines() {
    let env = fatal_env(None, "err");
    let line = JsonlCodec::encode(&env).unwrap();
    let input = format!("\n\n{line}\n\n{line}\n");
    let decoded = decode_all(&input);
    assert_eq!(decoded.len(), 2);
}

#[test]
fn e2e_decode_stream_skips_whitespace_lines() {
    let env = fatal_env(None, "e");
    let line = JsonlCodec::encode(&env).unwrap();
    let input = format!("   \n\t\n{line}");
    let decoded = decode_all(&input);
    assert_eq!(decoded.len(), 1);
}

#[test]
fn e2e_encode_to_writer() {
    let env = hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains(r#""t":"hello""#));
}

#[test]
fn e2e_encode_many_to_writer() {
    let envs = [hello(), fatal_env(None, "err"), event_msg("r", "m")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 3);
}

// ===========================================================================
// Additional coverage: unknown/extra fields (forward compat)
// ===========================================================================

#[test]
fn e2e_unknown_fields_in_hello_ignored() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"t","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped","future_field":"v"}"#;
    assert!(matches!(
        JsonlCodec::decode(json).unwrap(),
        Envelope::Hello { .. }
    ));
}

#[test]
fn e2e_unknown_fields_in_fatal_ignored() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra":42}"#;
    assert!(matches!(
        JsonlCodec::decode(json).unwrap(),
        Envelope::Fatal { .. }
    ));
}

#[test]
fn e2e_explicit_null_ref_id_in_fatal() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"test"}"#;
    match JsonlCodec::decode(json).unwrap() {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn e2e_explicit_string_ref_id_in_fatal() {
    let json = r#"{"t":"fatal","ref_id":"abc","error":"test"}"#;
    match JsonlCodec::decode(json).unwrap() {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("abc")),
        _ => panic!("expected Fatal"),
    }
}

// ===========================================================================
// Additional coverage: full protocol flow simulation
// ===========================================================================

#[test]
fn e2e_full_protocol_flow_happy_path() {
    let (run_id, run_env) = run_envelope();
    let envs = vec![
        hello(),
        run_env,
        event_msg(&run_id, "Starting work"),
        Envelope::Event {
            ref_id: run_id.clone(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: run_id.clone(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tc1".into()),
                    output: serde_json::json!("fn main() {}"),
                    is_error: false,
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: run_id.clone(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "src/main.rs".into(),
                    summary: "Refactored main".into(),
                },
                ext: None,
            },
        },
        event_msg(&run_id, "Done!"),
        final_env(&run_id),
    ];

    // Encode, decode, validate
    let jsonl = encode_all(&envs);
    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), envs.len());

    let validator = EnvelopeValidator::new();
    let seq_errs = validator.validate_sequence(&decoded);
    assert!(seq_errs.is_empty(), "sequence errors: {seq_errs:?}");
}

#[test]
fn e2e_full_protocol_flow_fatal_path() {
    let (run_id, run_env) = run_envelope();
    let envs = vec![
        hello(),
        run_env,
        event_msg(&run_id, "Starting"),
        Envelope::Event {
            ref_id: run_id.clone(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "API rate limit".into(),
                    error_code: None,
                },
                ext: None,
            },
        },
        fatal_env(Some(&run_id), "Rate limited"),
    ];

    let jsonl = encode_all(&envs);
    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), 5);
    assert!(matches!(decoded.last(), Some(Envelope::Fatal { .. })));
}

#[test]
fn e2e_two_sequential_runs_in_one_connection() {
    let (id1, run1) = run_envelope();
    let (id2, run2) = run_envelope();
    let envs = vec![
        hello(),
        run1,
        event_msg(&id1, "work1"),
        final_env(&id1),
        run2,
        event_msg(&id2, "work2"),
        final_env(&id2),
    ];
    let jsonl = encode_all(&envs);
    let decoded = decode_all(&jsonl);
    assert_eq!(decoded.len(), 7);

    // Count each type
    let hellos = decoded
        .iter()
        .filter(|e| matches!(e, Envelope::Hello { .. }))
        .count();
    let runs = decoded
        .iter()
        .filter(|e| matches!(e, Envelope::Run { .. }))
        .count();
    let events = decoded
        .iter()
        .filter(|e| matches!(e, Envelope::Event { .. }))
        .count();
    let finals = decoded
        .iter()
        .filter(|e| matches!(e, Envelope::Final { .. }))
        .count();
    assert_eq!(hellos, 1);
    assert_eq!(runs, 2);
    assert_eq!(events, 2);
    assert_eq!(finals, 2);
}

// ===========================================================================
// Additional coverage: ProtocolError variants
// ===========================================================================

#[test]
fn e2e_protocol_error_json_variant() {
    let err = JsonlCodec::decode("{bad json}").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(err.error_code().is_none());
}

#[test]
fn e2e_protocol_error_violation() {
    let err = ProtocolError::Violation("test violation".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn e2e_protocol_error_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn e2e_protocol_error_display() {
    let err = ProtocolError::Violation("broken".into());
    assert!(err.to_string().contains("broken"));
}

// ===========================================================================
// Additional coverage: builder API
// ===========================================================================

#[test]
fn e2e_envelope_builder_hello() {
    use abp_protocol::builder::EnvelopeBuilder;
    let env = EnvelopeBuilder::hello()
        .backend("builder-test")
        .version("2.0.0")
        .build()
        .unwrap();
    match roundtrip(&env) {
        Envelope::Hello { backend: b, .. } => {
            assert_eq!(b.id, "builder-test");
            assert_eq!(b.backend_version.as_deref(), Some("2.0.0"));
        }
        _ => panic!("expected Hello"),
    }
}

// ===========================================================================
// Additional: deterministic serialization (BTreeMap ordering)
// ===========================================================================

#[test]
fn e2e_capabilities_btreemap_ordering() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let env = hello_caps(caps);
    let json1 = JsonlCodec::encode(&env).unwrap();
    let json2 = JsonlCodec::encode(&env).unwrap();
    // BTreeMap ensures deterministic ordering
    assert_eq!(json1, json2);
}

#[test]
fn e2e_receipt_deterministic_json() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json1 = serde_json::to_string(&receipt).unwrap();
    let json2 = serde_json::to_string(&receipt).unwrap();
    assert_eq!(json1, json2);
}

// ===========================================================================
// Additional: nested tool calls
// ===========================================================================

#[test]
fn e2e_nested_tool_call_parent_id() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "inner_tool".into(),
                tool_use_id: Some("tc-child".into()),
                parent_tool_use_id: Some("tc-parent".into()),
                input: serde_json::json!({}),
            },
            ext: None,
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                parent_tool_use_id, ..
            } => {
                assert_eq!(parent_tool_use_id.as_deref(), Some("tc-parent"));
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

// ===========================================================================
// Additional: max line length in StreamParser
// ===========================================================================

#[test]
fn e2e_stream_parser_max_line_len_exceeded() {
    let mut parser = StreamParser::with_max_line_len(50);
    let big_line = format!(
        r#"{{"t":"fatal","ref_id":null,"error":"{}"}}"#,
        "X".repeat(100)
    );
    let results = parser.push(format!("{big_line}\n").as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    match &results[0] {
        Err(ProtocolError::Violation(msg)) => assert!(msg.contains("exceeds maximum")),
        other => panic!("expected Violation, got {other:?}"),
    }
}

#[test]
fn e2e_stream_parser_within_max_line_len() {
    let mut parser = StreamParser::with_max_line_len(10_000);
    let env = fatal_env(None, "short");
    let line = JsonlCodec::encode(&env).unwrap();
    let results = parser.push(line.as_bytes());
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}
