// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the protocol envelope system (abp-protocol).
//!
//! Covers: construction, tag discrimination, parsing, roundtrip, Hello/Run/Event/Final/Fatal
//! envelopes, JSONL format, invalid envelopes, ref_id handling, extension fields, large payloads.

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
        task: "deep-test-task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/workspace".into(),
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
        backend: mk_backend("test-backend"),
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

fn roundtrip(env: &Envelope) -> Envelope {
    let encoded = JsonlCodec::encode(env).unwrap();
    JsonlCodec::decode(encoded.trim_end()).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Envelope construction — all five variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn construct_hello_via_helper() {
    let env = Envelope::hello(mk_backend("sidecar-1"), mk_caps());
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn construct_hello_with_mode_passthrough() {
    let env = Envelope::hello_with_mode(
        mk_backend("sidecar-2"),
        mk_caps(),
        ExecutionMode::Passthrough,
    );
    if let Envelope::Hello { mode, .. } = &env {
        assert_eq!(*mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn construct_run_variant() {
    let env = Envelope::Run {
        id: "run-abc".into(),
        work_order: mk_work_order(),
    };
    assert!(matches!(env, Envelope::Run { .. }));
}

#[test]
fn construct_event_variant() {
    let env = Envelope::Event {
        ref_id: "run-abc".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
    };
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn construct_final_variant() {
    let env = Envelope::Final {
        ref_id: "run-abc".into(),
        receipt: mk_receipt(),
    };
    assert!(matches!(env, Envelope::Final { .. }));
}

#[test]
fn construct_fatal_variant_with_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some("run-abc".into()),
        error: "crashed".into(),
        error_code: None,
    };
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn construct_fatal_variant_without_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    if let Envelope::Fatal { ref_id, .. } = &env {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn construct_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-x".into()),
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    );
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Tag discrimination — verify "t" field in JSON, NOT "type"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tag_field_is_t_not_type_for_hello() {
    let env = Envelope::hello(mk_backend("x"), BTreeMap::new());
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "hello");
    assert!(v.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_for_run() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "run");
    assert!(v.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_for_event() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: "tok".into(),
        }),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "event");
    assert!(v.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_for_final() {
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt: mk_receipt(),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "final");
    assert!(v.get("type").is_none());
}

#[test]
fn tag_field_is_t_not_type_for_fatal() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
        error_code: None,
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "fatal");
    assert!(v.get("type").is_none());
}

#[test]
fn all_tag_values_are_snake_case() {
    let variants: Vec<Envelope> = vec![
        Envelope::hello(mk_backend("x"), BTreeMap::new()),
        Envelope::Run {
            id: "r".into(),
            work_order: mk_work_order(),
        },
        Envelope::Event {
            ref_id: "r".into(),
            event: mk_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        },
        Envelope::Final {
            ref_id: "r".into(),
            receipt: mk_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e".into(),
            error_code: None,
        },
    ];
    let expected_tags = ["hello", "run", "event", "final", "fatal"];
    for (env, expected) in variants.iter().zip(expected_tags.iter()) {
        let v = serde_json::to_value(env).unwrap();
        assert_eq!(v["t"].as_str().unwrap(), *expected);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Envelope parsing — parse from raw JSON strings via JsonlCodec::decode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_hello_from_json_string() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"node","backend_version":"2.0","adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello {
        contract_version,
        backend,
        ..
    } = env
    {
        assert_eq!(contract_version, "abp/v0.1");
        assert_eq!(backend.id, "node");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn parse_fatal_from_json_string() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"sidecar died"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal {
        ref_id, error, error_code,
    } = env
    {
        assert!(ref_id.is_none());
        assert_eq!(error, "sidecar died");
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn parse_event_assistant_delta_from_json_string() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_delta","text":"hello"}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Event { ref_id, event } = env {
        assert_eq!(ref_id, "r1");
        if let AgentEventKind::AssistantDelta { text } = event.kind {
            assert_eq!(text, "hello");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Roundtrip — serialize → deserialize for all envelope types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_hello_preserves_backend_identity() {
    let backend = BackendIdentity {
        id: "rt-backend".into(),
        backend_version: Some("3.2.1".into()),
        adapter_version: Some("0.8.0".into()),
    };
    let env = Envelope::hello(backend, mk_caps());
    match roundtrip(&env) {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "rt-backend");
            assert_eq!(backend.backend_version.as_deref(), Some("3.2.1"));
            assert_eq!(backend.adapter_version.as_deref(), Some("0.8.0"));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn roundtrip_run_preserves_work_order_fields() {
    let mut wo = mk_work_order();
    wo.task = "roundtrip task".into();
    wo.lane = ExecutionLane::WorkspaceFirst;
    let env = Envelope::Run {
        id: "run-rt".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-rt");
            assert_eq!(work_order.task, "roundtrip task");
            assert!(matches!(work_order.lane, ExecutionLane::WorkspaceFirst));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn roundtrip_event_preserves_tool_call_details() {
    let env = Envelope::Event {
        ref_id: "run-rt".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-99".into()),
            parent_tool_use_id: Some("tu-98".into()),
            input: serde_json::json!({"path": "/etc/hosts"}),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-rt");
            if let AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                input,
            } = event.kind
            {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tu-99"));
                assert_eq!(parent_tool_use_id.as_deref(), Some("tu-98"));
                assert_eq!(input["path"], "/etc/hosts");
            } else {
                panic!("expected ToolCall");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn roundtrip_final_preserves_receipt_outcome() {
    let mut receipt = mk_receipt();
    receipt.outcome = Outcome::Partial;
    receipt.meta.duration_ms = 5000;
    let env = Envelope::Final {
        ref_id: "run-rt".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-rt");
            assert!(matches!(receipt.outcome, Outcome::Partial));
            assert_eq!(receipt.meta.duration_ms, 5000);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn roundtrip_fatal_preserves_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-rt".into()),
        "backend crashed",
        abp_error::ErrorCode::BackendCrashed,
    );
    match roundtrip(&env) {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-rt"));
            assert_eq!(error, "backend crashed");
            assert_eq!(error_code, Some(abp_error::ErrorCode::BackendCrashed));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Hello envelope — backend, version, capabilities fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_sets_contract_version_from_constant() {
    let env = Envelope::hello(mk_backend("v"), BTreeMap::new());
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["contract_version"].as_str().unwrap(), CONTRACT_VERSION);
}

#[test]
fn hello_capabilities_with_restricted_level() {
    let mut caps = BTreeMap::new();
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let env = Envelope::hello(mk_backend("r"), caps);
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("restricted"));
    assert!(json.contains("sandboxed"));
}

#[test]
fn hello_capabilities_with_all_support_levels() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
    caps.insert(
        Capability::ToolEdit,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    );
    let env = Envelope::hello(mk_backend("all"), caps);
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 4);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Run envelope — work order embedding, ref_id generation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_embeds_full_work_order() {
    let mut wo = mk_work_order();
    wo.config.model = Some("gpt-4".into());
    wo.config.max_turns = Some(10);
    let env = Envelope::Run {
        id: "run-wo".into(),
        work_order: wo,
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["work_order"]["config"]["model"], "gpt-4");
    assert_eq!(v["work_order"]["config"]["max_turns"], 10);
}

#[test]
fn run_id_preserved_in_serialization() {
    let env = Envelope::Run {
        id: "unique-run-id-12345".into(),
        work_order: mk_work_order(),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["id"].as_str().unwrap(), "unique-run-id-12345");
}

#[test]
fn run_work_order_with_context_files() {
    let mut wo = mk_work_order();
    wo.context.files = vec!["src/main.rs".into(), "Cargo.toml".into()];
    let env = Envelope::Run {
        id: "run-ctx".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.files.len(), 2);
            assert_eq!(work_order.context.files[0], "src/main.rs");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_with_policy_restrictions() {
    let mut wo = mk_work_order();
    wo.policy.disallowed_tools = vec!["bash".into(), "write".into()];
    wo.policy.deny_write = vec!["*.lock".into()];
    let env = Envelope::Run {
        id: "run-pol".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.policy.disallowed_tools.len(), 2);
            assert_eq!(work_order.policy.deny_write[0], "*.lock");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Event envelope — AgentEvent embedding, ref_id correlation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_run_started_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-1".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "beginning work".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "corr-1");
            if let AgentEventKind::RunStarted { message } = event.kind {
                assert_eq!(message, "beginning work");
            } else {
                panic!("expected RunStarted");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_run_completed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-2".into(),
        event: mk_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_assistant_message_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-3".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: "Here is the fix.".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::AssistantMessage { text } = event.kind {
                assert_eq!(text, "Here is the fix.");
            } else {
                panic!("expected AssistantMessage");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_result_with_error_flag() {
    let env = Envelope::Event {
        ref_id: "corr-4".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-5".into()),
            output: serde_json::json!({"stderr": "command not found"}),
            is_error: true,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::ToolResult { is_error, .. } = event.kind {
                assert!(is_error);
            } else {
                panic!("expected ToolResult");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_file_changed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-5".into(),
        event: mk_event(AgentEventKind::FileChanged {
            path: "lib/auth.rs".into(),
            summary: "added JWT validation".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::FileChanged { path, summary } = event.kind {
                assert_eq!(path, "lib/auth.rs");
                assert_eq!(summary, "added JWT validation");
            } else {
                panic!("expected FileChanged");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_command_executed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-6".into(),
        event: mk_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(1),
            output_preview: Some("test failed".into()),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } = event.kind
            {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(1));
                assert_eq!(output_preview.as_deref(), Some("test failed"));
            } else {
                panic!("expected CommandExecuted");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_warning_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-7".into(),
        event: mk_event(AgentEventKind::Warning {
            message: "budget running low".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::Warning { message } = event.kind {
                assert_eq!(message, "budget running low");
            } else {
                panic!("expected Warning");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_error_kind_with_code_roundtrip() {
    let env = Envelope::Event {
        ref_id: "corr-8".into(),
        event: mk_event(AgentEventKind::Error {
            message: "permission denied".into(),
            error_code: Some(abp_error::ErrorCode::PolicyDenied),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::Error {
                message,
                error_code,
            } = event.kind
            {
                assert_eq!(message, "permission denied");
                assert_eq!(error_code, Some(abp_error::ErrorCode::PolicyDenied));
            } else {
                panic!("expected Error");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Final envelope — receipt embedding
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn final_receipt_outcome_failed() {
    let mut receipt = mk_receipt();
    receipt.outcome = Outcome::Failed;
    let env = Envelope::Final {
        ref_id: "run-f".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert!(matches!(receipt.outcome, Outcome::Failed));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_with_trace_events() {
    let mut receipt = mk_receipt();
    receipt.trace = vec![
        mk_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        mk_event(AgentEventKind::AssistantDelta {
            text: "tok".into(),
        }),
        mk_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let env = Envelope::Final {
        ref_id: "run-tr".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.trace.len(), 3);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_with_artifacts() {
    let mut receipt = mk_receipt();
    receipt.artifacts = vec![ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    }];
    let env = Envelope::Final {
        ref_id: "run-art".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.artifacts.len(), 1);
            assert_eq!(receipt.artifacts[0].kind, "patch");
            assert_eq!(receipt.artifacts[0].path, "output.diff");
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_with_usage_tokens() {
    let mut receipt = mk_receipt();
    receipt.usage.input_tokens = Some(1000);
    receipt.usage.output_tokens = Some(500);
    receipt.usage.estimated_cost_usd = Some(0.015);
    let env = Envelope::Final {
        ref_id: "run-usg".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.usage.input_tokens, Some(1000));
            assert_eq!(receipt.usage.output_tokens, Some(500));
            assert_eq!(receipt.usage.estimated_cost_usd, Some(0.015));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Fatal envelope — error_code, message, ref_id
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_error_code_serialized_when_present() {
    let env = Envelope::fatal_with_code(
        Some("run-ec".into()),
        "auth failed",
        abp_error::ErrorCode::BackendAuthFailed,
    );
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["error_code"], "backend_auth_failed");
    assert_eq!(v["error"], "auth failed");
}

#[test]
fn fatal_error_code_absent_when_none() {
    let env = Envelope::Fatal {
        ref_id: Some("run-nocode".into()),
        error: "unknown".into(),
        error_code: None,
    };
    let v = serde_json::to_value(&env).unwrap();
    // error_code should be omitted via skip_serializing_if
    assert!(v.get("error_code").is_none());
}

#[test]
fn fatal_with_null_ref_id_in_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"early crash"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = env {
        assert!(ref_id.is_none());
        assert_eq!(error, "early crash");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_error_code_method_returns_none_for_non_fatal() {
    let env = Envelope::hello(mk_backend("x"), BTreeMap::new());
    assert!(env.error_code().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. JSONL format — each envelope is exactly one line
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn encode_produces_exactly_one_newline() {
    let env = Envelope::hello(mk_backend("line"), mk_caps());
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert_eq!(encoded.matches('\n').count(), 1);
    assert!(encoded.ends_with('\n'));
}

#[test]
fn encode_produces_valid_single_line_json() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let trimmed = encoded.trim_end();
    // Must not contain internal newlines
    assert!(!trimmed.contains('\n'));
    // Must be valid JSON
    let _: Value = serde_json::from_str(trimmed).unwrap();
}

#[test]
fn decode_stream_reads_multiple_lines() {
    let hello = Envelope::hello(mk_backend("s"), BTreeMap::new());
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "e".into(),
        error_code: None,
    };
    let mut buf = String::new();
    buf.push_str(&JsonlCodec::encode(&hello).unwrap());
    buf.push_str(&JsonlCodec::encode(&fatal).unwrap());

    let reader = BufReader::new(buf.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let fatal = JsonlCodec::encode(&Envelope::Fatal {
        ref_id: None,
        error: "e".into(),
        error_code: None,
    })
    .unwrap();
    let input = format!("\n\n{fatal}\n\n{fatal}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn encode_to_writer_produces_valid_jsonl() {
    let env = Envelope::hello(mk_backend("w"), mk_caps());
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    assert_eq!(output.matches('\n').count(), 1);
}

#[test]
fn encode_many_to_writer_multiple_lines() {
    let envs = vec![
        Envelope::hello(mk_backend("m1"), BTreeMap::new()),
        Envelope::Fatal {
            ref_id: None,
            error: "err".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Invalid envelopes — missing tag, wrong tag value, malformed JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn invalid_missing_tag_field() {
    let result = JsonlCodec::decode(r#"{"data":"value"}"#);
    assert!(result.is_err());
}

#[test]
fn invalid_wrong_tag_value() {
    let result = JsonlCodec::decode(r#"{"t":"nonexistent_variant","data":"value"}"#);
    assert!(result.is_err());
}

#[test]
fn invalid_malformed_json() {
    let result = JsonlCodec::decode("{not valid json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn invalid_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn invalid_tag_as_number() {
    let result = JsonlCodec::decode(r#"{"t":42}"#);
    assert!(result.is_err());
}

#[test]
fn invalid_tag_as_null() {
    let result = JsonlCodec::decode(r#"{"t":null}"#);
    assert!(result.is_err());
}

#[test]
fn invalid_tag_as_boolean() {
    let result = JsonlCodec::decode(r#"{"t":true}"#);
    assert!(result.is_err());
}

#[test]
fn invalid_array_body() {
    let result = JsonlCodec::decode("[1,2,3]");
    assert!(result.is_err());
}

#[test]
fn invalid_null_body() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

#[test]
fn invalid_tag_uses_type_instead_of_t() {
    // Using "type" instead of "t" should fail
    let json = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn invalid_hello_missing_required_fields() {
    let result = JsonlCodec::decode(r#"{"t":"hello"}"#);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. ref_id handling — UUID format, correlation between envelopes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ref_id_uuid_format_preserved() {
    let uuid_str = Uuid::new_v4().to_string();
    let env = Envelope::Event {
        ref_id: uuid_str.clone(),
        event: mk_event(AgentEventKind::AssistantDelta { text: "x".into() }),
    };
    match roundtrip(&env) {
        Envelope::Event { ref_id, .. } => {
            assert_eq!(ref_id, uuid_str);
            // Verify it's a valid UUID
            assert!(Uuid::parse_str(&ref_id).is_ok());
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn ref_id_correlation_across_event_and_final() {
    let run_id = "correlated-run-42";

    let event_env = Envelope::Event {
        ref_id: run_id.into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: "working".into(),
        }),
    };
    let final_env = Envelope::Final {
        ref_id: run_id.into(),
        receipt: mk_receipt(),
    };

    let event_decoded = roundtrip(&event_env);
    let final_decoded = roundtrip(&final_env);

    let event_ref = match &event_decoded {
        Envelope::Event { ref_id, .. } => ref_id.as_str(),
        _ => panic!("expected Event"),
    };
    let final_ref = match &final_decoded {
        Envelope::Final { ref_id, .. } => ref_id.as_str(),
        _ => panic!("expected Final"),
    };

    assert_eq!(event_ref, final_ref);
    assert_eq!(event_ref, run_id);
}

#[test]
fn ref_id_correlation_run_to_events() {
    let run_id = "run-session-xyz";

    let run_env = Envelope::Run {
        id: run_id.into(),
        work_order: mk_work_order(),
    };
    let event_env = Envelope::Event {
        ref_id: run_id.into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
    };

    let run_id_decoded = match roundtrip(&run_env) {
        Envelope::Run { id, .. } => id,
        other => panic!("expected Run, got {other:?}"),
    };
    let event_ref_decoded = match roundtrip(&event_env) {
        Envelope::Event { ref_id, .. } => ref_id,
        other => panic!("expected Event, got {other:?}"),
    };

    assert_eq!(run_id_decoded, event_ref_decoded);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Extension fields — ext field passthrough
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_ext_field_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"vendor_field": "vendor_value"}),
    );
    ext.insert("custom_key".into(), serde_json::json!(42));

    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(ext),
    };
    let env = Envelope::Event {
        ref_id: "ext-test".into(),
        event,
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            let ext = event.ext.expect("ext should be present");
            assert_eq!(ext["raw_message"]["vendor_field"], "vendor_value");
            assert_eq!(ext["custom_key"], 42);
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_ext_none_omitted_in_json() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "no ext".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "ext-none".into(),
        event,
    };
    let json = serde_json::to_string(&env).unwrap();
    // ext should be omitted via skip_serializing_if
    assert!(!json.contains("\"ext\""));
}

#[test]
fn event_ext_with_nested_objects() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "deeply_nested".into(),
        serde_json::json!({
            "level1": {
                "level2": {
                    "level3": [1, 2, 3]
                }
            }
        }),
    );
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "nested".into(),
        },
        ext: Some(ext),
    };
    let env = Envelope::Event {
        ref_id: "ext-deep".into(),
        event,
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            let ext = event.ext.expect("ext should be present");
            assert_eq!(ext["deeply_nested"]["level1"]["level2"]["level3"][2], 3);
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Large payloads — large content in events/work orders
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_assistant_message_roundtrip() {
    let large_text = "A".repeat(100_000);
    let env = Envelope::Event {
        ref_id: "run-large".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: large_text.clone(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::AssistantMessage { text } = event.kind {
                assert_eq!(text.len(), 100_000);
                assert_eq!(text, large_text);
            } else {
                panic!("expected AssistantMessage");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn large_tool_call_input_roundtrip() {
    let large_input: serde_json::Value =
        serde_json::json!({"data": "X".repeat(50_000)});
    let env = Envelope::Event {
        ref_id: "run-large-tool".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu-big".into()),
            parent_tool_use_id: None,
            input: large_input.clone(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            if let AgentEventKind::ToolCall { input, .. } = event.kind {
                assert_eq!(input["data"].as_str().unwrap().len(), 50_000);
            } else {
                panic!("expected ToolCall");
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn large_work_order_with_many_context_files() {
    let mut wo = mk_work_order();
    wo.context.files = (0..500).map(|i| format!("src/file_{i}.rs")).collect();
    let env = Envelope::Run {
        id: "run-many-files".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.files.len(), 500);
            assert_eq!(work_order.context.files[499], "src/file_499.rs");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn large_receipt_with_many_trace_events() {
    let mut receipt = mk_receipt();
    receipt.trace = (0..200)
        .map(|i| {
            mk_event(AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            })
        })
        .collect();
    let env = Envelope::Final {
        ref_id: "run-big-trace".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.trace.len(), 200);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn large_payload_is_single_jsonl_line() {
    let large_text = "B".repeat(100_000);
    let env = Envelope::Event {
        ref_id: "run-sl".into(),
        event: mk_event(AgentEventKind::AssistantMessage { text: large_text }),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert_eq!(encoded.matches('\n').count(), 1);
    assert!(encoded.ends_with('\n'));
}
