// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot tests for protocol envelopes and batch types.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport, WorkOrderBuilder,
};
use abp_protocol::Envelope;
use abp_protocol::batch::{BatchItemStatus, BatchRequest, BatchResponse, BatchResult};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn sample_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps
}

fn sample_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "sidecar:test".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 100, "output_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.001),
        },
        trace: vec![AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "file".into(),
            path: "output.txt".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file.txt".into()),
            git_status: Some("M file.txt".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. Hello envelope snapshots
// ===========================================================================

#[test]
fn envelope_hello_default_mode() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: Some("3.5.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        sample_capabilities(),
    );
    let json_str = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn envelope_hello_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: None,
            adapter_version: None,
        },
        BTreeMap::new(),
        ExecutionMode::Passthrough,
    );
    let json_str = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn envelope_hello_minimal_capabilities() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let env = Envelope::hello(
        BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps,
    );
    let json_str = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn envelope_hello_full_capabilities() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    caps.insert(Capability::CodeExecution, SupportLevel::Unsupported);
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:gemini".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.2.0".into()),
        },
        caps,
    );
    let json_str = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!(json_str);
}

// ===========================================================================
// 2. Run envelope snapshots
// ===========================================================================

#[test]
fn envelope_run_basic() {
    let wo = WorkOrderBuilder::new("Write a unit test").build();
    let env = Envelope::Run {
        id: fixed_uuid().to_string(),
        work_order: wo,
    };
    insta::assert_json_snapshot!(env, {
        ".work_order.id" => "[uuid]"
    });
}

// ===========================================================================
// 3. Event envelope snapshots
// ===========================================================================

#[test]
fn envelope_event_text_delta() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello, ".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_tool_call() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write_file".into(),
                tool_use_id: Some("tu_001".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs", "content": "fn main() {}"}),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_tool_result() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_001".into()),
                output: "fn main() { println!(\"hello\"); }".into(),
                is_error: false,
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_error() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_run_started() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "Initializing agent run".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_run_completed() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunCompleted {
                message: "Task completed successfully".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_assistant_message() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantMessage {
                text: "I've completed the requested changes.".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_warning() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Warning {
                message: "Token budget 80% consumed".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_event_file_changed() {
    let env = Envelope::Event {
        ref_id: fixed_uuid().to_string(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "modified function signature".into(),
            },
            ext: None,
        },
    };
    insta::assert_json_snapshot!(env);
}

// ===========================================================================
// 4. Final envelope snapshots
// ===========================================================================

#[test]
fn envelope_final_with_receipt() {
    let env = Envelope::Final {
        ref_id: fixed_uuid().to_string(),
        receipt: sample_receipt(),
    };
    let json_str = serde_json::to_string_pretty(&env).unwrap();
    insta::assert_snapshot!(json_str);
}

// ===========================================================================
// 5. Fatal envelope snapshots
// ===========================================================================

#[test]
fn envelope_fatal_with_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some(fixed_uuid().to_string()),
        error: "sidecar process exited with code 137 (OOM killed)".into(),
    };
    insta::assert_json_snapshot!(env);
}

#[test]
fn envelope_fatal_without_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "failed to initialize: missing API key".into(),
    };
    insta::assert_json_snapshot!(env);
}

// ===========================================================================
// 6. Batch type snapshots
// ===========================================================================

#[test]
fn batch_item_status_success() {
    insta::assert_json_snapshot!(BatchItemStatus::Success);
}

#[test]
fn batch_item_status_failed() {
    let s = BatchItemStatus::Failed {
        error: "invalid JSON: unexpected token".into(),
    };
    insta::assert_json_snapshot!(s);
}

#[test]
fn batch_item_status_skipped() {
    let s = BatchItemStatus::Skipped {
        reason: "duplicate envelope detected".into(),
    };
    insta::assert_json_snapshot!(s);
}

#[test]
fn batch_request_snapshot() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
    };
    let req = BatchRequest {
        id: "batch-001".into(),
        envelopes: vec![fatal],
        created_at: "2025-01-15T12:00:00Z".into(),
    };
    insta::assert_json_snapshot!(req);
}

#[test]
fn batch_response_snapshot() {
    let resp = BatchResponse {
        request_id: "batch-001".into(),
        results: vec![
            BatchResult {
                index: 0,
                status: BatchItemStatus::Success,
                envelope: Some(Envelope::Fatal {
                    ref_id: None,
                    error: "test".into(),
                }),
            },
            BatchResult {
                index: 1,
                status: BatchItemStatus::Failed {
                    error: "decode error".into(),
                },
                envelope: None,
            },
        ],
        total_duration_ms: 42,
    };
    insta::assert_json_snapshot!(resp);
}

#[test]
fn batch_result_success() {
    let r = BatchResult {
        index: 0,
        status: BatchItemStatus::Success,
        envelope: None,
    };
    insta::assert_json_snapshot!(r);
}

#[test]
fn batch_result_failed() {
    let r = BatchResult {
        index: 3,
        status: BatchItemStatus::Failed {
            error: "serialization error".into(),
        },
        envelope: None,
    };
    insta::assert_json_snapshot!(r);
}
