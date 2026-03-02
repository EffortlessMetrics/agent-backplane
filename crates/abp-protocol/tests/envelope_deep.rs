// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the JSONL protocol envelope system.

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
        task: "deep-test".into(),
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
        backend: mk_backend("test"),
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

/// Encode → decode roundtrip helper.
fn roundtrip(env: &Envelope) -> Envelope {
    let encoded = JsonlCodec::encode(env).unwrap();
    JsonlCodec::decode(encoded.trim_end()).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Hello variant — tag, fields, roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_tag_is_hello() {
    let env = Envelope::hello(mk_backend("a"), mk_caps());
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "hello");
}

#[test]
fn hello_roundtrip_preserves_all_fields() {
    let env = Envelope::hello(mk_backend("deep"), mk_caps());
    match roundtrip(&env) {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "deep");
            assert_eq!(backend.backend_version.as_deref(), Some("1.0.0"));
            assert!(backend.adapter_version.is_none());
            assert!(!capabilities.is_empty());
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_contract_version_matches_constant() {
    let env = Envelope::hello(mk_backend("x"), BTreeMap::new());
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["contract_version"].as_str().unwrap(), CONTRACT_VERSION);
}

#[test]
fn hello_with_mode_passthrough_roundtrip() {
    let env = Envelope::hello_with_mode(mk_backend("p"), mk_caps(), ExecutionMode::Passthrough);
    match roundtrip(&env) {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_mode_defaults_to_mapped_when_absent() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": { "id": "x", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    match serde_json::from_value::<Envelope>(raw).unwrap() {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_empty_capabilities_roundtrip() {
    let env = Envelope::hello(mk_backend("e"), BTreeMap::new());
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_many_capabilities_roundtrip() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    let env = Envelope::hello(mk_backend("m"), caps);
    match roundtrip(&env) {
        Envelope::Hello { capabilities, .. } => assert_eq!(capabilities.len(), 5),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_backend_all_versions_populated() {
    let backend = BackendIdentity {
        id: "full".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.5.0".into()),
    };
    let env = Envelope::hello(backend, BTreeMap::new());
    match roundtrip(&env) {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "full");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("0.5.0"));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_backend_no_versions() {
    let backend = BackendIdentity {
        id: "bare".into(),
        backend_version: None,
        adapter_version: None,
    };
    let env = Envelope::hello(backend, BTreeMap::new());
    match roundtrip(&env) {
        Envelope::Hello { backend, .. } => {
            assert!(backend.backend_version.is_none());
            assert!(backend.adapter_version.is_none());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Run variant — tag, fields, roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_tag_is_run() {
    let env = Envelope::Run {
        id: "r1".into(),
        work_order: mk_work_order(),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "run");
}

#[test]
fn run_roundtrip_preserves_id_and_task() {
    let env = Envelope::Run {
        id: "run-deep".into(),
        work_order: mk_work_order(),
    };
    match roundtrip(&env) {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-deep");
            assert_eq!(work_order.task, "deep-test");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_uuid_preserved() {
    let mut wo = mk_work_order();
    let specific = Uuid::new_v4();
    wo.id = specific;
    let env = Envelope::Run {
        id: "r".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => assert_eq!(work_order.id, specific),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_with_context() {
    let mut wo = mk_work_order();
    wo.context = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    };
    let env = Envelope::Run {
        id: "ctx".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.context.files.len(), 1);
            assert_eq!(work_order.context.snippets.len(), 1);
            assert_eq!(work_order.context.snippets[0].name, "readme");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_with_policy() {
    let mut wo = mk_work_order();
    wo.policy = PolicyProfile {
        allowed_tools: vec!["bash".into(), "read".into()],
        disallowed_tools: vec!["write".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/*".into()],
        allow_network: vec![],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["bash".into()],
    };
    let env = Envelope::Run {
        id: "pol".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.policy.allowed_tools.len(), 2);
            assert_eq!(work_order.policy.deny_network.len(), 1);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_with_config() {
    let mut wo = mk_work_order();
    wo.config = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("temperature".into(), serde_json::json!(0.7));
            m
        },
        env: {
            let mut m = BTreeMap::new();
            m.insert("API_KEY".into(), "secret".into());
            m
        },
        max_budget_usd: Some(1.5),
        max_turns: Some(10),
    };
    let env = Envelope::Run {
        id: "cfg".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
            assert_eq!(work_order.config.max_turns, Some(10));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_work_order_workspace_staged_mode() {
    let mut wo = mk_work_order();
    wo.workspace.mode = WorkspaceMode::Staged;
    wo.workspace.include = vec!["src/**".into()];
    wo.workspace.exclude = vec!["*.log".into()];
    let env = Envelope::Run {
        id: "ws".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.workspace.include.len(), 1);
            assert_eq!(work_order.workspace.exclude.len(), 1);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Event variant — all AgentEventKind sub-types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_tag_is_event() {
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn event_run_started_roundtrip() {
    let env = Envelope::Event {
        ref_id: "rs".into(),
        event: mk_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "rs");
            match event.kind {
                AgentEventKind::RunStarted { message } => assert_eq!(message, "starting"),
                other => panic!("expected RunStarted, got {other:?}"),
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_run_completed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "rc".into(),
        event: mk_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::RunCompleted { message } => assert_eq!(message, "done"),
            other => panic!("expected RunCompleted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_assistant_delta_roundtrip() {
    let env = Envelope::Event {
        ref_id: "ad".into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: "token".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "token"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_assistant_message_roundtrip() {
    let env = Envelope::Event {
        ref_id: "am".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: "Hello, world!".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello, world!"),
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_call_roundtrip() {
    let env = Envelope::Event {
        ref_id: "tc".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls -la"}),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                input,
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert!(parent_tool_use_id.is_none());
                assert_eq!(input["cmd"], "ls -la");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_call_with_parent_id() {
    let env = Envelope::Event {
        ref_id: "tcp".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu-2".into()),
            parent_tool_use_id: Some("tu-1".into()),
            input: serde_json::json!({"path": "main.rs"}),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                parent_tool_use_id, ..
            } => {
                assert_eq!(parent_tool_use_id.as_deref(), Some("tu-1"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_result_roundtrip() {
    let env = Envelope::Event {
        ref_id: "tr".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!("file.txt\ndir/"),
            is_error: false,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                output,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert!(!is_error);
                assert_eq!(output, serde_json::json!("file.txt\ndir/"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_result_error_flag() {
    let env = Envelope::Event {
        ref_id: "tre".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: serde_json::json!("permission denied"),
            is_error: true,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_file_changed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "fc".into(),
        event: mk_event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added tests".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(summary, "added tests");
            }
            other => panic!("expected FileChanged, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_command_executed_roundtrip() {
    let env = Envelope::Event {
        ref_id: "ce".into(),
        event: mk_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("42 passed".into()),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
                assert_eq!(output_preview.as_deref(), Some("42 passed"));
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_command_executed_no_exit_code() {
    let env = Envelope::Event {
        ref_id: "cen".into(),
        event: mk_event(AgentEventKind::CommandExecuted {
            command: "kill -9".into(),
            exit_code: None,
            output_preview: None,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                exit_code,
                output_preview,
                ..
            } => {
                assert!(exit_code.is_none());
                assert!(output_preview.is_none());
            }
            other => panic!("expected CommandExecuted, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_warning_roundtrip() {
    let env = Envelope::Event {
        ref_id: "w".into(),
        event: mk_event(AgentEventKind::Warning {
            message: "budget low".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "budget low"),
            other => panic!("expected Warning, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_error_kind_roundtrip() {
    let env = Envelope::Event {
        ref_id: "ek".into(),
        event: mk_event(AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error {
                message,
                error_code,
            } => {
                assert_eq!(message, "oops");
                assert!(error_code.is_none());
            }
            other => panic!("expected Error, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_error_kind_with_code() {
    let env = Envelope::Event {
        ref_id: "ekc".into(),
        event: mk_event(AgentEventKind::Error {
            message: "bad".into(),
            error_code: Some(abp_error::ErrorCode::Internal),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { error_code, .. } => {
                assert_eq!(error_code, Some(abp_error::ErrorCode::Internal));
            }
            other => panic!("expected Error, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_ref_id_preserved_across_roundtrip() {
    let env = Envelope::Event {
        ref_id: "correlation-42".into(),
        event: mk_event(AgentEventKind::AssistantDelta { text: "x".into() }),
    };
    match roundtrip(&env) {
        Envelope::Event { ref_id, .. } => assert_eq!(ref_id, "correlation-42"),
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_with_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"original": true}));
    let env = Envelope::Event {
        ref_id: "ext".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some(ext),
        },
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => {
            let ext = event.ext.unwrap();
            assert_eq!(ext["raw_message"]["original"], true);
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_with_ext_none_omitted() {
    let env = Envelope::Event {
        ref_id: "no-ext".into(),
        event: mk_event(AgentEventKind::AssistantDelta { text: "a".into() }),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert!(v["event"].get("raw_message").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Final variant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn final_tag_is_final() {
    let env = Envelope::Final {
        ref_id: "f1".into(),
        receipt: mk_receipt(),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "final");
}

#[test]
fn final_roundtrip_preserves_ref_id() {
    let env = Envelope::Final {
        ref_id: "fin-99".into(),
        receipt: mk_receipt(),
    };
    match roundtrip(&env) {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, "fin-99"),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_outcome_preserved() {
    let mut receipt = mk_receipt();
    receipt.outcome = Outcome::Partial;
    let env = Envelope::Final {
        ref_id: "part".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Partial),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_failed_outcome() {
    let mut receipt = mk_receipt();
    receipt.outcome = Outcome::Failed;
    let env = Envelope::Final {
        ref_id: "fail".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.outcome, Outcome::Failed),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_with_trace() {
    let mut receipt = mk_receipt();
    receipt.trace = vec![
        mk_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        mk_event(AgentEventKind::AssistantMessage {
            text: "done".into(),
        }),
        mk_event(AgentEventKind::RunCompleted {
            message: "fin".into(),
        }),
    ];
    let env = Envelope::Final {
        ref_id: "t".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.trace.len(), 3),
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_with_artifacts() {
    let mut receipt = mk_receipt();
    receipt.artifacts = vec![ArtifactRef {
        kind: "patch".into(),
        path: "changes.diff".into(),
    }];
    let env = Envelope::Final {
        ref_id: "a".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.artifacts.len(), 1);
            assert_eq!(receipt.artifacts[0].kind, "patch");
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_contract_version_preserved() {
    let env = Envelope::Final {
        ref_id: "cv".into(),
        receipt: mk_receipt(),
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[test]
fn final_receipt_usage_normalized_roundtrip() {
    let mut receipt = mk_receipt();
    receipt.usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(5),
        estimated_cost_usd: Some(0.03),
    };
    let env = Envelope::Final {
        ref_id: "u".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.usage.input_tokens, Some(1000));
            assert_eq!(receipt.usage.output_tokens, Some(500));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Fatal variant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_tag_is_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("x".into()),
        error: "boom".into(),
        error_code: None,
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "fatal");
}

#[test]
fn fatal_with_ref_id_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: None,
    };
    match roundtrip(&env) {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert_eq!(ref_id.as_deref(), Some("run-1"));
            assert_eq!(error, "crash");
            assert!(error_code.is_none());
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_without_ref_id_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "early failure".into(),
        error_code: None,
    };
    match roundtrip(&env) {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early failure");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_with_error_code_roundtrip() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "backend crashed",
        abp_error::ErrorCode::BackendCrashed,
    );
    match roundtrip(&env) {
        Envelope::Fatal { error_code, .. } => {
            assert_eq!(error_code, Some(abp_error::ErrorCode::BackendCrashed));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn fatal_error_code_skipped_when_none() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "e".into(),
        error_code: None,
    };
    let v = serde_json::to_value(&env).unwrap();
    assert!(v.get("error_code").is_none());
}

#[test]
fn fatal_error_code_present_when_some() {
    let env = Envelope::fatal_with_code(None, "timeout", abp_error::ErrorCode::BackendTimeout);
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["error_code"], "BACKEND_TIMEOUT");
}

#[test]
fn fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(None, "e", abp_error::ErrorCode::ProtocolInvalidEnvelope);
    assert_eq!(
        env.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn fatal_error_code_accessor_none_on_other_variants() {
    let env = Envelope::hello(mk_backend("a"), BTreeMap::new());
    assert!(env.error_code().is_none());
}

#[test]
fn fatal_from_abp_error() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::PolicyDenied, "denied by policy");
    let env = Envelope::fatal_from_abp_error(Some("run-x".into()), &abp_err);
    match roundtrip(&env) {
        Envelope::Fatal {
            error, error_code, ..
        } => {
            assert_eq!(error, "denied by policy");
            assert_eq!(error_code, Some(abp_error::ErrorCode::PolicyDenied));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Tag field discrimination — the "t" field
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tag_field_is_t_not_type() {
    let env = Envelope::hello(mk_backend("a"), BTreeMap::new());
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
    assert!(!json.contains(r#""type":"hello""#));
}

#[test]
fn each_variant_has_distinct_tag() {
    let envelopes: Vec<Envelope> = vec![
        Envelope::hello(mk_backend("a"), BTreeMap::new()),
        Envelope::Run {
            id: "r".into(),
            work_order: mk_work_order(),
        },
        Envelope::Event {
            ref_id: "r".into(),
            event: mk_event(AgentEventKind::Warning {
                message: "w".into(),
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
    let tags: Vec<String> = envelopes
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap();
            v["t"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(tags, vec!["hello", "run", "event", "final", "fatal"]);
}

#[test]
fn inner_event_uses_type_not_t() {
    let env = Envelope::Event {
        ref_id: "r".into(),
        event: mk_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
    };
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["event"]["type"], "assistant_message");
    assert!(v["event"].get("t").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. ref_id correlation across envelopes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ref_id_matches_run_id_in_event() {
    let run_id = "run-abc";
    let run_env = Envelope::Run {
        id: run_id.into(),
        work_order: mk_work_order(),
    };
    let event_env = Envelope::Event {
        ref_id: run_id.into(),
        event: mk_event(AgentEventKind::AssistantDelta { text: "t".into() }),
    };
    let rv = serde_json::to_value(&run_env).unwrap();
    let ev = serde_json::to_value(&event_env).unwrap();
    assert_eq!(rv["id"].as_str().unwrap(), ev["ref_id"].as_str().unwrap());
}

#[test]
fn ref_id_matches_run_id_in_final() {
    let run_id = "run-abc";
    let run_env = Envelope::Run {
        id: run_id.into(),
        work_order: mk_work_order(),
    };
    let final_env = Envelope::Final {
        ref_id: run_id.into(),
        receipt: mk_receipt(),
    };
    let rv = serde_json::to_value(&run_env).unwrap();
    let fv = serde_json::to_value(&final_env).unwrap();
    assert_eq!(rv["id"].as_str().unwrap(), fv["ref_id"].as_str().unwrap());
}

#[test]
fn ref_id_matches_run_id_in_fatal() {
    let run_id = "run-abc";
    let fatal_env = Envelope::Fatal {
        ref_id: Some(run_id.into()),
        error: "e".into(),
        error_code: None,
    };
    let fv = serde_json::to_value(&fatal_env).unwrap();
    assert_eq!(fv["ref_id"].as_str().unwrap(), run_id);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Malformed envelope handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decode_empty_string_fails() {
    assert!(JsonlCodec::decode("").is_err());
}

#[test]
fn decode_bare_null_fails() {
    assert!(JsonlCodec::decode("null").is_err());
}

#[test]
fn decode_bare_number_fails() {
    assert!(JsonlCodec::decode("42").is_err());
}

#[test]
fn decode_bare_array_fails() {
    assert!(JsonlCodec::decode("[1,2,3]").is_err());
}

#[test]
fn decode_bare_string_fails() {
    assert!(JsonlCodec::decode(r#""hello""#).is_err());
}

#[test]
fn decode_not_json_fails() {
    let err = JsonlCodec::decode("{not valid json}").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn decode_truncated_json_fails() {
    assert!(
        JsonlCodec::decode(
            r#"{"t":"hello","contract_version":#).is_err());
}

#[test]
fn decode_missing_tag_field_fails() {
    assert!(JsonlCodec::decode(r#"{"data":1}"#
        )
        .is_err()
    );
}

#[test]
fn decode_tag_as_number_fails() {
    assert!(JsonlCodec::decode(r#"{"t":42}"#).is_err());
}

#[test]
fn decode_tag_as_null_fails() {
    assert!(JsonlCodec::decode(r#"{"t":null}"#).is_err());
}

#[test]
fn decode_tag_as_bool_fails() {
    assert!(JsonlCodec::decode(r#"{"t":true}"#).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Unknown variant handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn decode_unknown_tag_value_fails() {
    assert!(JsonlCodec::decode(r#"{"t":"nonexistent","data":1}"#).is_err());
}

#[test]
fn decode_tag_subscribe_fails() {
    assert!(JsonlCodec::decode(r#"{"t":"subscribe","channel":"events"}"#).is_err());
}

#[test]
fn decode_tag_empty_string_fails() {
    assert!(JsonlCodec::decode(r#"{"t":"","data":1}"#).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Missing required fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_missing_contract_version_fails() {
    let raw = r#"{"t":"hello","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn hello_missing_backend_fails() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","capabilities":{}}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn hello_missing_capabilities_fails() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null}}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn run_missing_id_fails() {
    let raw = r#"{"t":"run","work_order":{"id":"00000000-0000-0000-0000-000000000000","task":"t","lane":"patch_first","workspace":{"root":"/","mode":"pass_through","include":[],"exclude":[]},"context":{"files":[],"snippets":[]},"policy":{"allowed_tools":[],"disallowed_tools":[],"deny_read":[],"deny_write":[],"allow_network":[],"deny_network":[],"require_approval_for":[]},"requirements":{"required":[]},"config":{"model":null,"vendor":{},"env":{},"max_budget_usd":null,"max_turns":null}}}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn run_missing_work_order_fails() {
    let raw = r#"{"t":"run","id":"r1"}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn event_missing_ref_id_fails() {
    let raw =
        r#"{"t":"event","event":{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"w"}}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn event_missing_event_fails() {
    let raw = r#"{"t":"event","ref_id":"r1"}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn final_missing_ref_id_fails() {
    // Final ref_id is required (not Optional like Fatal)
    let raw = r#"{"t":"final","receipt":{}}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

#[test]
fn fatal_missing_error_fails() {
    let raw = r#"{"t":"fatal","ref_id":null}"#;
    assert!(serde_json::from_str::<Envelope>(raw).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Extra / unknown fields (serde default: ignored)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_extra_fields_ignored() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"boom","extra_field":"ignored","another":42}"#;
    let env: Envelope = serde_json::from_str(raw).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn hello_extra_fields_ignored() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": { "id": "x", "backend_version": null, "adapter_version": null },
        "capabilities": {},
        "unknown_key": "ignored"
    });
    let env: Envelope = serde_json::from_value(raw).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Null field handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_ref_id_explicit_null() {
    let raw = r#"{"t":"fatal","ref_id":null,"error":"e"}"#;
    match serde_json::from_str::<Envelope>(raw).unwrap() {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn hello_backend_version_null() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": CONTRACT_VERSION,
        "backend": { "id": "x", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    match serde_json::from_value::<Envelope>(raw).unwrap() {
        Envelope::Hello { backend, .. } => {
            assert!(backend.backend_version.is_none());
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Large payload envelopes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_assistant_message_roundtrip() {
    let large_text = "x".repeat(100_000);
    let env = Envelope::Event {
        ref_id: "big".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: large_text.clone(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text.len(), 100_000),
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn large_tool_result_roundtrip() {
    let big_output = serde_json::json!({"data": "a".repeat(50_000)});
    let env = Envelope::Event {
        ref_id: "big-tr".into(),
        event: mk_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: big_output.clone(),
            is_error: false,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert_eq!(output["data"].as_str().unwrap().len(), 50_000);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn large_trace_in_receipt_roundtrip() {
    let mut receipt = mk_receipt();
    receipt.trace = (0..200)
        .map(|i| {
            mk_event(AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            })
        })
        .collect();
    let env = Envelope::Final {
        ref_id: "big-trace".into(),
        receipt,
    };
    match roundtrip(&env) {
        Envelope::Final { receipt, .. } => assert_eq!(receipt.trace.len(), 200),
        other => panic!("expected Final, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Deterministic serialization (BTreeMap)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capabilities_serialize_in_sorted_order() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let env = Envelope::hello(mk_backend("det"), caps);
    let json = serde_json::to_string(&env).unwrap();
    let streaming_pos = json.find("\"streaming\"").unwrap();
    let read_pos = json.find("\"tool_read\"").unwrap();
    let write_pos = json.find("\"tool_write\"").unwrap();
    assert!(streaming_pos < read_pos);
    assert!(read_pos < write_pos);
}

#[test]
fn vendor_config_serializes_deterministically() {
    let mut wo = mk_work_order();
    let mut vendor = BTreeMap::new();
    vendor.insert("z_key".into(), serde_json::json!("z"));
    vendor.insert("a_key".into(), serde_json::json!("a"));
    vendor.insert("m_key".into(), serde_json::json!("m"));
    wo.config.vendor = vendor;
    let env = Envelope::Run {
        id: "det".into(),
        work_order: wo,
    };
    let json = serde_json::to_string(&env).unwrap();
    let a_pos = json.find("\"a_key\"").unwrap();
    let m_pos = json.find("\"m_key\"").unwrap();
    let z_pos = json.find("\"z_key\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn env_config_serializes_deterministically() {
    let mut wo = mk_work_order();
    let mut env_vars = BTreeMap::new();
    env_vars.insert("ZEBRA".into(), "z".into());
    env_vars.insert("ALPHA".into(), "a".into());
    wo.config.env = env_vars;
    let env = Envelope::Run {
        id: "det2".into(),
        work_order: wo,
    };
    let json = serde_json::to_string(&env).unwrap();
    let a_pos = json.find("\"ALPHA\"").unwrap();
    let z_pos = json.find("\"ZEBRA\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn double_serialization_is_stable() {
    let env = Envelope::hello(mk_backend("stable"), mk_caps());
    let first = serde_json::to_string(&env).unwrap();
    let second = serde_json::to_string(&env).unwrap();
    assert_eq!(first, second);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. JSONL line-by-line parsing simulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn jsonl_stream_multiple_envelopes() {
    let envelopes = vec![
        Envelope::hello(mk_backend("s"), BTreeMap::new()),
        Envelope::Event {
            ref_id: "r".into(),
            event: mk_event(AgentEventKind::Warning {
                message: "w".into(),
            }),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "done".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Event { .. }));
    assert!(matches!(decoded[2], Envelope::Fatal { .. }));
}

#[test]
fn jsonl_stream_skips_blank_lines() {
    let input = format!(
        "{}\n\n{}\n\n",
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        })
        .unwrap(),
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        })
        .unwrap()
    );
    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

#[test]
fn jsonl_stream_full_session_simulation() {
    let run_id = "session-1";
    let session: Vec<Envelope> = vec![
        Envelope::hello(mk_backend("node"), mk_caps()),
        Envelope::Run {
            id: run_id.into(),
            work_order: mk_work_order(),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: mk_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: mk_event(AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: mk_event(AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"cmd": "echo hi"}),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: mk_event(AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!("hi"),
                is_error: false,
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: mk_event(AgentEventKind::FileChanged {
                path: "out.txt".into(),
                summary: "created".into(),
            }),
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: mk_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: mk_receipt(),
        },
    ];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &session).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 9);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    for env in &decoded[2..8] {
        assert!(matches!(env, Envelope::Event { .. }));
    }
    assert!(matches!(decoded[8], Envelope::Final { .. }));
}

#[test]
fn jsonl_stream_with_whitespace_lines() {
    let input = format!(
        "   \n{}\n  \n  \n{}\n",
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        })
        .unwrap(),
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        })
        .unwrap(),
    );
    let reader = BufReader::new(input.as_bytes());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. JsonlCodec encoding properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn encode_ends_with_newline() {
    let env = Envelope::hello(mk_backend("a"), BTreeMap::new());
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn encode_is_single_line() {
    let env = Envelope::Run {
        id: "r".into(),
        work_order: mk_work_order(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert_eq!(encoded.matches('\n').count(), 1);
    assert!(encoded.ends_with('\n'));
}

#[test]
fn encode_produces_valid_json() {
    let env = Envelope::hello(mk_backend("a"), mk_caps());
    let encoded = JsonlCodec::encode(&env).unwrap();
    let _: Value = serde_json::from_str(encoded.trim_end()).expect("must be valid JSON");
}

#[test]
fn encode_to_writer_and_back() {
    let env = Envelope::Fatal {
        ref_id: Some("w".into()),
        error: "writer".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let line = String::from_utf8(buf).unwrap();
    assert!(line.ends_with('\n'));
    let decoded = JsonlCodec::decode(line.trim_end()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Protocol version in hello
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_protocol_version_is_abp_v01() {
    let env = Envelope::hello(mk_backend("v"), BTreeMap::new());
    match env {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, "abp/v0.1"),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_custom_version_deserializes() {
    let raw = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v99.42",
        "backend": { "id": "x", "backend_version": null, "adapter_version": null },
        "capabilities": {}
    });
    match serde_json::from_value::<Envelope>(raw).unwrap() {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, "abp/v99.42"),
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. ProtocolError variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn protocol_error_json_display() {
    let err = JsonlCodec::decode("{bad}").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid JSON"));
}

#[test]
fn protocol_error_violation_has_error_code() {
    let err = ProtocolError::Violation("test".into());
    assert_eq!(
        err.error_code(),
        Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
    );
}

#[test]
fn protocol_error_unexpected_message_has_error_code() {
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
fn protocol_error_json_has_no_error_code() {
    let err = JsonlCodec::decode("{bad}").unwrap_err();
    assert!(err.error_code().is_none());
}

#[test]
fn protocol_error_from_abp_error() {
    let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::Internal, "internal");
    let err = ProtocolError::from(abp_err);
    assert_eq!(err.error_code(), Some(abp_error::ErrorCode::Internal));
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Wire format stability from fixed JSON strings
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wire_hello_from_fixed_json() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"node","backend_version":"2.0","adapter_version":null},"capabilities":{"streaming":"native"},"mode":"passthrough"}"#;
    match serde_json::from_str::<Envelope>(json).unwrap() {
        Envelope::Hello {
            backend,
            mode,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "node");
            assert_eq!(mode, ExecutionMode::Passthrough);
            assert!(capabilities.contains_key(&Capability::Streaming));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn wire_event_assistant_delta_from_fixed_json() {
    let json = r#"{"t":"event","ref_id":"r1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_delta","text":"hi"}}"#;
    match serde_json::from_str::<Envelope>(json).unwrap() {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "r1");
            match event.kind {
                AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hi"),
                other => panic!("expected AssistantDelta, got {other:?}"),
            }
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn wire_fatal_null_ref_id_from_fixed_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"early"}"#;
    match serde_json::from_str::<Envelope>(json).unwrap() {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "early");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Property: all variants roundtrip cleanly
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_variants_roundtrip_via_jsonl_codec() {
    let variants: Vec<Envelope> = vec![
        Envelope::hello(mk_backend("a"), mk_caps()),
        Envelope::Run {
            id: "r1".into(),
            work_order: mk_work_order(),
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: mk_event(AgentEventKind::AssistantMessage {
                text: "hello".into(),
            }),
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: mk_receipt(),
        },
        Envelope::Fatal {
            ref_id: Some("r1".into()),
            error: "err".into(),
            error_code: None,
        },
    ];
    for env in &variants {
        let encoded = JsonlCodec::encode(env).unwrap();
        assert!(encoded.ends_with('\n'));
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        assert_eq!(encoded, re_encoded, "roundtrip stability check");
    }
}

#[test]
fn all_event_kinds_roundtrip() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: serde_json::json!("ok"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f.rs".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];
    for kind in kinds {
        let env = Envelope::Event {
            ref_id: "r".into(),
            event: mk_event(kind),
        };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        assert!(matches!(decoded, Envelope::Event { .. }));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. Edge cases: special characters, unicode, empty strings
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_error_with_special_characters() {
    let env = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: r#"error with "quotes" and \backslash and newline\n"#.into(),
        error_code: None,
    };
    match roundtrip(&env) {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("quotes"));
            assert!(error.contains("backslash"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn event_with_unicode_text() {
    let env = Envelope::Event {
        ref_id: "u".into(),
        event: mk_event(AgentEventKind::AssistantMessage {
            text: "こんにちは 🌍 Ñoño café".into(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert!(text.contains("こんにちは"));
                assert!(text.contains("🌍"));
                assert!(text.contains("café"));
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_empty_text_roundtrip() {
    let env = Envelope::Event {
        ref_id: "empty".into(),
        event: mk_event(AgentEventKind::AssistantDelta {
            text: String::new(),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert!(text.is_empty()),
            other => panic!("expected AssistantDelta, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn fatal_empty_error_string() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: String::new(),
        error_code: None,
    };
    match roundtrip(&env) {
        Envelope::Fatal { error, .. } => assert!(error.is_empty()),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn run_empty_task_string() {
    let mut wo = mk_work_order();
    wo.task = String::new();
    let env = Envelope::Run {
        id: "et".into(),
        work_order: wo,
    };
    match roundtrip(&env) {
        Envelope::Run { work_order, .. } => assert!(work_order.task.is_empty()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn hello_empty_backend_id() {
    let backend = BackendIdentity {
        id: String::new(),
        backend_version: None,
        adapter_version: None,
    };
    let env = Envelope::hello(backend, BTreeMap::new());
    match roundtrip(&env) {
        Envelope::Hello { backend, .. } => assert!(backend.id.is_empty()),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn event_tool_call_empty_input() {
    let env = Envelope::Event {
        ref_id: "ei".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "noop".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input, serde_json::json!({}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn event_tool_call_null_input() {
    let env = Envelope::Event {
        ref_id: "ni".into(),
        event: mk_event(AgentEventKind::ToolCall {
            tool_name: "noop".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::Value::Null,
        }),
    };
    match roundtrip(&env) {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert!(input.is_null());
            }
            other => panic!("expected ToolCall, got {other:?}"),
        },
        other => panic!("expected Event, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. Encode many + stream decode roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn encode_many_then_decode_stream() {
    let envelopes = vec![
        Envelope::hello(mk_backend("s"), mk_caps()),
        Envelope::Run {
            id: "r1".into(),
            work_order: mk_work_order(),
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: mk_receipt(),
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let text = String::from_utf8(buf.clone()).unwrap();
    // Each line should end with \n
    assert_eq!(text.matches('\n').count(), 3);

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
}

#[test]
fn encode_many_empty_slice() {
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[]).unwrap();
    assert!(buf.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. JSON value-level access patterns
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_type_tag_uses_snake_case() {
    let kinds_and_expected: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted {
                message: "s".into(),
            },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted {
                message: "c".into(),
            },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta { text: "d".into() },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage { text: "m".into() },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: "t".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!(null),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "t".into(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: "p".into(),
                summary: "s".into(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: "c".into(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (
            AgentEventKind::Warning {
                message: "w".into(),
            },
            "warning",
        ),
        (
            AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
            "error",
        ),
    ];

    for (kind, expected_tag) in kinds_and_expected {
        let env = Envelope::Event {
            ref_id: "r".into(),
            event: mk_event(kind),
        };
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(
            v["event"]["type"].as_str().unwrap(),
            expected_tag,
            "wrong tag for {expected_tag}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. Version parsing integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_version_from_hello_contract_version() {
    let env = Envelope::hello(mk_backend("v"), BTreeMap::new());
    match env {
        Envelope::Hello {
            contract_version, ..
        } => {
            let parsed = abp_protocol::parse_version(&contract_version);
            assert_eq!(parsed, Some((0, 1)));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn is_compatible_version_with_hello() {
    let env = Envelope::hello(mk_backend("v"), BTreeMap::new());
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
