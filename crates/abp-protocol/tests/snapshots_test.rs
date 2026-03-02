// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
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
        capabilities: BTreeMap::new(),
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

#[test]
fn snapshot_hello_envelope() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("0.1.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
    );
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("envelope_hello", value);
}

#[test]
fn snapshot_run_envelope() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: sample_work_order(),
    };
    assert_json_snapshot!("envelope_run", env);
}

#[test]
fn snapshot_event_envelope_assistant_delta() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        },
    };
    assert_json_snapshot!("envelope_event_assistant_delta", env);
}

#[test]
fn snapshot_event_envelope_tool_call() {
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
    assert_json_snapshot!("envelope_event_tool_call", env);
}

#[test]
fn snapshot_event_envelope_file_changed() {
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "added fn".into(),
            },
            ext: None,
        },
    };
    assert_json_snapshot!("envelope_event_file_changed", env);
}

#[test]
fn snapshot_final_envelope() {
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: sample_receipt(),
    };
    let value = serde_json::to_value(&env).unwrap();
    assert_json_snapshot!("envelope_final", value);
}

#[test]
fn snapshot_fatal_envelope() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "sidecar crashed".into(),
        error_code: None,
    };
    assert_json_snapshot!("envelope_fatal", env);
}

#[test]
fn snapshot_fatal_envelope_no_ref() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    assert_json_snapshot!("envelope_fatal_no_ref", env);
}

/// Verify JSONL codec produces valid newline-terminated output that round-trips.
#[test]
fn jsonl_codec_round_trip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "test".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    let re_encoded = JsonlCodec::encode(&decoded).unwrap();
    assert_eq!(encoded, re_encoded);
}
