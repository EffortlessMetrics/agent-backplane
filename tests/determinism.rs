// SPDX-License-Identifier: MIT OR Apache-2.0
//! Determinism tests for ABP contract types.
//!
//! These tests guard against accidental non-determinism from HashMap usage,
//! random ordering, or inconsistent serialization. Every test verifies that
//! the same inputs always produce byte-identical outputs.

use std::collections::BTreeMap;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    canonical_json, receipt_hash,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_integrations::projection::{Dialect, ProjectionMatrix};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────

const FIXED_UUID: Uuid = Uuid::from_bytes([
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
]);
const FIXED_UUID2: Uuid = Uuid::from_bytes([
    0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
]);

fn fixed_timestamp() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_timestamp2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

fn make_work_order() -> WorkOrder {
    WorkOrder {
        id: FIXED_UUID,
        task: "Fix the login bug".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/workspace".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["src/main.rs".into(), "README.md".into()],
            snippets: vec![ContextSnippet {
                name: "error log".into(),
                content: "NullPointerException at line 42".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/lockfile".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["DeleteFile".into()],
        },
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::from([
                ("key_a".into(), serde_json::json!("value_a")),
                ("key_b".into(), serde_json::json!(42)),
                ("key_z".into(), serde_json::json!(true)),
            ]),
            env: BTreeMap::from([
                ("HOME".into(), "/home/user".into()),
                ("PATH".into(), "/usr/bin".into()),
            ]),
            max_budget_usd: Some(1.5),
            max_turns: Some(10),
        },
    }
}

fn make_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: FIXED_UUID,
            work_order_id: FIXED_UUID2,
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: fixed_timestamp(),
            finished_at: fixed_timestamp2(),
            duration_ms: 42_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::from([
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Emulated),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        },
        trace: vec![
            AgentEvent {
                ts: fixed_timestamp(),
                kind: AgentEventKind::RunStarted {
                    message: "Starting run".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_timestamp2(),
                kind: AgentEventKind::RunCompleted {
                    message: "Done".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file b/file".into()),
            git_status: Some("M file".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(ts: chrono::DateTime<Utc>, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

// ── 1. WorkOrder serialization determinism ──────────────────────────

#[test]
fn work_order_serializes_identically_100_times() {
    let wo = make_work_order();
    let reference = serde_json::to_string(&wo).unwrap();

    for i in 0..100 {
        let json = serde_json::to_string(&wo).unwrap();
        assert_eq!(
            json, reference,
            "WorkOrder serialization diverged on iteration {i}"
        );
    }
}

// ── 2. Receipt hash determinism ─────────────────────────────────────

#[test]
fn receipt_hash_is_identical_100_times() {
    let receipt = make_receipt();
    let reference = receipt_hash(&receipt).unwrap();

    for i in 0..100 {
        let hash = receipt_hash(&receipt).unwrap();
        assert_eq!(hash, reference, "Receipt hash diverged on iteration {i}");
    }
}

#[test]
fn receipt_with_hash_is_deterministic() {
    let r1 = make_receipt().with_hash().unwrap();
    let r2 = make_receipt().with_hash().unwrap();

    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert!(r1.receipt_sha256.is_some());
    assert_eq!(r1.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_excludes_self_referential_field() {
    let mut receipt = make_receipt();
    let hash1 = receipt_hash(&receipt).unwrap();

    receipt.receipt_sha256 = Some("bogus_value".into());
    let hash2 = receipt_hash(&receipt).unwrap();

    // The hash must be the same regardless of the receipt_sha256 field value.
    assert_eq!(hash1, hash2);
}

// ── 3. BTreeMap key ordering ────────────────────────────────────────

#[test]
fn btreemap_vendor_keys_are_alphabetical() {
    let wo = make_work_order();
    let json: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let vendor = json["config"]["vendor"].as_object().unwrap();
    let keys: Vec<&String> = vendor.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "Vendor map keys should be in alphabetical order"
    );
}

#[test]
fn btreemap_env_keys_are_alphabetical() {
    let wo = make_work_order();
    let json: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let env = json["config"]["env"].as_object().unwrap();
    let keys: Vec<&String> = env.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "Env map keys should be in alphabetical order");
}

#[test]
fn capability_manifest_keys_are_ordered() {
    let receipt = make_receipt();
    let json: serde_json::Value = serde_json::to_value(&receipt).unwrap();
    let caps = json["capabilities"].as_object().unwrap();
    let keys: Vec<&String> = caps.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "CapabilityManifest keys should be in sorted order"
    );
}

// ── 4. Canonical JSON byte-identical ────────────────────────────────

#[test]
fn canonical_json_is_byte_identical_across_runs() {
    let wo = make_work_order();
    let reference = canonical_json(&wo).unwrap();

    for i in 0..100 {
        let json = canonical_json(&wo).unwrap();
        assert_eq!(
            json.as_bytes(),
            reference.as_bytes(),
            "Canonical JSON bytes diverged on iteration {i}"
        );
    }
}

#[test]
fn canonical_json_receipt_is_byte_identical() {
    let receipt = make_receipt();
    let reference = canonical_json(&receipt).unwrap();

    for i in 0..100 {
        let json = canonical_json(&receipt).unwrap();
        assert_eq!(
            json.as_bytes(),
            reference.as_bytes(),
            "Canonical JSON receipt bytes diverged on iteration {i}"
        );
    }
}

// ── 5. Envelope batch serialization ─────────────────────────────────

#[test]
fn envelope_batch_serialization_order_is_preserved() {
    let envelopes = [
        Envelope::hello(
            BackendIdentity {
                id: "sidecar-a".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        ),
        Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "timeout".into(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "oom".into(),
        },
    ];

    let reference: Vec<String> = envelopes
        .iter()
        .map(|e| JsonlCodec::encode(e).unwrap())
        .collect();

    for i in 0..100 {
        let batch: Vec<String> = envelopes
            .iter()
            .map(|e| JsonlCodec::encode(e).unwrap())
            .collect();
        assert_eq!(
            batch, reference,
            "Envelope batch serialization diverged on iteration {i}"
        );
    }
}

#[test]
fn envelope_roundtrip_is_deterministic() {
    let envelope = Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("2.0".into()),
            adapter_version: None,
        },
        BTreeMap::from([
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::Streaming, SupportLevel::Emulated),
        ]),
    );

    let encoded = JsonlCodec::encode(&envelope).unwrap();
    for _ in 0..100 {
        let re_encoded = JsonlCodec::encode(&envelope).unwrap();
        assert_eq!(encoded, re_encoded);
    }
}

// ── 6. PolicyProfile decisions determinism ──────────────────────────

#[test]
fn policy_engine_decisions_are_deterministic() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into(), "Write".into()],
        deny_read: vec!["**/.env".into(), "**/secret/**".into()],
        deny_write: vec!["**/locked/**".into()],
        ..PolicyProfile::default()
    };

    let engine = PolicyEngine::new(&policy).unwrap();

    let tool_cases = &["Read", "Write", "Bash", "Grep", "Unknown", "DeleteFile"];
    let read_paths = &[".env", "src/lib.rs", "secret/key.pem", "config/app.toml"];
    let write_paths = &["locked/data.txt", "src/main.rs", "output.log"];

    // Capture reference decisions.
    let tool_ref: Vec<bool> = tool_cases
        .iter()
        .map(|t| engine.can_use_tool(t).allowed)
        .collect();
    let read_ref: Vec<bool> = read_paths
        .iter()
        .map(|p| engine.can_read_path(Path::new(p)).allowed)
        .collect();
    let write_ref: Vec<bool> = write_paths
        .iter()
        .map(|p| engine.can_write_path(Path::new(p)).allowed)
        .collect();

    // Re-create engine and verify same decisions.
    for _ in 0..50 {
        let engine2 = PolicyEngine::new(&policy).unwrap();
        let tool_now: Vec<bool> = tool_cases
            .iter()
            .map(|t| engine2.can_use_tool(t).allowed)
            .collect();
        let read_now: Vec<bool> = read_paths
            .iter()
            .map(|p| engine2.can_read_path(Path::new(p)).allowed)
            .collect();
        let write_now: Vec<bool> = write_paths
            .iter()
            .map(|p| engine2.can_write_path(Path::new(p)).allowed)
            .collect();

        assert_eq!(tool_ref, tool_now);
        assert_eq!(read_ref, read_now);
        assert_eq!(write_ref, write_now);
    }
}

// ── 7. Glob match determinism ───────────────────────────────────────

#[test]
fn glob_patterns_produce_same_results() {
    let include = vec!["src/**".into(), "tests/**".into()];
    let exclude = vec!["src/generated/**".into(), "tests/fixtures/**".into()];

    let paths = [
        "src/lib.rs",
        "src/generated/out.rs",
        "tests/unit.rs",
        "tests/fixtures/data.json",
        "README.md",
        "docs/guide.md",
    ];

    let reference_globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();
    let reference: Vec<MatchDecision> = paths
        .iter()
        .map(|p| reference_globs.decide_str(p))
        .collect();

    for _ in 0..100 {
        let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();
        let results: Vec<MatchDecision> = paths.iter().map(|p| globs.decide_str(p)).collect();
        assert_eq!(results, reference);
    }
}

#[test]
fn glob_path_vs_str_consistency() {
    let include = vec!["src/**".into()];
    let exclude = vec!["src/secret/**".into()];
    let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();

    let cases = [
        "src/lib.rs",
        "src/secret/key.pem",
        "README.md",
        "tests/it.rs",
    ];
    for _ in 0..100 {
        for c in &cases {
            assert_eq!(
                globs.decide_str(c),
                globs.decide_path(Path::new(c)),
                "str vs path mismatch for '{c}'"
            );
        }
    }
}

// ── 8. ProjectionMatrix translations determinism ────────────────────

#[test]
fn projection_matrix_identity_is_deterministic() {
    let matrix = ProjectionMatrix::new();
    let wo = make_work_order();

    let reference = matrix.translate(Dialect::Abp, Dialect::Abp, &wo).unwrap();

    for i in 0..100 {
        let result = matrix.translate(Dialect::Abp, Dialect::Abp, &wo).unwrap();
        assert_eq!(
            result, reference,
            "Identity translation diverged on iteration {i}"
        );
    }
}

#[test]
fn projection_matrix_abp_to_claude_is_deterministic() {
    let matrix = ProjectionMatrix::new();
    let wo = make_work_order();

    let reference = matrix
        .translate(Dialect::Abp, Dialect::Claude, &wo)
        .unwrap();

    for i in 0..100 {
        let result = matrix
            .translate(Dialect::Abp, Dialect::Claude, &wo)
            .unwrap();
        assert_eq!(
            result, reference,
            "ABP→Claude translation diverged on iteration {i}"
        );
    }
}

#[test]
fn projection_matrix_all_vendors_deterministic() {
    let matrix = ProjectionMatrix::new();
    let wo = make_work_order();

    let targets = [
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
    ];
    let references: Vec<serde_json::Value> = targets
        .iter()
        .map(|t| matrix.translate(Dialect::Abp, *t, &wo).unwrap())
        .collect();

    for _ in 0..50 {
        for (i, target) in targets.iter().enumerate() {
            let result = matrix.translate(Dialect::Abp, *target, &wo).unwrap();
            assert_eq!(result, references[i], "Translation to {target:?} diverged");
        }
    }
}

#[test]
fn projection_supported_translations_order_is_stable() {
    let matrix = ProjectionMatrix::new();
    let reference = matrix.supported_translations();

    for _ in 0..100 {
        assert_eq!(matrix.supported_translations(), reference);
    }
}

// ── 9. WorkOrderBuilder determinism ─────────────────────────────────

#[test]
fn work_order_builder_produces_identical_output_for_same_inputs() {
    // The builder generates a random UUID via new_v4(), so we compare
    // everything except `id` to verify structural determinism.
    let build = || {
        WorkOrderBuilder::new("Fix the bug")
            .lane(ExecutionLane::WorkspaceFirst)
            .root("/tmp/ws")
            .workspace_mode(WorkspaceMode::Staged)
            .include(vec!["src/**".into()])
            .exclude(vec!["target/**".into()])
            .model("gpt-4")
            .max_turns(10)
            .max_budget_usd(2.0)
            .build()
    };

    let ref_wo = build();
    let ref_json = {
        let mut v = serde_json::to_value(&ref_wo).unwrap();
        v.as_object_mut().unwrap().remove("id");
        serde_json::to_string(&v).unwrap()
    };

    for i in 0..100 {
        let wo = build();
        let mut v = serde_json::to_value(&wo).unwrap();
        v.as_object_mut().unwrap().remove("id");
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(
            json, ref_json,
            "WorkOrderBuilder output diverged on iteration {i}"
        );
    }
}

// ── 10. ReceiptBuilder determinism ──────────────────────────────────

#[test]
fn receipt_builder_produces_identical_output_for_same_inputs() {
    let ts = fixed_timestamp();
    let ts2 = fixed_timestamp2();

    let build = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .work_order_id(FIXED_UUID)
            .started_at(ts)
            .finished_at(ts2)
            .backend_version("1.0")
            .adapter_version("0.1")
            .mode(ExecutionMode::Mapped)
            .capabilities(BTreeMap::from([(
                Capability::ToolRead,
                SupportLevel::Native,
            )]))
            .build()
    };

    let ref_receipt = build();
    // run_id is random, strip it for comparison.
    let normalize = |r: &Receipt| {
        let mut v = serde_json::to_value(r).unwrap();
        v["meta"].as_object_mut().unwrap().remove("run_id");
        serde_json::to_string(&v).unwrap()
    };

    let ref_json = normalize(&ref_receipt);
    for i in 0..100 {
        let r = build();
        let json = normalize(&r);
        assert_eq!(
            json, ref_json,
            "ReceiptBuilder output diverged on iteration {i}"
        );
    }
}

#[test]
fn receipt_builder_with_hash_produces_same_hash_for_same_fixed_receipt() {
    // Construct two receipts with identical content (including fixed run_id).
    let build = || Receipt {
        meta: RunMetadata {
            run_id: FIXED_UUID,
            work_order_id: FIXED_UUID2,
            contract_version: abp_core::CONTRACT_VERSION.into(),
            started_at: fixed_timestamp(),
            finished_at: fixed_timestamp2(),
            duration_ms: 42_000,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };

    let hash1 = build().with_hash().unwrap().receipt_sha256;
    let hash2 = build().with_hash().unwrap().receipt_sha256;
    assert_eq!(hash1, hash2);
}

// ── 11. Event serialization consistency ─────────────────────────────

#[test]
fn event_serialization_order_is_consistent() {
    let events = [
        make_event(
            fixed_timestamp(),
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
        ),
        make_event(
            fixed_timestamp(),
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/lib.rs"}),
            },
        ),
        make_event(
            fixed_timestamp(),
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
        ),
        make_event(
            fixed_timestamp(),
            AgentEventKind::AssistantMessage {
                text: "I read the file.".into(),
            },
        ),
        make_event(
            fixed_timestamp(),
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "Added function".into(),
            },
        ),
        make_event(
            fixed_timestamp2(),
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ),
    ];

    let reference: Vec<String> = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();

    for i in 0..100 {
        let batch: Vec<String> = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        assert_eq!(
            batch, reference,
            "Event serialization diverged on iteration {i}"
        );
    }
}

#[test]
fn event_with_ext_map_has_sorted_keys() {
    let event = AgentEvent {
        ts: fixed_timestamp(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: Some(BTreeMap::from([
            ("zebra".into(), serde_json::json!("z")),
            ("alpha".into(), serde_json::json!("a")),
            ("middle".into(), serde_json::json!("m")),
        ])),
    };

    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    let ext = json["ext"].as_object().unwrap();
    let keys: Vec<&String> = ext.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "ext BTreeMap keys must be sorted");
}

#[test]
fn agent_event_kind_tag_is_deterministic() {
    let kinds = [
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "end".into(),
        },
        AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        AgentEventKind::Warning {
            message: "warn".into(),
        },
        AgentEventKind::Error {
            message: "err".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: Some("file.txt".into()),
        },
    ];

    let reference: Vec<String> = kinds
        .iter()
        .map(|k| serde_json::to_string(k).unwrap())
        .collect();

    for _ in 0..100 {
        let batch: Vec<String> = kinds
            .iter()
            .map(|k| serde_json::to_string(k).unwrap())
            .collect();
        assert_eq!(batch, reference);
    }
}
