// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp".into(),
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

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps
}

fn test_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn test_receipt() -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: test_backend(),
        capabilities: test_capabilities(),
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

// ── 1. Hello envelope ───────────────────────────────────────────────────

#[test]
fn hello_serialize_has_correct_tag() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "hello");
}

#[test]
fn hello_roundtrip() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Hello {
        contract_version,
        backend,
        capabilities,
        mode,
    } = back
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
        assert_eq!(backend.id, "test-backend");
        assert!(!capabilities.is_empty());
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello variant");
    }
}

#[test]
fn hello_contract_version_field() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["contract_version"], CONTRACT_VERSION);
}

#[test]
fn hello_with_mode_passthrough() {
    let env = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["mode"], "passthrough");
}

#[test]
fn hello_without_mode_defaults_to_mapped() {
    // Deserialize JSON that omits the "mode" field — should default to Mapped.
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": { "id": "x", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    let env: Envelope = serde_json::from_value(raw).unwrap();
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello variant");
    }
}

// ── 2. Run envelope ─────────────────────────────────────────────────────

#[test]
fn run_serialize_has_correct_tag() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: test_work_order(),
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "run");
}

#[test]
fn run_roundtrip() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: test_work_order(),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Run { id, work_order } = back {
        assert_eq!(id, "run-1");
        assert_eq!(work_order.task, "test");
        assert_eq!(work_order.id, Uuid::nil());
    } else {
        panic!("expected Run variant");
    }
}

// ── 3. Event envelope ───────────────────────────────────────────────────

#[test]
fn event_serialize_has_correct_tag() {
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event: test_agent_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "event");
}

#[test]
fn event_ref_id_correlation() {
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event: test_agent_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Event { ref_id, .. } = back {
        assert_eq!(ref_id, "run-42");
    } else {
        panic!("expected Event variant");
    }
}

#[test]
fn event_with_tool_call() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls"}),
        }),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::ToolCall { tool_name, .. } = event.kind {
            assert_eq!(tool_name, "bash");
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event variant");
    }
}

#[test]
fn event_with_tool_result() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!("file list"),
            is_error: false,
        }),
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["event"]["type"], "tool_result");
}

#[test]
fn event_with_file_changed() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        }),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::FileChanged { path, summary } = event.kind {
            assert_eq!(path, "src/main.rs");
            assert_eq!(summary, "added function");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event variant");
    }
}

#[test]
fn event_with_warning() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::Warning {
            message: "budget low".into(),
        }),
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["event"]["type"], "warning");
    assert_eq!(json["event"]["message"], "budget low");
}

#[test]
fn event_with_error_kind() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        }),
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["event"]["type"], "error");
}

#[test]
fn event_with_command_executed() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        }),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Event { event, .. } = back {
        if let AgentEventKind::CommandExecuted {
            command, exit_code, ..
        } = event.kind
        {
            assert_eq!(command, "cargo build");
            assert_eq!(exit_code, Some(0));
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event variant");
    }
}

// ── 4. Final envelope ───────────────────────────────────────────────────

#[test]
fn final_serialize_has_correct_tag() {
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: test_receipt(),
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "final");
}

#[test]
fn final_roundtrip_preserves_ref_id() {
    let env = Envelope::Final {
        ref_id: "run-77".into(),
        receipt: test_receipt(),
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Final { ref_id, receipt } = back {
        assert_eq!(ref_id, "run-77");
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Final variant");
    }
}

// ── 5. Fatal envelope ──────────────────────────────────────────────────

#[test]
fn fatal_serialize_has_correct_tag() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something broke".into(),
        error_code: None,
    };
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "fatal");
}

#[test]
fn fatal_with_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: None,
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = back {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "crash");
    } else {
        panic!("expected Fatal variant");
    }
}

#[test]
fn fatal_without_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "early failure".into(),
        error_code: None,
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Envelope = serde_json::from_str(&s).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = back {
        assert!(ref_id.is_none());
        assert_eq!(error, "early failure");
    } else {
        panic!("expected Fatal variant");
    }
}

// ── 6. JsonlCodec::encode ──────────────────────────────────────────────

#[test]
fn encode_ends_with_newline() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn encode_produces_valid_json() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let trimmed = encoded.trim_end();
    let _: Value = serde_json::from_str(trimmed).expect("encoded output must be valid JSON");
}

#[test]
fn encode_is_single_line() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: test_work_order(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    // Only one newline, at the very end
    assert_eq!(encoded.matches('\n').count(), 1);
    assert!(encoded.ends_with('\n'));
}

// ── 7. JsonlCodec::decode ──────────────────────────────────────────────

#[test]
fn decode_valid_hello() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn decode_invalid_json_returns_error() {
    let result = JsonlCodec::decode("{not json}");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_wrong_tag_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"nonexistent","data":1}"#);
    assert!(result.is_err());
}

#[test]
fn decode_missing_tag_returns_error() {
    let result = JsonlCodec::decode(r#"{"data":1}"#);
    assert!(result.is_err());
}

// ── 8. Envelope::hello constructor ─────────────────────────────────────

#[test]
fn hello_constructor_default_mode_is_mapped() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello variant");
    }
}

#[test]
fn hello_constructor_sets_contract_version() {
    let env = Envelope::hello(test_backend(), BTreeMap::new());
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello variant");
    }
}

// ── 9. Envelope::hello_with_mode ───────────────────────────────────────

#[test]
fn hello_with_mode_passthrough_explicit() {
    let env = Envelope::hello_with_mode(
        test_backend(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello variant");
    }
}

#[test]
fn hello_with_mode_mapped_explicit() {
    let env = Envelope::hello_with_mode(test_backend(), test_capabilities(), ExecutionMode::Mapped);
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello variant");
    }
}

// ── 10. Round-trip all variants ────────────────────────────────────────

#[test]
fn roundtrip_hello() {
    let env = Envelope::hello(test_backend(), test_capabilities());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn roundtrip_run() {
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: test_work_order(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "r1");
        assert_eq!(work_order.task, "test");
    } else {
        panic!("expected Run variant");
    }
}

#[test]
fn roundtrip_event() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: test_agent_event(AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        }),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Envelope::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "r1");
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, "hello world");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event variant");
    }
}

#[test]
fn roundtrip_final() {
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt: test_receipt(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Envelope::Final { ref_id, receipt } = decoded {
        assert_eq!(ref_id, "r1");
        assert!(matches!(receipt.outcome, Outcome::Complete));
    } else {
        panic!("expected Final variant");
    }
}

#[test]
fn roundtrip_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert_eq!(ref_id.as_deref(), Some("r1"));
        assert_eq!(error, "boom");
    } else {
        panic!("expected Fatal variant");
    }
}

// ── 11. Malformed input ────────────────────────────────────────────────

#[test]
fn decode_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_partial_json() {
    let result = JsonlCodec::decode(
        r#"{"t":"hello","contract_version":#);
    assert!(result.is_err());
}

#[test]
fn decode_wrong_tag_value() {
    let result = JsonlCodec::decode(r#"{"t":"bogus","data":"value"}"#,
    );
    assert!(result.is_err());
}

#[test]
fn decode_tag_as_number() {
    let result = JsonlCodec::decode(r#"{"t":42}"#);
    assert!(result.is_err());
}

#[test]
fn decode_null_body() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

#[test]
fn decode_array_body() {
    let result = JsonlCodec::decode("[1,2,3]");
    assert!(result.is_err());
}

// ── 12. Wire format stability ──────────────────────────────────────────

#[test]
fn wire_format_hello_from_fixed_json() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"node-sidecar","backend_version":"2.0","adapter_version":null},"capabilities":{"streaming":"native"},"mode":"passthrough"}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Hello {
        contract_version,
        backend,
        capabilities,
        mode,
    } = env
    {
        assert_eq!(contract_version, "abp/v0.1");
        assert_eq!(backend.id, "node-sidecar");
        assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
        assert!(capabilities.contains_key(&Capability::Streaming));
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn wire_format_fatal_from_fixed_json() {
    let json = r#"{"t":"fatal","ref_id":"abc-123","error":"sidecar crashed"}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = env {
        assert_eq!(ref_id.as_deref(), Some("abc-123"));
        assert_eq!(error, "sidecar crashed");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn wire_format_fatal_null_ref_id() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"early"}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Fatal { ref_id, .. } = env {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn wire_format_event_assistant_delta() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_delta","text":"hi"}}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Event { ref_id, event } = env {
        assert_eq!(ref_id, "r1");
        if let AgentEventKind::AssistantDelta { text } = event.kind {
            assert_eq!(text, "hi");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn wire_format_run_from_fixed_json() {
    let json = r#"{"t":"run","id":"run-99","work_order":{"id":"00000000-0000-0000-0000-000000000000","task":"hello","lane":"patch_first","workspace":{"root":"/tmp","mode":"pass_through","include":[],"exclude":[]},"context":{"files":[],"snippets":[]},"policy":{"allowed_tools":[],"disallowed_tools":[],"deny_read":[],"deny_write":[],"allow_network":[],"deny_network":[],"require_approval_for":[]},"requirements":{"required":[]},"config":{"model":null,"vendor":{},"env":{},"max_budget_usd":null,"max_turns":null}}}"#;
    let env: Envelope = serde_json::from_str(json).unwrap();
    if let Envelope::Run { id, work_order } = env {
        assert_eq!(id, "run-99");
        assert_eq!(work_order.task, "hello");
    } else {
        panic!("expected Run");
    }
}

// ── ProtocolError display ──────────────────────────────────────────────

#[test]
fn protocol_error_json_display() {
    let err = JsonlCodec::decode("{bad}").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"), "got: {msg}");
}
