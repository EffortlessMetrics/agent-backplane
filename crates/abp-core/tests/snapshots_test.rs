use abp_core::*;
use chrono::{TimeZone, Utc};
use insta::assert_json_snapshot;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "Refactor auth module".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use JWT".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["write".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(1.0),
            max_turns: Some(10),
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
        capabilities: BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
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
        artifacts: vec![ArtifactRef {
            kind: "diff".into(),
            path: "out.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("+line".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

#[test]
fn snapshot_work_order() {
    assert_json_snapshot!("work_order", sample_work_order());
}

#[test]
fn snapshot_receipt() {
    let value = serde_json::to_value(&sample_receipt()).unwrap();
    assert_json_snapshot!("receipt", value);
}

#[test]
fn snapshot_agent_event_run_started() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_run_started", event);
}

#[test]
fn snapshot_agent_event_run_completed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_run_completed", event);
}

#[test]
fn snapshot_agent_event_assistant_delta() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_assistant_delta", event);
}

#[test]
fn snapshot_agent_event_assistant_message() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_assistant_message", event);
}

#[test]
fn snapshot_agent_event_tool_call() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "file.rs"}),
        },
        ext: None,
    };
    assert_json_snapshot!("event_tool_call", event);
}

#[test]
fn snapshot_agent_event_tool_result() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!("file contents"),
            is_error: false,
        },
        ext: None,
    };
    assert_json_snapshot!("event_tool_result", event);
}

#[test]
fn snapshot_agent_event_file_changed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_file_changed", event);
}

#[test]
fn snapshot_agent_event_command_executed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        ext: None,
    };
    assert_json_snapshot!("event_command_executed", event);
}

#[test]
fn snapshot_agent_event_warning() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "watch out".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_warning", event);
}

#[test]
fn snapshot_agent_event_error() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("event_error", event);
}

#[test]
fn snapshot_policy_profile() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "glob".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec![".env".into(), "**/*.key".into()],
        deny_write: vec!["Cargo.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["write".into(), "edit".into()],
    };
    assert_json_snapshot!("policy_profile", policy);
}

#[test]
fn snapshot_capability_manifest() {
    let manifest: CapabilityManifest = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Unsupported),
        (
            Capability::McpClient,
            SupportLevel::Restricted {
                reason: "policy".into(),
            },
        ),
    ]);
    let value = serde_json::to_value(&manifest).unwrap();
    assert_json_snapshot!("capability_manifest", value);
}
