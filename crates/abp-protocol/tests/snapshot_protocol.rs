// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive snapshot tests for abp-protocol serialization formats.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use insta::{assert_json_snapshot, assert_snapshot};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap()
}

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "Fix auth bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            ..Default::default()
        },
    }
}

fn sample_receipt() -> Receipt {
    let ts = fixed_ts();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::from_u128(1),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: BTreeMap::from([(Capability::Streaming, SupportLevel::Native)]),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        },
        trace: vec![AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        }],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ── 1. Hello envelope (basic) ───────────────────────────────────────────

#[test]
fn snapshot_envelope_hello_basic() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "node-sidecar".into(),
            backend_version: Some("1.2.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        BTreeMap::new(),
    );
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_hello_basic", value);
}

// ── 2. Hello with full capabilities ─────────────────────────────────────

#[test]
fn snapshot_envelope_hello_full_capabilities() {
    let caps: CapabilityManifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolEdit, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Native),
        (Capability::ToolGlob, SupportLevel::Native),
        (Capability::ToolGrep, SupportLevel::Native),
        (Capability::ToolWebSearch, SupportLevel::Emulated),
        (Capability::ToolWebFetch, SupportLevel::Emulated),
        (Capability::HooksPreToolUse, SupportLevel::Native),
        (Capability::HooksPostToolUse, SupportLevel::Native),
        (Capability::Checkpointing, SupportLevel::Native),
        (
            Capability::McpClient,
            SupportLevel::Restricted {
                reason: "beta".into(),
            },
        ),
    ]);
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "claude-sidecar".into(),
            backend_version: Some("2024.6.1".into()),
            adapter_version: Some("0.3.0".into()),
        },
        caps,
        ExecutionMode::Passthrough,
    );
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_hello_full_caps", value);
}

// ── 3. Run envelope ─────────────────────────────────────────────────────

#[test]
fn snapshot_envelope_run() {
    let env = Envelope::Run {
        id: "run-42".into(),
        work_order: sample_work_order(),
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_run", value);
}

// ── 4. Event envelope — AssistantDelta ──────────────────────────────────

#[test]
fn snapshot_envelope_event_assistant_delta() {
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "Let me look at ".into(),
            },
            ext: None,
        },
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_event_assistant_delta", value);
}

// ── 5. Event envelope — ToolCall ────────────────────────────────────────

#[test]
fn snapshot_envelope_event_tool_call() {
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("toolu_01".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_event_tool_call", value);
}

// ── 6. Event envelope — CommandExecuted ─────────────────────────────────

#[test]
fn snapshot_envelope_event_command_executed() {
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: Some("Compiling...".into()),
            },
            ext: None,
        },
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_event_command_executed", value);
}

// ── 7. Event envelope — Error ───────────────────────────────────────────

#[test]
fn snapshot_envelope_event_error() {
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".into(),
                error_code: None,
            },
            ext: None,
        },
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_event_error", value);
}

// ── 8. Fatal envelope with ref ──────────────────────────────────────────

#[test]
fn snapshot_envelope_fatal_with_ref() {
    let env = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "sidecar process crashed: exit code 137 (OOM killed)".into(),
        error_code: None,
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_fatal_with_ref", value);
}

// ── 9. Fatal envelope without ref ───────────────────────────────────────

#[test]
fn snapshot_envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "handshake timeout: no hello received within 5s".into(),
        error_code: None,
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_fatal_no_ref", value);
}

// ── 10. Final envelope with receipt ─────────────────────────────────────

#[test]
fn snapshot_envelope_final_with_receipt() {
    let env = Envelope::Final {
        ref_id: "run-42".into(),
        receipt: sample_receipt(),
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_final_with_receipt", value);
}

// ── 11. JSONL batch (3 envelopes) ───────────────────────────────────────

#[test]
fn snapshot_jsonl_batch_three_envelopes() {
    let envelopes = vec![
        Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            BTreeMap::new(),
        ),
        Envelope::Event {
            ref_id: "run-1".into(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
        },
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "boom".into(),
            error_code: None,
        },
    ];

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let jsonl_output = String::from_utf8(buf).unwrap();
    assert_snapshot!("protocol_jsonl_batch", jsonl_output);
}

// ── 12. Hello with mapped mode (default) ────────────────────────────────

#[test]
fn snapshot_envelope_hello_mapped_mode() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "python-sidecar".into(),
            backend_version: None,
            adapter_version: Some("0.1.0".into()),
        },
        BTreeMap::from([(Capability::Streaming, SupportLevel::Emulated)]),
        ExecutionMode::Mapped,
    );
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_hello_mapped_mode", value);
}

// ── 13. Event envelope — ToolResult with error ──────────────────────────

#[test]
fn snapshot_envelope_event_tool_result_error() {
    let env = Envelope::Event {
        ref_id: "run-42".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("toolu_02".into()),
                output: json!("command not found: foobar"),
                is_error: true,
            },
            ext: None,
        },
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("protocol_event_tool_result_error", value);
}
