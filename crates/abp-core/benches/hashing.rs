// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RunMetadata, RuntimeConfig, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash, sha256_hex,
};
use chrono::Utc;
use std::collections::BTreeMap;
use uuid::Uuid;

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "Refactor the authentication module".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/home/user/project".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into(), "tests/**".into()],
            exclude: vec!["target/**".into(), "*.log".into()],
        },
        context: ContextPacket {
            files: vec!["src/auth.rs".into(), "src/lib.rs".into()],
            snippets: vec![],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into(), "Bash".into()],
            disallowed_tools: vec!["DeleteFile".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/locked/**".into()],
            ..PolicyProfile::default()
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("claude-sonnet".into()),
            max_turns: Some(10),
            ..RuntimeConfig::default()
        },
    }
}

fn sample_receipt() -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"tokens": 500}),
        usage: UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(300),
            ..UsageNormalized::default()
        },
        trace: vec![
            AgentEvent {
                ts: now,
                kind: AgentEventKind::RunStarted {
                    message: "starting".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: now,
                kind: AgentEventKind::ToolCall {
                    tool_name: "Read".into(),
                    tool_use_id: Some("tu_1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "src/lib.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn bench_canonical_json(c: &mut Criterion) {
    let wo = sample_work_order();
    c.bench_function("canonical_json/work_order", |b| {
        b.iter(|| canonical_json(black_box(&wo)).unwrap())
    });
}

fn bench_sha256_hex(c: &mut Criterion) {
    let data = vec![0xABu8; 1024];
    c.bench_function("sha256_hex/1kb", |b| {
        b.iter(|| sha256_hex(black_box(&data)))
    });
}

fn bench_receipt_hash(c: &mut Criterion) {
    let receipt = sample_receipt();
    c.bench_function("receipt_hash", |b| {
        b.iter(|| receipt_hash(black_box(&receipt)).unwrap())
    });
}

fn bench_receipt_with_hash(c: &mut Criterion) {
    let receipt = sample_receipt();
    c.bench_function("receipt_with_hash", |b| {
        b.iter(|| black_box(receipt.clone()).with_hash().unwrap())
    });
}

criterion_group!(
    benches,
    bench_canonical_json,
    bench_sha256_hex,
    bench_receipt_hash,
    bench_receipt_with_hash,
);
criterion_main!(benches);
