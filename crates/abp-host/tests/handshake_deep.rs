// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the sidecar JSONL handshake protocol.
//!
//! Covers valid/invalid hello envelopes, run forwarding, event streaming,
//! final/fatal envelopes, ref_id correlation, version negotiation,
//! sequence validation, serde roundtrips, and edge cases.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, ReceiptBuilder, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use std::collections::BTreeMap;
use std::io::BufReader;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.3.0".into()),
    }
}

fn minimal_backend() -> BackendIdentity {
    BackendIdentity {
        id: "minimal".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn test_work_order() -> WorkOrder {
    WorkOrderBuilder::new("handshake test task").build()
}

fn test_receipt() -> Receipt {
    ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build()
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), CapabilityManifest::new())
}

fn make_hello_with_caps(caps: CapabilityManifest) -> Envelope {
    Envelope::hello(test_backend(), caps)
}

fn make_run() -> (String, Envelope) {
    let wo = test_work_order();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    (id, env)
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
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: test_receipt(),
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(Into::into),
        error: error.into(),
        error_code: None,
    }
}

fn roundtrip(env: &Envelope) -> Envelope {
    let encoded = JsonlCodec::encode(env).unwrap();
    JsonlCodec::decode(encoded.trim()).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Valid hello envelopes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_hello_minimal_backend() {
    let hello = Envelope::hello(minimal_backend(), CapabilityManifest::new());
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "minimal");
            assert!(backend.backend_version.is_none());
            assert!(backend.adapter_version.is_none());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_full_backend() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello {
            backend,
            contract_version,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("0.3.0"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_contract_version_matches() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains(&format!("\"contract_version\":\"{}\"", CONTRACT_VERSION)));
}

#[test]
fn valid_hello_with_empty_capabilities() {
    let hello = make_hello_with_caps(CapabilityManifest::new());
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_with_single_capability() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let hello = make_hello_with_caps(caps);
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 1);
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_with_many_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
    let hello = make_hello_with_caps(caps.clone());
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 5);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_mapped_mode_default() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        test_backend(),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn valid_hello_serde_tag_is_t() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.contains(r#""t":"hello""#));
    // Must NOT contain "type":"hello"
    assert!(!encoded.contains(r#""type":"hello""#));
}

#[test]
fn valid_hello_ends_with_newline() {
    let hello = make_hello();
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn valid_hello_validator_passes() {
    let hello = make_hello();
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn valid_hello_validator_warns_missing_optional_fields() {
    let hello = Envelope::hello(minimal_backend(), CapabilityManifest::new());
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(result.valid); // warnings are not errors
    let warning_fields: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            ValidationWarning::MissingOptionalField { field } => Some(field.as_str()),
            _ => None,
        })
        .collect();
    assert!(warning_fields.contains(&"backend.backend_version"));
    assert!(warning_fields.contains(&"backend.adapter_version"));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Invalid hello envelopes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn invalid_hello_missing_t_field() {
    let json = r#"{"contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_wrong_t_value() {
    let json = r#"{"t":"greet","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_missing_contract_version() {
    let json = r#"{"t":"hello","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_missing_backend() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_missing_capabilities() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_missing_backend_id() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_wrong_version_format_validator() {
    let hello = Envelope::Hello {
        contract_version: "invalid-version".into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::InvalidVersion { version } if version == "invalid-version"
    )));
}

#[test]
fn invalid_hello_empty_version_validator() {
    let hello = Envelope::Hello {
        contract_version: String::new(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "contract_version"
    )));
}

#[test]
fn invalid_hello_empty_backend_id_validator() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn invalid_hello_bad_json_not_json() {
    let result = JsonlCodec::decode("this is not json at all");
    assert!(matches!(result, Err(ProtocolError::Json(_))));
}

#[test]
fn invalid_hello_bad_json_empty() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn invalid_hello_bad_json_empty_object() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn invalid_hello_bad_json_array() {
    let result = JsonlCodec::decode("[]");
    assert!(result.is_err());
}

#[test]
fn invalid_hello_bad_json_null() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

#[test]
fn invalid_hello_bad_json_number() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

#[test]
fn invalid_hello_bad_json_trailing_comma() {
    let result = JsonlCodec::decode(r#"{"t":"hello",}"#);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_bad_json_truncated() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0"#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Multiple hello envelopes (should error in sequence)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sequence_double_hello_no_crash() {
    // The sequence validator does not explicitly reject duplicate hellos
    // as long as the first envelope is a Hello. This test documents
    // that the validator tolerates (but does not encourage) duplicates.
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        make_hello(), // second hello — tolerated
        run,
        make_final(&id),
    ];
    let _errors = v.validate_sequence(&seq);
    // No panic — the validator handles this gracefully.
}

#[test]
fn sequence_hello_not_first_position() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![run, make_hello(), make_final(&id)];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Run envelope after hello
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_envelope_roundtrip() {
    let wo = test_work_order();
    let run = Envelope::Run {
        id: "run-abc".into(),
        work_order: wo.clone(),
    };
    let decoded = roundtrip(&run);
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-abc");
            assert_eq!(work_order.task, wo.task);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_serde_tag() {
    let (_, run) = make_run();
    let encoded = JsonlCodec::encode(&run).unwrap();
    assert!(encoded.contains(r#""t":"run""#));
}

#[test]
fn run_envelope_preserves_work_order_id() {
    let wo = test_work_order();
    let expected_id = wo.id;
    let run = Envelope::Run {
        id: expected_id.to_string(),
        work_order: wo,
    };
    let decoded = roundtrip(&run);
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.id, expected_id);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_preserves_task() {
    let wo = WorkOrderBuilder::new("complex multi-line\ntask description").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let decoded = roundtrip(&run);
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert!(work_order.task.contains("complex multi-line"));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_empty_id_validator_error() {
    let v = EnvelopeValidator::new();
    let run = Envelope::Run {
        id: String::new(),
        work_order: test_work_order(),
    };
    let result = v.validate(&run);
    assert!(!result.valid);
}

#[test]
fn run_envelope_empty_task_validator_error() {
    let v = EnvelopeValidator::new();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: WorkOrder {
            id: Uuid::new_v4(),
            task: String::new(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: ".".into(),
                mode: WorkspaceMode::PassThrough,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        },
    };
    let result = v.validate(&run);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Event streaming
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_assistant_delta_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantDelta {
            text: "Hello ".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(
                matches!(event.kind, AgentEventKind::AssistantDelta { text } if text == "Hello ")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_assistant_message_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "Complete message".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::AssistantMessage { text } if text == "Complete message")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_call_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/tmp/test.txt"}),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert_eq!(input["path"], "/tmp/test.txt");
            }
            _ => panic!("expected ToolCall"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_result_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"content": "file contents"}),
            is_error: false,
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                output,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!is_error);
                assert_eq!(output["content"], "file contents");
            }
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_result_error_flag() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!({"stderr": "command not found"}),
            is_error: true,
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::ToolResult { is_error, .. } => {
                assert!(is_error);
            }
            _ => panic!("expected ToolResult"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_run_started_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::RunStarted {
            message: "Starting execution".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::RunStarted { message } if message == "Starting execution")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_run_completed_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::RunCompleted {
            message: "Done".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::RunCompleted { message } if message == "Done")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_file_changed_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "Added entry point".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "Added entry point");
            }
            _ => panic!("expected FileChanged"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_command_executed_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all tests passed".into()),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(*exit_code, Some(0));
                assert_eq!(output_preview.as_deref(), Some("all tests passed"));
            }
            _ => panic!("expected CommandExecuted"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_warning_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::Warning {
            message: "deprecated API".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::Warning { message } if message == "deprecated API")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_error_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::Error {
            message: "something broke".into(),
            error_code: None,
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(&event.kind, AgentEventKind::Error { message, .. } if message == "something broke")
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_error_with_error_code_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::Error {
            message: "tool failed".into(),
            error_code: Some(abp_error::ErrorCode::ExecutionToolFailed),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match &event.kind {
            AgentEventKind::Error {
                error_code: Some(code),
                ..
            } => {
                assert_eq!(*code, abp_error::ErrorCode::ExecutionToolFailed);
            }
            _ => panic!("expected Error with code"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_serde_tag_is_t() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantDelta { text: "hi".into() },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains(r#""t":"event""#));
}

#[test]
fn event_empty_ref_id_validator_error() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Event {
        ref_id: String::new(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "x".into() },
            ext: None,
        },
    };
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn event_with_ext_passthrough_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor": "data"}),
    );
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "msg".into() },
            ext: Some(ext),
        },
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            let ext = event.ext.as_ref().unwrap();
            assert!(ext.contains_key("raw_message"));
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Final envelope
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn final_envelope_roundtrip() {
    let env = make_final("run-42");
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
fn final_envelope_serde_tag() {
    let env = make_final("run-1");
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains(r#""t":"final""#));
}

#[test]
fn final_envelope_receipt_hash_verification() {
    let receipt = test_receipt().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);

    // Re-hash should produce the same value
    let rehash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(&rehash, hash);
}

#[test]
fn final_envelope_receipt_hash_deterministic() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let r2 = r1.clone();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn final_envelope_receipt_hash_null_before_hash() {
    // receipt_hash sets receipt_sha256 to null before hashing
    let mut receipt = test_receipt();
    receipt.receipt_sha256 = Some("fake_hash".into());
    let h1 = abp_core::receipt_hash(&receipt).unwrap();

    receipt.receipt_sha256 = None;
    let h2 = abp_core::receipt_hash(&receipt).unwrap();

    // Both should produce the same hash because the field is nulled
    assert_eq!(h1, h2);
}

#[test]
fn final_envelope_receipt_with_trace() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        })
        .build();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.trace.len(), 1);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_envelope_receipt_partial_outcome() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Partial)
        .build();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.outcome, Outcome::Partial);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_envelope_receipt_failed_outcome() {
    let receipt = ReceiptBuilder::new("test").outcome(Outcome::Failed).build();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.outcome, Outcome::Failed);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_envelope_empty_ref_id_validator_error() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Final {
        ref_id: String::new(),
        receipt: test_receipt(),
    };
    let result = v.validate(&env);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Fatal envelope
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fatal_envelope_roundtrip() {
    let env = make_fatal(Some("run-1"), "out of memory");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "out of memory");
            assert!(error_code.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_no_ref_id() {
    let env = make_fatal(None, "startup failure");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup failure");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_serde_tag() {
    let env = make_fatal(Some("r"), "err");
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains(r#""t":"fatal""#));
}

#[test]
fn fatal_envelope_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "handshake failed",
        abp_error::ErrorCode::ProtocolHandshakeFailed,
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { error_code, .. } => {
            assert_eq!(
                error_code,
                Some(abp_error::ErrorCode::ProtocolHandshakeFailed)
            );
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_error_code_accessor() {
    let env = Envelope::fatal_with_code(
        None,
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolVersionMismatch)
    );
}

#[test]
fn fatal_envelope_no_error_code_accessor_returns_none() {
    let env = make_fatal(None, "generic error");
    assert!(env.error_code().is_none());
}

#[test]
fn fatal_envelope_empty_error_validator_error() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: String::new(),
        error_code: None,
    };
    let result = v.validate(&env);
    assert!(!result.valid);
}

#[test]
fn fatal_envelope_missing_ref_id_validator_warns() {
    let v = EnvelopeValidator::new();
    let env = make_fatal(None, "error msg");
    let result = v.validate(&env);
    assert!(result.valid); // warning, not error
    assert!(result.warnings.iter().any(|w| matches!(
        w,
        ValidationWarning::MissingOptionalField { field } if field == "ref_id"
    )));
}

#[test]
fn fatal_from_abp_error() {
    let abp_err =
        abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "backend timed out");
    let env = Envelope::fatal_from_abp_error(Some("run-1".into()), &abp_err);
    match &env {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "backend timed out");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
        }
        _ => panic!("expected Fatal"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. ref_id correlation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ref_id_mismatch_event_detected() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_event(
            "wrong-id",
            AgentEventKind::AssistantDelta { text: "x".into() },
        ),
        make_final(&id),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn ref_id_mismatch_final_detected() {
    let v = EnvelopeValidator::new();
    let (_, run) = make_run();
    let seq = vec![make_hello(), run, make_final("wrong-id")];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn ref_id_mismatch_fatal_detected() {
    let v = EnvelopeValidator::new();
    let (_, run) = make_run();
    let seq = vec![make_hello(), run, make_fatal(Some("wrong-id"), "err")];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

#[test]
fn ref_id_correct_sequence_no_errors() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_event(&id, AgentEventKind::AssistantDelta { text: "hi".into() }),
        make_event(
            &id,
            AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
        ),
        make_final(&id),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn ref_id_fatal_with_none_ref_id_no_mismatch() {
    let v = EnvelopeValidator::new();
    let (_, run) = make_run();
    // Fatal with None ref_id should not trigger RefIdMismatch
    let seq = vec![make_hello(), run, make_fatal(None, "crash")];
    let errors = v.validate_sequence(&seq);
    assert!(
        !errors
            .iter()
            .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Protocol version negotiation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v0"), None);
    assert_eq!(parse_version("abp/v0."), None);
    assert_eq!(parse_version("abp/v.1"), None);
    assert_eq!(parse_version("invalid"), None);
}

#[test]
fn is_compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn is_incompatible_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn is_compatible_invalid_returns_false() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
    assert!(!is_compatible_version("invalid", "invalid"));
}

#[test]
fn protocol_version_parse_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn protocol_version_parse_invalid() {
    assert!(ProtocolVersion::parse("invalid").is_err());
    assert!(ProtocolVersion::parse("abp/v").is_err());
    assert!(ProtocolVersion::parse("abp/v0").is_err());
}

#[test]
fn protocol_version_display() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(format!("{v}"), "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn protocol_version_compatibility() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(v01.is_compatible(&v02)); // remote newer minor is ok
    assert!(!v02.is_compatible(&v01)); // remote older minor is not
    assert!(!v01.is_compatible(&v10)); // different major
}

#[test]
fn negotiate_version_same_major_picks_min() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result, v01);
}

#[test]
fn negotiate_version_different_major_errors() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(negotiate_version(&v01, &v10).is_err());
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
}

#[test]
fn version_range_compatible_same_major() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_unicode_in_task() {
    let wo = WorkOrderBuilder::new("修复登录漏洞 🐛 — résumé café").build();
    let run = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let decoded = roundtrip(&run);
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert!(work_order.task.contains("修复登录漏洞"));
            assert!(work_order.task.contains("🐛"));
            assert!(work_order.task.contains("café"));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn edge_unicode_in_error() {
    let env = make_fatal(Some("r"), "ошибка: файл не найден 📁");
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("ошибка"));
            assert!(error.contains("📁"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn edge_unicode_in_backend_id() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "sidecar-日本語".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "sidecar-日本語");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn edge_empty_event_text() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantDelta {
            text: String::new(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(
                matches!(event.kind, AgentEventKind::AssistantDelta { text } if text.is_empty())
            );
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn edge_very_long_text() {
    let long_text = "a".repeat(100_000);
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: long_text.clone(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text.len(), 100_000);
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn edge_special_chars_in_strings() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "line1\nline2\ttab\\backslash\"quote".into(),
        },
    );
    let decoded = roundtrip(&env);
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains('\n'));
                assert!(text.contains('\t'));
                assert!(text.contains('\\'));
                assert!(text.contains('"'));
            }
            _ => panic!("expected AssistantMessage"),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn edge_null_bytes_in_json_value() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "test".into(),
            tool_use_id: None,
            output: serde_json::json!({"data": "before\u{0000}after"}),
            is_error: false,
        },
    );
    // Should successfully encode and decode
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn edge_deeply_nested_json_in_tool_input() {
    let mut val = serde_json::json!({"leaf": true});
    for _ in 0..20 {
        val = serde_json::json!({"nested": val});
    }
    let env = make_event(
        "run-1",
        AgentEventKind::ToolCall {
            tool_name: "complex".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: val,
        },
    );
    let decoded = roundtrip(&env);
    assert!(matches!(decoded, Envelope::Event { .. }));
}

#[test]
fn edge_many_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    caps.insert(Capability::ToolGlob, SupportLevel::Emulated);
    caps.insert(Capability::ToolGrep, SupportLevel::Emulated);
    caps.insert(Capability::ToolWebSearch, SupportLevel::Unsupported);
    caps.insert(Capability::ToolWebFetch, SupportLevel::Unsupported);
    caps.insert(Capability::McpClient, SupportLevel::Native);
    caps.insert(Capability::McpServer, SupportLevel::Emulated);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Native);
    let hello = make_hello_with_caps(caps.clone());
    let decoded = roundtrip(&hello);
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), caps.len());
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Ordering guarantees
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ordering_missing_hello() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![run, make_final(&id)];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
}

#[test]
fn ordering_missing_terminal() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_event(&id, AgentEventKind::AssistantDelta { text: "x".into() }),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn ordering_multiple_terminals() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_final(&id),
        make_fatal(Some(&id), "extra"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MultipleTerminals))
    );
}

#[test]
fn ordering_event_before_run() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        make_event(&id, AgentEventKind::AssistantDelta { text: "x".into() }),
        run,
        make_final(&id),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::OutOfOrderEvents))
    );
}

#[test]
fn ordering_event_after_final() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_final(&id),
        make_event(
            &id,
            AgentEventKind::AssistantDelta {
                text: "late".into(),
            },
        ),
    ];
    let errors = v.validate_sequence(&seq);
    // Should flag out-of-order or multiple terminals
    assert!(!errors.is_empty());
}

#[test]
fn ordering_empty_sequence() {
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&[]);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingHello))
    );
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn ordering_hello_only() {
    let v = EnvelopeValidator::new();
    let seq = vec![make_hello()];
    let errors = v.validate_sequence(&seq);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, SequenceError::MissingTerminal))
    );
}

#[test]
fn ordering_valid_full_sequence() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_event(
            &id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event(
            &id,
            AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
        ),
        make_event(
            &id,
            AgentEventKind::AssistantDelta {
                text: " World".into(),
            },
        ),
        make_event(
            &id,
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
        ),
        make_event(
            &id,
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!("ok"),
                is_error: false,
            },
        ),
        make_event(
            &id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        make_final(&id),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

#[test]
fn ordering_valid_fatal_sequence() {
    let v = EnvelopeValidator::new();
    let (id, run) = make_run();
    let seq = vec![
        make_hello(),
        run,
        make_event(
            &id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_fatal(Some(&id), "crashed"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Decode stream (BufRead-based)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_stream_multiple_envelopes() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let fatal = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    let input = format!("{hello}{fatal}");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let fatal = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();
    let input = format!("\n\n{hello}\n\n{fatal}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_invalid_line_returns_error() {
    let input = "not valid json\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Encode/decode to writer
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_to_writer_roundtrip() {
    let env = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let line = String::from_utf8(buf).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn encode_many_to_writer() {
    let envs = vec![
        make_hello(),
        make_fatal(None, "e1"),
        make_fatal(Some("r"), "e2"),
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

// ═══════════════════════════════════════════════════════════════════════
// 14. ProtocolError variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("bad json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_error_violation_has_code() {
    let err = ProtocolError::Violation("test violation".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_code() {
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
fn protocol_error_display() {
    let err = ProtocolError::Violation("bad state".into());
    let msg = format!("{err}");
    assert!(msg.contains("bad state"));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Builder API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builder_hello_minimal() {
    use abp_protocol::builder::EnvelopeBuilder;
    let env = EnvelopeBuilder::hello()
        .backend("my-sidecar")
        .build()
        .unwrap();
    match env {
        Envelope::Hello {
            contract_version,
            backend,
            mode,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "my-sidecar");
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn builder_hello_missing_backend_errors() {
    use abp_protocol::builder::EnvelopeBuilder;
    let result = EnvelopeBuilder::hello().build();
    assert!(result.is_err());
}

#[test]
fn builder_fatal() {
    use abp_protocol::builder::EnvelopeBuilder;
    let env = EnvelopeBuilder::fatal("kaboom")
        .ref_id("run-1")
        .build()
        .unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "kaboom");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn builder_event_missing_ref_id_errors() {
    use abp_protocol::builder::EnvelopeBuilder;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "x".into() },
        ext: None,
    };
    let result = EnvelopeBuilder::event(event).build();
    assert!(result.is_err());
}

#[test]
fn builder_final_missing_ref_id_errors() {
    use abp_protocol::builder::EnvelopeBuilder;
    let result = EnvelopeBuilder::final_receipt(test_receipt()).build();
    assert!(result.is_err());
}

#[test]
fn builder_run_uses_work_order_id() {
    use abp_protocol::builder::EnvelopeBuilder;
    let wo = test_work_order();
    let expected_id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    match env {
        Envelope::Run { id, .. } => {
            assert_eq!(id, expected_id);
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Streaming codec (batch)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn streaming_codec_encode_decode_batch() {
    use abp_protocol::codec::StreamingCodec;
    let envs = vec![
        make_hello(),
        make_fatal(None, "e1"),
        make_fatal(Some("r"), "e2"),
    ];
    let batch = StreamingCodec::encode_batch(&envs);
    let decoded = StreamingCodec::decode_batch(&batch);
    assert_eq!(decoded.len(), 3);
    for r in &decoded {
        assert!(r.is_ok());
    }
}

#[test]
fn streaming_codec_line_count() {
    use abp_protocol::codec::StreamingCodec;
    let envs = vec![make_hello(), make_fatal(None, "e")];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(StreamingCodec::line_count(&batch), 2);
}

#[test]
fn streaming_codec_validate_detects_bad_lines() {
    use abp_protocol::codec::StreamingCodec;
    let good = JsonlCodec::encode(&make_hello()).unwrap();
    let input = format!("{good}not valid json\n");
    let errors = StreamingCodec::validate_jsonl(&input);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, 2); // 1-based line number
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Cross-cutting serde guarantees
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_envelope_variants_use_tag_t() {
    let envs: Vec<Envelope> = vec![
        make_hello(),
        Envelope::Run {
            id: "r".into(),
            work_order: test_work_order(),
        },
        make_event("r", AgentEventKind::AssistantDelta { text: "x".into() }),
        make_final("r"),
        make_fatal(None, "e"),
    ];
    for env in &envs {
        let json = JsonlCodec::encode(env).unwrap();
        assert!(json.contains(r#""t":"#), "missing tag t in: {json}");
    }
}

#[test]
fn agent_event_kind_uses_tag_type() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "x".into() },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"assistant_delta""#));
}

#[test]
fn capability_manifest_btreemap_deterministic_order() {
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::ToolWrite, SupportLevel::Native);
    caps1.insert(Capability::Streaming, SupportLevel::Native);
    caps1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::Streaming, SupportLevel::Native);
    caps2.insert(Capability::ToolRead, SupportLevel::Native);
    caps2.insert(Capability::ToolWrite, SupportLevel::Native);

    let j1 = serde_json::to_string(&caps1).unwrap();
    let j2 = serde_json::to_string(&caps2).unwrap();
    assert_eq!(j1, j2, "BTreeMap should produce deterministic order");
}

#[test]
fn execution_mode_default_is_mapped() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_roundtrip() {
    let mapped: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(mapped, ExecutionMode::Mapped);
    let pt: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(pt, ExecutionMode::Passthrough);
}

#[test]
fn outcome_serde_roundtrip() {
    let c: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(c, Outcome::Complete);
    let p: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(p, Outcome::Partial);
    let f: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(f, Outcome::Failed);
}

#[test]
fn receipt_builder_produces_valid_receipt() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}
