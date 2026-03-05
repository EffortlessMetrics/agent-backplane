#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the JSONL wire protocol (Envelope serde, JSONL
//! line parsing, stream decoding, malformed input, ref_id correlation, and
//! hello/run/event/final/fatal payloads).

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    ReceiptBuilder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_hello_passthrough() -> Envelope {
    Envelope::hello_with_mode(
        BackendIdentity {
            id: "passthrough-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    )
}

fn make_run() -> Envelope {
    let wo = WorkOrderBuilder::new("test task").build();
    Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    }
}

fn make_event(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

fn make_fatal(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

// =========================================================================
// 1. Envelope serialization — discriminator field is "t"
// =========================================================================

#[test]
fn hello_envelope_has_t_discriminator() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn run_envelope_has_t_discriminator() {
    let json = JsonlCodec::encode(&make_run()).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn event_envelope_has_t_discriminator() {
    let json = JsonlCodec::encode(&make_event("r1")).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn final_envelope_has_t_discriminator() {
    let json = JsonlCodec::encode(&make_final("r1")).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn fatal_envelope_has_t_discriminator() {
    let json = JsonlCodec::encode(&make_fatal(None, "boom")).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn discriminator_is_t_not_type() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    // Must use "t", NOT "type" at the envelope level.
    assert!(json.contains(r#""t":"#));
    // "type" may appear inside nested payloads (AgentEventKind) but NOT as
    // the top-level envelope discriminator.
    let top_level: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert!(top_level.get("t").is_some());
}

// =========================================================================
// 2. JSONL line parsing — one envelope per line
// =========================================================================

#[test]
fn encode_appends_newline() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn encode_is_single_line() {
    let json = JsonlCodec::encode(&make_run()).unwrap();
    let line_count = json.trim().lines().count();
    assert_eq!(line_count, 1, "encoded envelope should be exactly one line");
}

#[test]
fn decode_accepts_trimmed_line() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn decode_accepts_line_with_trailing_newline() {
    // decode should work when the trailing newline is absent
    let raw = r#"{"t":"fatal","ref_id":null,"error":"oops"}"#;
    let env = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

// =========================================================================
// 3. Malformed JSON handling
// =========================================================================

#[test]
fn decode_rejects_empty_string() {
    let err = JsonlCodec::decode("").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_plain_text() {
    let err = JsonlCodec::decode("not json at all").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_incomplete_json() {
    let err = JsonlCodec::decode(r#"{"t":"hello","contract_version":"abp/v0.1""#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_json_array() {
    let err = JsonlCodec::decode(r#"[1,2,3]"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_json_number() {
    let err = JsonlCodec::decode("42").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_json_string() {
    let err = JsonlCodec::decode(r#""hello""#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_unknown_discriminator() {
    let err = JsonlCodec::decode(r#"{"t":"unknown_type","data":123}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_missing_discriminator() {
    let err = JsonlCodec::decode(r#"{"ref_id":"r1","error":"boom"}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_null_json() {
    let err = JsonlCodec::decode("null").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_rejects_boolean_json() {
    let err = JsonlCodec::decode("true").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

// =========================================================================
// 4. Envelope ref_id correlation
// =========================================================================

#[test]
fn event_preserves_ref_id() {
    let env = make_event("run-42");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn final_preserves_ref_id() {
    let env = make_final("run-42");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "run-42"),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn fatal_with_ref_id_preserves_it() {
    let env = make_fatal(Some("run-99"), "kaboom");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id.as_deref(), Some("run-99")),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_without_ref_id_is_none() {
    let env = make_fatal(None, "kaboom");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn run_id_preserved_roundtrip() {
    let env = make_run();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { id, .. } => assert_eq!(id, "run-1"),
        other => panic!("expected Run, got {other:?}"),
    }
}

// =========================================================================
// 5. Hello envelope — contract version validation
// =========================================================================

#[test]
fn hello_has_contract_version() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn hello_roundtrip_contract_version() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, CONTRACT_VERSION),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_default_mode_is_mapped() {
    let env = make_hello();
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Mapped),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_passthrough_mode_roundtrips() {
    let env = make_hello_passthrough();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_backend_identity_roundtrips() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "test-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            assert!(backend.adapter_version.is_none());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_with_capabilities_roundtrips() {
    use abp_core::{Capability, SupportLevel};

    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let env = Envelope::hello(
        BackendIdentity {
            id: "caps-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.contains_key(&Capability::ToolRead));
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_empty_capabilities_roundtrips() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_from_raw_json_with_current_contract() {
    let raw = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"raw","backend_version":null,"adapter_version":null}},"capabilities":{{}},"mode":"mapped"}}"#,
        CONTRACT_VERSION
    );
    let env = JsonlCodec::decode(&raw).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn hello_mode_defaults_when_absent() {
    // mode is #[serde(default)] so omitting it should yield Mapped
    let raw = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"x","backend_version":null,"adapter_version":null}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION
    );
    let decoded = JsonlCodec::decode(&raw).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        other => panic!("expected Hello, got {other:?}"),
    }
}

// =========================================================================
// 6. Run envelope with WorkOrder payload
// =========================================================================

#[test]
fn run_contains_work_order_task() {
    let env = make_run();
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("test task"));
}

#[test]
fn run_roundtrip_preserves_task() {
    let env = make_run();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => assert_eq!(work_order.task, "test task"),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_with_model_config_roundtrips() {
    let wo = WorkOrderBuilder::new("model task").model("gpt-4").build();
    let env = Envelope::Run {
        id: "run-model".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_with_max_turns_roundtrips() {
    let wo = WorkOrderBuilder::new("limited task").max_turns(5).build();
    let env = Envelope::Run {
        id: "run-lim".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.max_turns, Some(5));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_id_is_uuid() {
    let env = make_run();
    match &env {
        Envelope::Run { work_order, .. } => {
            assert!(!work_order.id.is_nil(), "work order id should not be nil");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// =========================================================================
// 7. Event envelope with AgentEvent kinds
// =========================================================================

#[test]
fn event_assistant_message_roundtrips() {
    let env = make_event("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { .. }
            ));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_call_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(input["path"], "src/main.rs");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_result_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
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
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_file_changed_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Added function".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(summary, "Added function");
            }
            other => panic!("expected FileChanged, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_command_executed_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
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
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command, exit_code, ..
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_warning_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "watch out".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "watch out"),
            other => panic!("expected Warning, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_error_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "bad things".into(),
                error_code: None,
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { message, .. } => assert_eq!(message, "bad things"),
            other => panic!("expected Error, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_run_started_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_run_completed_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_assistant_delta_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_with_ext_data_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), serde_json::json!({"sdk":"data"}));
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "msg".into() },
            ext: Some(ext),
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert_eq!(ext["raw_message"]["sdk"], "data");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_without_ext_omits_field() {
    let env = make_event("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    // ext is skip_serializing_if = "Option::is_none"
    assert!(!json.contains(r#""ext""#));
}

#[test]
fn event_type_discriminator_inside_event_is_type() {
    // AgentEventKind uses #[serde(tag = "type")] — different from envelope "t"
    let env = make_event("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    let event_val = &v["event"];
    assert!(event_val.get("type").is_some(), "nested event uses 'type'");
    assert_eq!(event_val["type"], "assistant_message");
}

// =========================================================================
// 8. Final envelope with Receipt
// =========================================================================

#[test]
fn final_roundtrip_preserves_outcome() {
    let env = make_final("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Complete),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_backend_identity_roundtrips() {
    let env = make_final("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.backend.id, "mock"),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_has_contract_version() {
    let env = make_final("r1");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_partial_outcome() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_failed_outcome() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_with_trace_events() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(ev)
        .build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.trace.len(), 1);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// =========================================================================
// 9. Fatal envelope with error message
// =========================================================================

#[test]
fn fatal_error_message_roundtrips() {
    let env = make_fatal(Some("r1"), "out of memory");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "out of memory"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_with_null_ref_id_from_raw_json() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"crash"}"#;
    let env = JsonlCodec::decode(raw).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "crash");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_with_error_code_roundtrips() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "bad envelope",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("error_code"));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error_code, .. } => {
            assert!(error_code.is_some());
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_without_error_code_skips_field() {
    let env = make_fatal(None, "simple error");
    let json = JsonlCodec::encode(&env).unwrap();
    // error_code is skip_serializing_if = "Option::is_none"
    assert!(!json.contains("error_code"));
}

#[test]
fn fatal_empty_error_string() {
    let env = make_fatal(None, "");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert!(error.is_empty()),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_unicode_error_message() {
    let env = make_fatal(None, "エラーが発生しました 🚨");
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "エラーが発生しました 🚨"),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    assert!(env.error_code().is_some());

    let env2 = make_fatal(None, "err");
    assert!(env2.error_code().is_none());
}

// =========================================================================
// 10. Multi-line JSONL stream parsing
// =========================================================================

#[test]
fn decode_stream_multiple_envelopes() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let event = JsonlCodec::encode(&make_event("r1")).unwrap();
    let fatal = JsonlCodec::encode(&make_fatal(Some("r1"), "oops")).unwrap();

    let stream = format!("{hello}{event}{fatal}");
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 3);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
    assert!(matches!(envelopes[2], Envelope::Fatal { .. }));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let fatal = JsonlCodec::encode(&make_fatal(None, "err")).unwrap();

    let stream = format!("{hello}\n\n  \n{fatal}");
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn decode_stream_only_blank_lines() {
    let reader = BufReader::new("\n\n\n".as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(envelopes.is_empty());
}

#[test]
fn decode_stream_stops_on_error() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let stream = format!("{hello}invalid json line\n");
    let reader = BufReader::new(stream.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn decode_stream_full_handshake_sequence() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let run = JsonlCodec::encode(&make_run()).unwrap();
    let ev1 = JsonlCodec::encode(&make_event("run-1")).unwrap();
    let ev2 = JsonlCodec::encode(&Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "tok".into() },
            ext: None,
        },
    })
    .unwrap();
    let fin = JsonlCodec::encode(&make_final("run-1")).unwrap();

    let stream = format!("{hello}{run}{ev1}{ev2}{fin}");
    let reader = BufReader::new(stream.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 5);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Run { .. }));
    assert!(matches!(envelopes[2], Envelope::Event { .. }));
    assert!(matches!(envelopes[3], Envelope::Event { .. }));
    assert!(matches!(envelopes[4], Envelope::Final { .. }));
}

#[test]
fn encode_to_writer_roundtrips() {
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &make_hello()).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    let decoded = JsonlCodec::decode(output.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn encode_many_to_writer_produces_valid_stream() {
    let envelopes = vec![make_hello(), make_event("r1"), make_final("r1")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

#[test]
fn decode_stream_single_line() {
    let hello = JsonlCodec::encode(&make_hello()).unwrap();
    let reader = BufReader::new(hello.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

// =========================================================================
// Additional edge cases and cross-cutting concerns
// =========================================================================

#[test]
fn version_parse_current_contract() {
    let parsed = abp_protocol::parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn version_parse_higher_minor() {
    assert_eq!(abp_protocol::parse_version("abp/v0.2"), Some((0, 2)));
}

#[test]
fn version_parse_invalid_string() {
    assert_eq!(abp_protocol::parse_version("invalid"), None);
}

#[test]
fn version_compatible_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
}

#[test]
fn version_incompatible_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn version_incompatible_garbage() {
    assert!(!abp_protocol::is_compatible_version("garbage", "abp/v0.1"));
}

#[test]
fn protocol_error_json_variant() {
    let err = JsonlCodec::decode("bad").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"), "got: {msg}");
}

#[test]
fn protocol_error_display_is_descriptive() {
    let err = ProtocolError::Violation("test violation".into());
    let msg = format!("{err}");
    assert!(msg.contains("test violation"));
}

#[test]
fn envelope_clone_equality_after_roundtrip() {
    let env = make_hello();
    let cloned = env.clone();
    let json1 = JsonlCodec::encode(&env).unwrap();
    let json2 = JsonlCodec::encode(&cloned).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn large_work_order_roundtrips() {
    let wo = WorkOrderBuilder::new("a".repeat(10_000)).build();
    let env = Envelope::Run {
        id: "run-big".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => assert_eq!(work_order.task.len(), 10_000),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn special_characters_in_error_message() {
    let env = make_fatal(None, r#"line1\nline2 "quoted" & <tagged>"#);
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("quoted"));
            assert!(error.contains("<tagged>"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn event_timestamp_survives_roundtrip() {
    let now = Utc::now();
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: "ts test".into(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            // chrono DateTime roundtrips may lose sub-nanosecond precision but
            // should be within 1ms
            let diff = (event.ts - now).num_milliseconds().unsigned_abs();
            assert!(diff < 1, "timestamp drift: {diff}ms");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn tool_call_with_nested_json_input() {
    let input = serde_json::json!({
        "nested": {"deep": {"value": [1, 2, 3]}},
        "flag": true
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "complex_tool".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: input.clone(),
            },
            ext: None,
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                input: decoded_input,
                ..
            } => {
                assert_eq!(decoded_input, input);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn tool_result_error_flag_roundtrips() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
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
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn decode_stream_preserves_envelope_order() {
    let mut lines = String::new();
    for i in 0..20 {
        let env = make_event(&format!("run-{i}"));
        lines.push_str(&JsonlCodec::encode(&env).unwrap());
    }
    let reader = BufReader::new(lines.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 20);
    for (i, env) in envelopes.iter().enumerate() {
        match env {
            Envelope::Event { ref_id, .. } => {
                assert_eq!(ref_id, &format!("run-{i}"));
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }
}

#[test]
fn mixed_envelope_types_in_stream() {
    let envelopes = vec![
        make_hello(),
        make_run(),
        make_event("run-1"),
        make_event("run-1"),
        make_event("run-1"),
        make_final("run-1"),
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 6);
}
