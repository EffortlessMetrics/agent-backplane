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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive sidecar protocol compliance tests covering the full JSONL
//! protocol lifecycle: hello → run → event* → final/fatal.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionLane,
    ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder, SupportLevel, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use abp_protocol::{
    is_compatible_version, parse_version,
    validate::{EnvelopeValidator, SequenceError, ValidationError},
    Envelope, JsonlCodec, ProtocolError,
};
use chrono::Utc;
use uuid::Uuid;

// =========================================================================
// Helpers
// =========================================================================

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps
}

fn test_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Test task").build()
}

fn test_receipt() -> Receipt {
    ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build()
}

fn make_hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

fn make_run(wo: &WorkOrder) -> Envelope {
    Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    }
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

// =========================================================================
// 1. Hello envelope tests (10)
// =========================================================================

#[test]
fn hello_valid_roundtrip() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_contains_contract_version() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(&format!("\"contract_version\":\"{CONTRACT_VERSION}\"")));
}

#[test]
fn hello_discriminator_is_t() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("\"t\":\"hello\""));
    // Must NOT use "type" as discriminator at protocol level
    assert!(!json.contains("\"type\":\"hello\""));
}

#[test]
fn hello_ends_with_newline() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn hello_default_mode_is_mapped() {
    let hello = make_hello();
    if let Envelope::Hello { mode, .. } = hello {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn hello_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("\"mode\":\"passthrough\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn hello_backend_identity_preserved() {
    let hello = make_hello();
    if let Envelope::Hello { backend, .. } = &hello {
        assert_eq!(backend.id, "test-sidecar");
        assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
        assert_eq!(backend.adapter_version.as_deref(), Some("0.1.0"));
    } else {
        panic!("expected Hello");
    }
    // Roundtrip
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.id, "test-sidecar");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_capabilities_preserved() {
    let hello = make_hello();
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert_eq!(capabilities.len(), 2);
        assert!(capabilities.contains_key(&Capability::ToolRead));
        assert!(capabilities.contains_key(&Capability::Streaming));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_empty_capabilities() {
    let hello = Envelope::hello(test_backend(), BTreeMap::new());
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello { capabilities, .. } = decoded {
        assert!(capabilities.is_empty());
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_invalid_json_fails() {
    let bad = r#"{"t":"hello","contract_version":"abp/v0.1"}"#;
    // Missing required fields backend + capabilities
    let result = JsonlCodec::decode(bad);
    assert!(result.is_err());
}

// =========================================================================
// 2. Run envelope tests (10)
// =========================================================================

#[test]
fn run_valid_roundtrip() {
    let wo = test_work_order();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run {
        id,
        work_order: decoded_wo,
    } = decoded
    {
        assert_eq!(id, wo.id.to_string());
        assert_eq!(decoded_wo.task, "Test task");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_discriminator_is_t() {
    let run = make_run(&test_work_order());
    let json = JsonlCodec::encode(&run).unwrap();
    assert!(json.contains("\"t\":\"run\""));
}

#[test]
fn run_preserves_work_order_id() {
    let wo = test_work_order();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.id, wo.id);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_task() {
    let wo = WorkOrderBuilder::new("Fix the auth module").build();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.task, "Fix the auth module");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_execution_lane() {
    let wo = WorkOrderBuilder::new("test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert!(matches!(work_order.lane, ExecutionLane::WorkspaceFirst));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        deny_write: vec!["*.secret".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("test").policy(policy).build();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.policy.allowed_tools, vec!["read", "write"]);
        assert_eq!(work_order.policy.deny_write, vec!["*.secret"]);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_config_model() {
    let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_max_turns() {
    let wo = WorkOrderBuilder::new("test").max_turns(10).build();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.config.max_turns, Some(10));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_workspace_spec() {
    let wo = WorkOrderBuilder::new("test")
        .root("/tmp/ws")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["*.log".into()])
        .build();
    let run = make_run(&wo);
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Run { work_order, .. } = decoded {
        assert_eq!(work_order.workspace.root, "/tmp/ws");
        assert!(matches!(work_order.workspace.mode, WorkspaceMode::Staged));
        assert_eq!(work_order.workspace.include, vec!["src/**"]);
        assert_eq!(work_order.workspace.exclude, vec!["*.log"]);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_ref_id_matches_work_order_id() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let run = make_run(&wo);
    if let Envelope::Run { id, .. } = &run {
        assert_eq!(id, &run_id);
    } else {
        panic!("expected Run");
    }
}

// =========================================================================
// 3. Event streaming tests (15)
// =========================================================================

#[test]
fn event_run_started() {
    let env = make_event(
        "run-1",
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\":\"event\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "run-1");
        assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_run_completed() {
    let env = make_event(
        "run-1",
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
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
fn event_assistant_delta() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantDelta { text } = event.kind {
            assert_eq!(text, "Hello");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_message() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "Hello, world!".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, "Hello, world!");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } = event.kind
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
            assert_eq!(input["path"], "src/main.rs");
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::ToolResult {
            tool_name,
            is_error,
            ..
        } = event.kind
        {
            assert_eq!(tool_name, "read_file");
            assert!(!is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_error() {
    let env = make_event(
        "run-1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!({"error": "permission denied"}),
            is_error: true,
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
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
fn event_file_changed() {
    let env = make_event(
        "run-1",
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added function".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::FileChanged { path, summary } = event.kind {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(summary, "Added function");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed() {
    let env = make_event(
        "run-1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } = event.kind
        {
            assert_eq!(command, "cargo test");
            assert_eq!(exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("test result: ok"));
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_warning() {
    let env = make_event(
        "run-1",
        AgentEventKind::Warning {
            message: "rate limit approaching".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Warning { message } = event.kind {
            assert_eq!(message, "rate limit approaching");
        } else {
            panic!("expected Warning");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error() {
    let env = make_event(
        "run-1",
        AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: Some(abp_error::ErrorCode::ExecutionToolFailed),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::Error {
            message,
            error_code,
        } = event.kind
        {
            assert_eq!(message, "something went wrong");
            assert_eq!(error_code, Some(abp_error::ErrorCode::ExecutionToolFailed));
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_ref_id_preserved() {
    let ref_id = Uuid::new_v4().to_string();
    let env = make_event(
        &ref_id,
        AgentEventKind::AssistantDelta { text: "hi".into() },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event {
        ref_id: decoded_ref,
        ..
    } = decoded
    {
        assert_eq!(decoded_ref, ref_id);
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_agent_event_kind_uses_type_tag() {
    // AgentEventKind uses #[serde(tag = "type")] (NOT "t")
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "test".into(),
        },
    );
    let json = JsonlCodec::encode(&env).unwrap();
    // The event payload should contain "type":"assistant_message"
    assert!(json.contains("\"type\":\"assistant_message\""));
}

#[test]
fn event_stream_decode_multiple() {
    let events: Vec<Envelope> = vec![
        make_event(
            "r1",
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
        ),
        make_event(
            "r1",
            AgentEventKind::AssistantDelta {
                text: "tok1".into(),
            },
        ),
        make_event(
            "r1",
            AgentEventKind::AssistantDelta {
                text: "tok2".into(),
            },
        ),
    ];
    let mut buf = Vec::new();
    for env in &events {
        let line = JsonlCodec::encode(env).unwrap();
        buf.extend_from_slice(line.as_bytes());
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

#[test]
fn event_with_extension_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor_field": "value"}),
    );
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "test".into(),
            },
            ext: Some(ext),
        },
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        assert!(event.ext.is_some());
        let ext = event.ext.unwrap();
        assert!(ext.contains_key("raw_message"));
    } else {
        panic!("expected Event");
    }
}

// =========================================================================
// 4. Final/Fatal tests (10)
// =========================================================================

#[test]
fn final_valid_roundtrip() {
    let fin = make_final("run-1");
    let json = JsonlCodec::encode(&fin).unwrap();
    assert!(json.contains("\"t\":\"final\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

#[test]
fn final_ref_id_preserved() {
    let fin = make_final("my-run-id");
    let json = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { ref_id, .. } = decoded {
        assert_eq!(ref_id, "my-run-id");
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_outcome_preserved() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Partial)
        .build();
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&fin).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.outcome, Outcome::Partial);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_hash_verification() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);

    // Verify deterministic hashing
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash, &recomputed);
}

#[test]
fn final_receipt_hash_excludes_self() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let hash1 = abp_core::receipt_hash(&receipt).unwrap();
    let mut receipt2 = receipt;
    receipt2.receipt_sha256 = Some("should-be-ignored".into());
    let hash2 = abp_core::receipt_hash(&receipt2).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn fatal_valid_roundtrip() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(json.contains("\"t\":\"fatal\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "out of memory");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_error_code() {
    let fatal = Envelope::fatal_with_code(
        Some("run-1".into()),
        "timed out",
        abp_error::ErrorCode::BackendTimeout,
    );
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(json.contains("\"error_code\":\"backend_timeout\""));
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::BackendTimeout)
    );
}

#[test]
fn fatal_error_code_serializes_snake_case() {
    let codes = vec![
        (abp_error::ErrorCode::BackendTimeout, "backend_timeout"),
        (abp_error::ErrorCode::Internal, "internal"),
        (
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
            "protocol_invalid_envelope",
        ),
        (
            abp_error::ErrorCode::ProtocolHandshakeFailed,
            "protocol_handshake_failed",
        ),
    ];
    for (code, expected_str) in codes {
        assert_eq!(code.as_str(), expected_str);
        let fatal = Envelope::fatal_with_code(None, "test", code);
        let json = JsonlCodec::encode(&fatal).unwrap();
        assert!(
            json.contains(&format!("\"error_code\":\"{expected_str}\"")),
            "ErrorCode::{code:?} should serialize as \"{expected_str}\", got: {json}"
        );
    }
}

#[test]
fn fatal_without_ref_id() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Fatal { ref_id, .. } = decoded {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_from_abp_error() {
    let err = abp_error::AbpError::new(abp_error::ErrorCode::BackendCrashed, "process exited");
    let fatal = Envelope::fatal_from_abp_error(Some("run-1".into()), &err);
    if let Envelope::Fatal {
        error, error_code, ..
    } = &fatal
    {
        assert_eq!(error, "process exited");
        assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendCrashed));
    } else {
        panic!("expected Fatal");
    }
}

// =========================================================================
// 5. Protocol state machine tests (15)
// =========================================================================

#[test]
fn state_valid_complete_sequence() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        make_run(&wo),
        make_event(
            &run_id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event(
            &run_id,
            AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
        ),
        make_final(&run_id),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn state_hello_must_be_first() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![make_run(&wo), make_hello(), make_final(&run_id)];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::HelloNotFirst { .. })));
}

#[test]
fn state_missing_hello() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![make_run(&wo), make_final(&run_id)];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingHello)));
}

#[test]
fn state_missing_terminal() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        make_run(&wo),
        make_event(
            &run_id,
            AgentEventKind::AssistantDelta { text: "hi".into() },
        ),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingTerminal)));
}

#[test]
fn state_multiple_terminals() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        make_run(&wo),
        make_final(&run_id),
        Envelope::Fatal {
            ref_id: Some(run_id.clone()),
            error: "oops".into(),
            error_code: None,
        },
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MultipleTerminals)));
}

#[test]
fn state_ref_id_mismatch_event() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        make_run(&wo),
        make_event(
            "wrong-ref-id",
            AgentEventKind::AssistantDelta { text: "hi".into() },
        ),
        make_final(&run_id),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn state_ref_id_mismatch_final() {
    let wo = test_work_order();
    let seq = vec![make_hello(), make_run(&wo), make_final("wrong-ref-id")];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::RefIdMismatch { .. })));
}

#[test]
fn state_event_before_run_is_out_of_order() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        make_event(
            &run_id,
            AgentEventKind::AssistantDelta { text: "hi".into() },
        ),
        make_run(&wo),
        make_final(&run_id),
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
}

#[test]
fn state_empty_sequence() {
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
fn state_hello_only() {
    let seq = vec![make_hello()];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors
        .iter()
        .any(|e| matches!(e, SequenceError::MissingTerminal)));
}

#[test]
fn state_valid_sequence_with_fatal() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let seq = vec![
        make_hello(),
        make_run(&wo),
        Envelope::Fatal {
            ref_id: Some(run_id),
            error: "crash".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        },
    ];
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn state_many_events_valid() {
    let wo = test_work_order();
    let run_id = wo.id.to_string();
    let mut seq = vec![make_hello(), make_run(&wo)];
    for i in 0..50 {
        seq.push(make_event(
            &run_id,
            AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
        ));
    }
    seq.push(make_final(&run_id));
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn state_validator_individual_hello_valid() {
    let hello = make_hello();
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn state_validator_hello_empty_backend_id() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        ValidationError::EmptyField { field } if field == "backend.id"
    )));
}

#[test]
fn state_validator_hello_invalid_version() {
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_backend(),
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
    };
    let validator = EnvelopeValidator::new();
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
}

// =========================================================================
// 6. Backward compatibility tests (10)
// =========================================================================

#[test]
fn compat_unknown_fields_ignored_on_decode() {
    // JSON with extra fields should still decode correctly
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored","another":42}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert_eq!(error, "boom");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn compat_version_parse_v0_1() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
}

#[test]
fn compat_version_parse_v2_3() {
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn compat_version_parse_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn compat_same_major_compatible() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.99"));
}

#[test]
fn compat_different_major_incompatible() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
}

#[test]
fn compat_invalid_version_not_compatible() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
}

#[test]
fn compat_contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    let (major, minor) = parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

#[test]
fn compat_decode_stream_skips_blank_lines() {
    let input = "\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn compat_hello_mode_defaults_when_missing() {
    // Simulate a hello from an older sidecar that doesn't send "mode"
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"old-sidecar","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let decoded = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

// =========================================================================
// Additional edge-case tests for full coverage (6)
// =========================================================================

#[test]
fn codec_encode_to_writer() {
    let hello = make_hello();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("\"t\":\"hello\""));
}

#[test]
fn codec_encode_many_to_writer() {
    let envelopes = vec![make_hello(), make_final("r1")];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = s.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn invalid_json_returns_protocol_error() {
    let err = JsonlCodec::decode("not valid json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn unknown_envelope_type_fails() {
    let json = r#"{"t":"unknown_type","data":"foo"}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn protocol_error_codes() {
    let violation = ProtocolError::Violation("test".into());
    assert_eq!(
        violation.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );

    let unexpected = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(
        unexpected.error_code(),
        Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
    );
}

#[test]
fn full_lifecycle_jsonl_stream() {
    // Simulate a complete sidecar session as a JSONL stream
    let wo = test_work_order();
    let run_id = wo.id.to_string();

    let sequence = vec![
        make_hello(),
        make_run(&wo),
        make_event(
            &run_id,
            AgentEventKind::RunStarted {
                message: "starting".into(),
            },
        ),
        make_event(
            &run_id,
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/lib.rs"}),
            },
        ),
        make_event(
            &run_id,
            AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
        ),
        make_event(
            &run_id,
            AgentEventKind::AssistantMessage {
                text: "I read the file".into(),
            },
        ),
        make_event(
            &run_id,
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "refactored".into(),
            },
        ),
        make_event(
            &run_id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
        make_final(&run_id),
    ];

    // Encode the entire sequence
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &sequence).unwrap();

    // Decode it back
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), sequence.len());

    // Validate the sequence
    let validator = EnvelopeValidator::new();
    let errors = validator.validate_sequence(&decoded);
    assert!(errors.is_empty(), "lifecycle errors: {errors:?}");

    // Verify envelope types in order
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    for env in &decoded[2..8] {
        assert!(matches!(env, Envelope::Event { .. }));
    }
    assert!(matches!(decoded[8], Envelope::Final { .. }));
}
