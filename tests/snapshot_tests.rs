// SPDX-License-Identifier: MIT OR Apache-2.0
//! Golden-file / snapshot tests for JSON serialization of core contract types.
//!
//! Every test constructs a value with deterministic data (fixed UUIDs, timestamps)
//! and asserts the `serde_json` output via `insta::assert_json_snapshot!`.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_capability::{CompatibilityReport, NegotiationResult, SupportLevel as CapSupportLevel};
use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_protocol::Envelope;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 10, 0, 0).unwrap()
}

fn ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 10, 5, 0).unwrap()
}

fn uid1() -> Uuid {
    Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").unwrap()
}

fn uid2() -> Uuid {
    Uuid::parse_str("11111111-2222-4333-8444-555555555555").unwrap()
}

fn backend() -> BackendIdentity {
    BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("2.1.0".into()),
        adapter_version: Some("0.5.0".into()),
    }
}

fn small_caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolUse, SupportLevel::Emulated);
    m
}

fn full_caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Native);
    m.insert(Capability::ToolEdit, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m.insert(Capability::ToolUse, SupportLevel::Emulated);
    m.insert(Capability::ExtendedThinking, SupportLevel::Unsupported);
    m.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "experimental".into(),
        },
    );
    m
}

fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: "abp/v0.1".into(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: backend(),
        capabilities: small_caps(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn full_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: uid1(),
            work_order_id: uid2(),
            contract_version: "abp/v0.1".into(),
            started_at: ts(),
            finished_at: ts2(),
            duration_ms: 300_000,
        },
        backend: backend(),
        capabilities: full_caps(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"prompt_tokens": 500, "completion_tokens": 200}),
        usage: UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(200),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(10),
            request_units: Some(1),
            estimated_cost_usd: Some(0.005),
        },
        trace: vec![
            AgentEvent {
                ts: ts(),
                kind: AgentEventKind::RunStarted {
                    message: "starting run".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "fix.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("diff --git a/f.txt b/f.txt".into()),
            git_status: Some("M f.txt".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: uid1(),
        task: "Fix login bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["node_modules/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "check auth module".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["delete".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Emulated,
                },
            ],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: {
                let mut v = BTreeMap::new();
                v.insert("temperature".into(), json!(0.7));
                v
            },
            env: {
                let mut e = BTreeMap::new();
                e.insert("LANG".into(), "en".into());
                e
            },
            max_budget_usd: Some(1.0),
            max_turns: Some(25),
        },
    }
}

// =========================================================================
// WorkOrder snapshots
// =========================================================================

#[test]
fn snapshot_work_order_minimal() {
    let wo = WorkOrderBuilder::new("hello world").build();
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]"
    });
}

#[test]
fn snapshot_work_order_full() {
    insta::assert_json_snapshot!(sample_work_order());
}

#[test]
fn snapshot_work_order_with_tools_policy() {
    let wo = WorkOrder {
        id: uid1(),
        task: "Refactor".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile {
            allowed_tools: vec!["grep".into(), "glob".into(), "read".into()],
            disallowed_tools: vec!["bash".into(), "write".into()],
            deny_read: vec!["secrets/**".into()],
            deny_write: vec!["**/*.lock".into()],
            allow_network: vec![],
            deny_network: vec!["*".into()],
            require_approval_for: vec!["edit".into()],
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn snapshot_work_order_with_capability_requirements() {
    let wo = WorkOrder {
        id: uid1(),
        task: "needs capabilities".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolBash,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Emulated,
                },
                CapabilityRequirement {
                    capability: Capability::ImageInput,
                    min_support: MinSupport::Native,
                },
            ],
        },
        config: RuntimeConfig::default(),
    };
    insta::assert_json_snapshot!(wo);
}

#[test]
fn snapshot_work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("default task").build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["task"], "default task");
    assert_eq!(v["lane"], "patch_first");
    assert_eq!(v["workspace"]["mode"], "staged");
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]"
    });
}

#[test]
fn snapshot_work_order_builder_custom() {
    let wo = WorkOrderBuilder::new("custom task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/home/user/project")
        .workspace_mode(WorkspaceMode::PassThrough)
        .model("claude-3.5-sonnet")
        .max_turns(50)
        .max_budget_usd(10.0)
        .build();
    insta::assert_json_snapshot!(wo, {
        ".id" => "[uuid]"
    });
}

// =========================================================================
// Receipt snapshots
// =========================================================================

#[test]
fn snapshot_receipt_minimal() {
    insta::assert_snapshot!(snap_json(&minimal_receipt()));
}

#[test]
fn snapshot_receipt_full() {
    insta::assert_snapshot!(snap_json(&full_receipt()));
}

#[test]
fn snapshot_receipt_with_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    // Hash is deterministic for fixed data, include it in the snapshot.
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_without_hash() {
    let r = minimal_receipt();
    assert!(r.receipt_sha256.is_none());
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_outcome_complete() {
    let mut r = minimal_receipt();
    r.outcome = Outcome::Complete;
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_outcome_partial() {
    let mut r = minimal_receipt();
    r.outcome = Outcome::Partial;
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_outcome_failed() {
    let mut r = minimal_receipt();
    r.outcome = Outcome::Failed;
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_passthrough_mode() {
    let mut r = minimal_receipt();
    r.mode = ExecutionMode::Passthrough;
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_builder_minimal() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts())
        .finished_at(ts2())
        .work_order_id(uid2())
        .build();
    // Builder generates a random run_id; pin it for deterministic snapshots.
    r.meta.run_id = uid1();
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_builder_with_trace() {
    let mut r = ReceiptBuilder::new("sidecar:node")
        .outcome(Outcome::Complete)
        .started_at(ts())
        .finished_at(ts2())
        .work_order_id(uid2())
        .add_trace_event(AgentEvent {
            ts: ts(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts2(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "file".into(),
            path: "out.txt".into(),
        })
        .build();
    r.meta.run_id = uid1();
    insta::assert_snapshot!(snap_json(&r));
}

#[test]
fn snapshot_receipt_full_usage() {
    let mut r = minimal_receipt();
    r.usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.025),
    };
    r.usage_raw = json!({
        "prompt_tokens": 1000,
        "completion_tokens": 500,
        "cached_tokens": 200,
    });
    insta::assert_snapshot!(snap_json(&r));
}

// =========================================================================
// AgentEvent snapshots — all AgentEventKind variants
// =========================================================================

#[test]
fn snapshot_event_run_started() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "beginning run".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_run_completed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunCompleted {
            message: "run finished".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_assistant_delta() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_assistant_message() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "Here is my full response.".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_tool_call() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_tool_call_nested() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu_002".into()),
            parent_tool_use_id: Some("tu_001".into()),
            input: json!({"path": "out.txt", "content": "data"}),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_tool_result_success() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_001".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_tool_result_error() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_003".into()),
            output: json!({"error": "file not found"}),
            is_error: true,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_file_changed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added helper function".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_command_executed() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok. 42 passed".into()),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_command_executed_no_exit_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::CommandExecuted {
            command: "long-running-job".into(),
            exit_code: None,
            output_preview: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_warning() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Warning {
            message: "budget nearly exhausted".into(),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_error_without_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: None,
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_error_with_code() {
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::Error {
            message: "backend crashed".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        },
        ext: None,
    };
    insta::assert_json_snapshot!(e);
}

#[test]
fn snapshot_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "data"}));
    let e = AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: "passthrough".into(),
        },
        ext: Some(ext),
    };
    insta::assert_json_snapshot!(e);
}

// =========================================================================
// Envelope snapshots — all variants
// =========================================================================

#[test]
fn snapshot_envelope_hello() {
    let env = Envelope::hello(backend(), small_caps());
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn snapshot_envelope_hello_passthrough() {
    let env = Envelope::hello_with_mode(backend(), small_caps(), ExecutionMode::Passthrough);
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn snapshot_envelope_hello_empty_caps() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
    );
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn snapshot_envelope_run() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: sample_work_order(),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "token".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: minimal_receipt(),
    };
    insta::assert_snapshot!(snap_json(&env));
}

#[test]
fn snapshot_envelope_fatal_simple() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "sidecar process exited unexpectedly".into(),
        error_code: None,
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_envelope_fatal_with_code() {
    let env = Envelope::fatal_with_code(
        Some("run-002".into()),
        "backend timed out",
        abp_error::ErrorCode::BackendTimeout,
    );
    insta::assert_json_snapshot!(env);
}

#[test]
fn snapshot_envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "handshake failed".into(),
        error_code: Some(abp_error::ErrorCode::ProtocolVersionMismatch),
    };
    insta::assert_json_snapshot!(env);
}

// =========================================================================
// IR type snapshots
// =========================================================================

#[test]
fn snapshot_ir_message_text() {
    let msg = IrMessage::text(IrRole::User, "Hello, assistant!");
    insta::assert_json_snapshot!(msg);
}

#[test]
fn snapshot_ir_message_system() {
    let msg = IrMessage::text(IrRole::System, "You are a helpful coding assistant.");
    insta::assert_json_snapshot!(msg);
}

#[test]
fn snapshot_ir_message_assistant() {
    let msg = IrMessage::text(IrRole::Assistant, "I'll help you fix that bug.");
    insta::assert_json_snapshot!(msg);
}

#[test]
fn snapshot_ir_message_tool_role() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_100".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents here".into(),
            }],
            is_error: false,
        }],
    );
    insta::assert_json_snapshot!(msg);
}

#[test]
fn snapshot_ir_message_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("vendor_id".into(), json!("msg-abc123"));
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text {
            text: "with metadata".into(),
        }],
        metadata: meta,
    };
    insta::assert_json_snapshot!(msg);
}

#[test]
fn snapshot_ir_content_block_text() {
    let block = IrContentBlock::Text {
        text: "plain text".into(),
    };
    insta::assert_json_snapshot!(block);
}

#[test]
fn snapshot_ir_content_block_image() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    insta::assert_json_snapshot!(block);
}

#[test]
fn snapshot_ir_content_block_tool_use() {
    let block = IrContentBlock::ToolUse {
        id: "tu_200".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    };
    insta::assert_json_snapshot!(block);
}

#[test]
fn snapshot_ir_content_block_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_200".into(),
        content: vec![IrContentBlock::Text {
            text: "fn main() {}".into(),
        }],
        is_error: false,
    };
    insta::assert_json_snapshot!(block);
}

#[test]
fn snapshot_ir_content_block_tool_result_error() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_201".into(),
        content: vec![IrContentBlock::Text {
            text: "permission denied".into(),
        }],
        is_error: true,
    };
    insta::assert_json_snapshot!(block);
}

#[test]
fn snapshot_ir_content_block_thinking() {
    let block = IrContentBlock::Thinking {
        text: "Let me reason step by step...".into(),
    };
    insta::assert_json_snapshot!(block);
}

#[test]
fn snapshot_ir_tool_definition() {
    let td = IrToolDefinition {
        name: "read_file".into(),
        description: "Read the contents of a file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"}
            },
            "required": ["path"]
        }),
    };
    insta::assert_json_snapshot!(td);
}

#[test]
fn snapshot_ir_tool_definition_no_params() {
    let td = IrToolDefinition {
        name: "list_files".into(),
        description: "List files in the workspace".into(),
        parameters: json!({"type": "object", "properties": {}}),
    };
    insta::assert_json_snapshot!(td);
}

#[test]
fn snapshot_ir_conversation_empty() {
    let conv = IrConversation::new();
    insta::assert_json_snapshot!(conv);
}

#[test]
fn snapshot_ir_conversation_simple() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hi"))
        .push(IrMessage::text(IrRole::Assistant, "Hello!"));
    insta::assert_json_snapshot!(conv);
}

#[test]
fn snapshot_ir_conversation_with_tool_use() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Read my file"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_300".into(),
                name: "read_file".into(),
                input: json!({"path": "test.rs"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_300".into(),
                content: vec![IrContentBlock::Text {
                    text: "file content".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::Assistant, "Here is the content."));
    insta::assert_json_snapshot!(conv);
}

#[test]
fn snapshot_ir_message_multi_block() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Analyzing the code...".into(),
            },
            IrContentBlock::Text {
                text: "I found the issue.".into(),
            },
            IrContentBlock::ToolUse {
                id: "tu_400".into(),
                name: "edit_file".into(),
                input: json!({"path": "lib.rs", "content": "fixed"}),
            },
        ],
    );
    insta::assert_json_snapshot!(msg);
}

// =========================================================================
// CapabilityManifest snapshots
// =========================================================================

/// Helper: CapabilityManifest has non-string enum keys so insta's
/// `assert_json_snapshot!` cannot handle it directly. We serialise via
/// `serde_json` (which maps the enum keys to strings) and snapshot the
/// resulting pretty-printed JSON string.
fn snap_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap()
}

#[test]
fn snapshot_capability_manifest_empty() {
    let m: CapabilityManifest = BTreeMap::new();
    insta::assert_snapshot!(snap_json(&m));
}

#[test]
fn snapshot_capability_manifest_small() {
    insta::assert_snapshot!(snap_json(&small_caps()));
}

#[test]
fn snapshot_capability_manifest_full() {
    insta::assert_snapshot!(snap_json(&full_caps()));
}

#[test]
fn snapshot_support_level_native() {
    insta::assert_json_snapshot!(SupportLevel::Native);
}

#[test]
fn snapshot_support_level_emulated() {
    insta::assert_json_snapshot!(SupportLevel::Emulated);
}

#[test]
fn snapshot_support_level_unsupported() {
    insta::assert_json_snapshot!(SupportLevel::Unsupported);
}

#[test]
fn snapshot_support_level_restricted() {
    insta::assert_json_snapshot!(SupportLevel::Restricted {
        reason: "requires opt-in".into()
    });
}

// =========================================================================
// PolicyProfile snapshots
// =========================================================================

#[test]
fn snapshot_policy_profile_empty() {
    insta::assert_json_snapshot!(PolicyProfile::default());
}

#[test]
fn snapshot_policy_profile_full() {
    let p = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into(), "edit".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec![".env".into(), "secrets/**".into()],
        deny_write: vec!["Cargo.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.malware.com".into()],
        require_approval_for: vec!["delete".into(), "exec".into()],
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn snapshot_policy_profile_read_deny_only() {
    let p = PolicyProfile {
        deny_read: vec!["/etc/passwd".into(), "/etc/shadow".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

#[test]
fn snapshot_policy_profile_network_only() {
    let p = PolicyProfile {
        allow_network: vec!["*.github.com".into()],
        deny_network: vec!["*".into()],
        ..PolicyProfile::default()
    };
    insta::assert_json_snapshot!(p);
}

// =========================================================================
// RuntimeConfig snapshots
// =========================================================================

#[test]
fn snapshot_runtime_config_default() {
    insta::assert_json_snapshot!(RuntimeConfig::default());
}

#[test]
fn snapshot_runtime_config_full() {
    let cfg = RuntimeConfig {
        model: Some("claude-3.5-sonnet".into()),
        vendor: {
            let mut v = BTreeMap::new();
            v.insert("temperature".into(), json!(0.5));
            v.insert("max_tokens".into(), json!(4096));
            v
        },
        env: {
            let mut e = BTreeMap::new();
            e.insert("API_KEY".into(), "sk-redacted".into());
            e
        },
        max_budget_usd: Some(5.0),
        max_turns: Some(100),
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn snapshot_runtime_config_model_only() {
    let cfg = RuntimeConfig {
        model: Some("gpt-4o".into()),
        ..RuntimeConfig::default()
    };
    insta::assert_json_snapshot!(cfg);
}

// =========================================================================
// BackplaneConfig snapshots (abp-config)
// =========================================================================

#[test]
fn snapshot_backplane_config_default() {
    insta::assert_json_snapshot!(BackplaneConfig::default());
}

#[test]
fn snapshot_backplane_config_full() {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/node/index.js".into()],
            timeout_secs: Some(120),
        },
    );
    let cfg = BackplaneConfig {
        default_backend: Some("node".into()),
        workspace_dir: Some("/tmp/abp".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends,
    };
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn snapshot_backend_entry_mock() {
    insta::assert_json_snapshot!(BackendEntry::Mock {});
}

#[test]
fn snapshot_backend_entry_sidecar() {
    insta::assert_json_snapshot!(BackendEntry::Sidecar {
        command: "python3".into(),
        args: vec!["hosts/python/main.py".into(), "--verbose".into()],
        timeout_secs: Some(60),
    });
}

#[test]
fn snapshot_backend_entry_sidecar_no_timeout() {
    insta::assert_json_snapshot!(BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    });
}

// =========================================================================
// Capability negotiation snapshots (abp-capability)
// =========================================================================

#[test]
fn snapshot_negotiation_result_all_native() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![],
        unsupported: vec![],
    };
    insta::assert_json_snapshot!(r);
}

#[test]
fn snapshot_negotiation_result_mixed() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![Capability::ExtendedThinking],
        unsupported: vec![Capability::McpServer],
    };
    insta::assert_json_snapshot!(r);
}

#[test]
fn snapshot_compatibility_report() {
    let r = CompatibilityReport {
        compatible: true,
        native_count: 3,
        emulated_count: 1,
        unsupported_count: 0,
        summary: "fully compatible (3 native, 1 emulated)".into(),
        details: vec![
            ("streaming".into(), CapSupportLevel::Native),
            ("tool_use".into(), CapSupportLevel::Native),
            (
                "extended_thinking".into(),
                CapSupportLevel::Emulated {
                    strategy: "adapter".into(),
                },
            ),
        ],
    };
    insta::assert_json_snapshot!(r);
}

#[test]
fn snapshot_compatibility_report_incompatible() {
    let r = CompatibilityReport {
        compatible: false,
        native_count: 1,
        emulated_count: 0,
        unsupported_count: 2,
        summary: "incompatible (2 unsupported)".into(),
        details: vec![
            ("streaming".into(), CapSupportLevel::Native),
            ("mcp_client".into(), CapSupportLevel::Unsupported),
            ("mcp_server".into(), CapSupportLevel::Unsupported),
        ],
    };
    insta::assert_json_snapshot!(r);
}

// =========================================================================
// Execution mode & lane snapshots
// =========================================================================

#[test]
fn snapshot_execution_mode_mapped() {
    insta::assert_json_snapshot!(ExecutionMode::Mapped);
}

#[test]
fn snapshot_execution_mode_passthrough() {
    insta::assert_json_snapshot!(ExecutionMode::Passthrough);
}

#[test]
fn snapshot_execution_lane_patch_first() {
    insta::assert_json_snapshot!(ExecutionLane::PatchFirst);
}

#[test]
fn snapshot_execution_lane_workspace_first() {
    insta::assert_json_snapshot!(ExecutionLane::WorkspaceFirst);
}

// =========================================================================
// Miscellaneous sub-type snapshots
// =========================================================================

#[test]
fn snapshot_backend_identity_full() {
    insta::assert_json_snapshot!(backend());
}

#[test]
fn snapshot_backend_identity_minimal() {
    insta::assert_json_snapshot!(BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    });
}

#[test]
fn snapshot_run_metadata() {
    let m = RunMetadata {
        run_id: uid1(),
        work_order_id: uid2(),
        contract_version: "abp/v0.1".into(),
        started_at: ts(),
        finished_at: ts2(),
        duration_ms: 300_000,
    };
    insta::assert_json_snapshot!(m);
}

#[test]
fn snapshot_usage_normalized_default() {
    insta::assert_json_snapshot!(UsageNormalized::default());
}

#[test]
fn snapshot_usage_normalized_full() {
    let u = UsageNormalized {
        input_tokens: Some(2000),
        output_tokens: Some(800),
        cache_read_tokens: Some(400),
        cache_write_tokens: Some(150),
        request_units: Some(5),
        estimated_cost_usd: Some(0.05),
    };
    insta::assert_json_snapshot!(u);
}

#[test]
fn snapshot_verification_report_default() {
    insta::assert_json_snapshot!(VerificationReport::default());
}

#[test]
fn snapshot_verification_report_full() {
    let v = VerificationReport {
        git_diff: Some("diff --git a/x.rs b/x.rs\n+added".into()),
        git_status: Some("M x.rs\nA y.rs".into()),
        harness_ok: true,
    };
    insta::assert_json_snapshot!(v);
}

#[test]
fn snapshot_artifact_ref() {
    insta::assert_json_snapshot!(ArtifactRef {
        kind: "patch".into(),
        path: "changes.patch".into(),
    });
}

#[test]
fn snapshot_context_packet_empty() {
    insta::assert_json_snapshot!(ContextPacket::default());
}

#[test]
fn snapshot_context_packet_full() {
    let cp = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![
            ContextSnippet {
                name: "error_log".into(),
                content: "thread 'main' panicked".into(),
            },
            ContextSnippet {
                name: "spec".into(),
                content: "must handle auth".into(),
            },
        ],
    };
    insta::assert_json_snapshot!(cp);
}

#[test]
fn snapshot_workspace_spec_staged() {
    let ws = WorkspaceSpec {
        root: "/projects/app".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into(), "tests/**".into()],
        exclude: vec!["target/**".into()],
    };
    insta::assert_json_snapshot!(ws);
}

#[test]
fn snapshot_workspace_spec_passthrough() {
    let ws = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    insta::assert_json_snapshot!(ws);
}

#[test]
fn snapshot_capability_requirement_native() {
    let cr = CapabilityRequirement {
        capability: Capability::ToolBash,
        min_support: MinSupport::Native,
    };
    insta::assert_json_snapshot!(cr);
}

#[test]
fn snapshot_capability_requirement_emulated() {
    let cr = CapabilityRequirement {
        capability: Capability::ExtendedThinking,
        min_support: MinSupport::Emulated,
    };
    insta::assert_json_snapshot!(cr);
}

#[test]
fn snapshot_outcome_all_variants() {
    insta::assert_json_snapshot!("outcome_complete", Outcome::Complete);
    insta::assert_json_snapshot!("outcome_partial", Outcome::Partial);
    insta::assert_json_snapshot!("outcome_failed", Outcome::Failed);
}
