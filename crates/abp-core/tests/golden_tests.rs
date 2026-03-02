// SPDX-License-Identifier: MIT OR Apache-2.0
//! Golden-file snapshot tests for JSON schemas and key serialization formats.

use abp_core::*;
use chrono::{TimeZone, Utc};
use insta::assert_json_snapshot;
use schemars::schema_for;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}

// ---------------------------------------------------------------------------
// JSON Schema snapshots (schemars)
// ---------------------------------------------------------------------------

#[test]
fn golden_schema_work_order() {
    let schema = schema_for!(WorkOrder);
    let value = serde_json::to_value(&schema).unwrap();
    assert_json_snapshot!("golden_schema_work_order", value);
}

#[test]
fn golden_schema_receipt() {
    let schema = schema_for!(Receipt);
    let value = serde_json::to_value(&schema).unwrap();
    assert_json_snapshot!("golden_schema_receipt", value);
}

// ---------------------------------------------------------------------------
// Fully-populated value snapshots
// ---------------------------------------------------------------------------

fn full_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "Refactor auth module".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into(), "src/lib.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use JWT for auth".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into(), "glob".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["write".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::from([("abp".into(), json!({"mode": "mapped"}))]),
            env: BTreeMap::from([("RUST_LOG".into(), "debug".into())]),
            max_budget_usd: Some(5.0),
            max_turns: Some(20),
        },
    }
}

fn full_receipt() -> Receipt {
    let ts = fixed_ts();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::from_u128(1),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 1500,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.2.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Emulated),
            (Capability::ToolBash, SupportLevel::Unsupported),
            (
                Capability::McpClient,
                SupportLevel::Restricted {
                    reason: "disabled by policy".into(),
                },
            ),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"prompt_tokens": 200, "completion_tokens": 150}),
        usage: UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(150),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(10),
            request_units: Some(1),
            estimated_cost_usd: Some(0.005),
        },
        trace: vec![
            AgentEvent {
                ts,
                kind: AgentEventKind::RunStarted {
                    message: "starting run".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts,
                kind: AgentEventKind::ToolCall {
                    tool_name: "read".into(),
                    tool_use_id: Some("tu_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/lib.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts,
                kind: AgentEventKind::ToolResult {
                    tool_name: "read".into(),
                    tool_use_id: Some("tu_1".into()),
                    output: json!("fn main() {}"),
                    is_error: false,
                },
                ext: None,
            },
            AgentEvent {
                ts,
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "out.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("+added line\n-removed line".into()),
            git_status: Some("M src/lib.rs\nA src/new.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

#[test]
fn golden_full_work_order() {
    assert_json_snapshot!("golden_full_work_order", full_work_order());
}

#[test]
fn golden_full_receipt() {
    let value = serde_json::to_value(full_receipt()).unwrap();
    assert_json_snapshot!("golden_full_receipt", value);
}

// ---------------------------------------------------------------------------
// All AgentEventKind variants
// ---------------------------------------------------------------------------

#[test]
fn golden_event_run_started() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "initializing".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_run_started", event);
}

#[test]
fn golden_event_run_completed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunCompleted {
            message: "finished".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_run_completed", event);
}

#[test]
fn golden_event_assistant_delta() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello, ".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_assistant_delta", event);
}

#[test]
fn golden_event_assistant_message() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello, world!".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_assistant_message", event);
}

#[test]
fn golden_event_tool_call() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu_42".into()),
            parent_tool_use_id: Some("tu_parent".into()),
            input: json!({"path": "src/main.rs", "line": 10}),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_tool_call", event);
}

#[test]
fn golden_event_tool_result() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("tu_42".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_tool_result", event);
}

#[test]
fn golden_event_file_changed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/auth.rs".into(),
            summary: "added JWT validation".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_file_changed", event);
}

#[test]
fn golden_event_command_executed() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok. 42 passed".into()),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_command_executed", event);
}

#[test]
fn golden_event_warning() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "approaching budget limit".into(),
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_warning", event);
}

#[test]
fn golden_event_error() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Error {
            message: "backend returned 500".into(),
            error_code: None,
        },
        ext: None,
    };
    assert_json_snapshot!("golden_event_error", event);
}

#[test]
fn golden_event_with_ext() {
    let event = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "passthrough".into(),
        },
        ext: Some(BTreeMap::from([(
            "raw_message".into(),
            json!({"role": "assistant", "content": "passthrough"}),
        )])),
    };
    assert_json_snapshot!("golden_event_with_ext", event);
}
