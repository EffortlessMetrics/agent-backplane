// SPDX-License-Identifier: MIT OR Apache-2.0
//! Golden-file snapshot tests for protocol envelope serialization and JSONL streams.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use insta::{assert_json_snapshot, assert_snapshot};
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
            vendor: BTreeMap::from([("abp".into(), json!({"mode": "mapped"}))]),
            env: BTreeMap::from([("RUST_LOG".into(), "debug".into())]),
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
            kind: "patch".into(),
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

// ---------------------------------------------------------------------------
// Envelope variant snapshots
// ---------------------------------------------------------------------------

#[test]
fn golden_envelope_hello() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("0.1.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Emulated),
        ]),
        ExecutionMode::Passthrough,
    );
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("golden_envelope_hello", value);
}

#[test]
fn golden_envelope_run() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: sample_work_order(),
    };
    assert_json_snapshot!("golden_envelope_run", env);
}

#[test]
fn golden_envelope_event() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "file.rs"}),
            },
            ext: None,
        },
    };
    assert_json_snapshot!("golden_envelope_event", env);
}

#[test]
fn golden_envelope_final() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: sample_receipt(),
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("golden_envelope_final", value);
}

#[test]
fn golden_envelope_fatal() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "sidecar crashed".into(),
    };
    assert_json_snapshot!("golden_envelope_fatal", env);
}

#[test]
fn golden_envelope_fatal_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
    };
    assert_json_snapshot!("golden_envelope_fatal_no_ref", env);
}

// ---------------------------------------------------------------------------
// JSONL stream snapshot
// ---------------------------------------------------------------------------

#[test]
fn golden_jsonl_stream() {
    let ts = fixed_ts();

    let envelopes = [
        Envelope::hello(
            BackendIdentity {
                id: "test-sidecar".into(),
                backend_version: Some("0.1.0".into()),
                adapter_version: None,
            },
            BTreeMap::from([(Capability::Streaming, SupportLevel::Native)]),
        ),
        Envelope::Run {
            id: "run-001".into(),
            work_order: sample_work_order(),
        },
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts,
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts,
                kind: AgentEventKind::AssistantDelta {
                    text: "working...".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts,
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "run-001".into(),
            receipt: sample_receipt(),
        },
    ];

    let stream: String = envelopes
        .iter()
        .map(|e| JsonlCodec::encode(e).unwrap())
        .collect();

    assert_snapshot!("golden_jsonl_stream", stream);
}
