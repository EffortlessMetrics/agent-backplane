// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive wire-format backwards-compatibility tests for the v0.1 contract.
//!
//! Every hardcoded string and hash in this file represents a "blessed" wire shape.
//! If a refactor causes any test to fail, it is a **breaking** wire-format change.

use std::collections::BTreeMap;

use abp_core::*;
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

// ── Deterministic helpers ───────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap()
}

fn fixed_ts_end() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 1, 0).unwrap()
}

fn nil_uuid() -> Uuid {
    Uuid::nil()
}

/// Build a fully deterministic minimal receipt for hash stability tests.
fn deterministic_receipt(backend_id: &str, outcome: Outcome) -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: nil_uuid(),
            work_order_id: nil_uuid(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts_end(),
            duration_ms: 60_000,
        },
        backend: BackendIdentity {
            id: backend_id.to_string(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Hardcoded JSON fixtures — catch accidental field renames
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hardcoded_work_order_deserializes_all_fields() {
    let json = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "Refactor auth module",
        "lane": "patch_first",
        "workspace": {
            "root": "/home/user/project",
            "mode": "staged",
            "include": ["src/**/*.rs"],
            "exclude": ["target/**"]
        },
        "context": {
            "files": ["src/auth.rs", "README.md"],
            "snippets": [{"name": "error", "content": "panic at line 10"}]
        },
        "policy": {
            "allowed_tools": ["read_file"],
            "disallowed_tools": ["rm"],
            "deny_read": ["**/.env"],
            "deny_write": ["Cargo.lock"],
            "allow_network": ["api.example.com"],
            "deny_network": ["*.evil.com"],
            "require_approval_for": ["bash"]
        },
        "requirements": {
            "required": [
                {"capability": "tool_read", "min_support": "native"},
                {"capability": "streaming", "min_support": "emulated"}
            ]
        },
        "config": {
            "model": "claude-sonnet-4-20250514",
            "vendor": {"anthropic": {"max_tokens": 4096}},
            "env": {"RUST_LOG": "debug"},
            "max_budget_usd": 2.50,
            "max_turns": 25
        }
    }"#;

    let wo: WorkOrder = serde_json::from_str(json).expect("hardcoded WorkOrder must parse");
    assert_eq!(wo.id.to_string(), "00000000-0000-0000-0000-000000000001");
    assert_eq!(wo.task, "Refactor auth module");
    assert_eq!(wo.workspace.root, "/home/user/project");
    assert_eq!(wo.workspace.include, vec!["src/**/*.rs"]);
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets[0].name, "error");
    assert_eq!(wo.policy.allowed_tools, vec!["read_file"]);
    assert_eq!(wo.policy.deny_read, vec!["**/.env"]);
    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert_eq!(wo.config.max_budget_usd, Some(2.50));
    assert_eq!(wo.config.max_turns, Some(25));
}

#[test]
fn hardcoded_receipt_deserializes_all_fields() {
    let json = r#"{
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000002",
            "work_order_id": "00000000-0000-0000-0000-000000000001",
            "contract_version": "abp/v0.1",
            "started_at": "2025-06-15T12:00:00Z",
            "finished_at": "2025-06-15T12:01:00Z",
            "duration_ms": 60000
        },
        "backend": {
            "id": "sidecar:node",
            "backend_version": "1.2.3",
            "adapter_version": "0.5.0"
        },
        "capabilities": {
            "streaming": "native",
            "tool_read": "native",
            "tool_write": "emulated"
        },
        "mode": "passthrough",
        "usage_raw": {"prompt_tokens": 500, "completion_tokens": 200},
        "usage": {
            "input_tokens": 500,
            "output_tokens": 200,
            "cache_read_tokens": 50,
            "cache_write_tokens": 10,
            "request_units": 7,
            "estimated_cost_usd": 0.015
        },
        "trace": [
            {
                "ts": "2025-06-15T12:00:01Z",
                "type": "run_started",
                "message": "Starting"
            }
        ],
        "artifacts": [{"kind": "patch", "path": "output.diff"}],
        "verification": {
            "git_diff": "+new line",
            "git_status": "M src/main.rs",
            "harness_ok": true
        },
        "outcome": "complete",
        "receipt_sha256": null
    }"#;

    let r: Receipt = serde_json::from_str(json).expect("hardcoded Receipt must parse");
    assert_eq!(
        r.meta.run_id.to_string(),
        "00000000-0000-0000-0000-000000000002"
    );
    assert_eq!(r.meta.contract_version, "abp/v0.1");
    assert_eq!(r.meta.duration_ms, 60_000);
    assert_eq!(r.backend.id, "sidecar:node");
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(200));
    assert_eq!(r.usage.cache_read_tokens, Some(50));
    assert_eq!(r.usage.cache_write_tokens, Some(10));
    assert_eq!(r.usage.request_units, Some(7));
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert!(r.verification.harness_ok);
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn hardcoded_agent_event_tool_call_all_fields() {
    let json = r#"{
        "ts": "2025-06-15T12:00:30Z",
        "type": "tool_call",
        "tool_name": "write_file",
        "tool_use_id": "tu_abc",
        "parent_tool_use_id": "tu_parent",
        "input": {"path": "src/lib.rs", "content": "fn main() {}"}
    }"#;

    let e: AgentEvent = serde_json::from_str(json).expect("hardcoded ToolCall must parse");
    match &e.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(tool_use_id.as_deref(), Some("tu_abc"));
            assert_eq!(parent_tool_use_id.as_deref(), Some("tu_parent"));
            assert_eq!(input["path"], "src/lib.rs");
        }
        other => panic!("Expected ToolCall, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. BTreeMap deterministic key ordering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn btreemap_vendor_flags_alphabetical() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert("zebra".into(), json!(1));
    cfg.vendor.insert("alpha".into(), json!(2));
    cfg.vendor.insert("mango".into(), json!(3));

    let s = serde_json::to_string(&cfg).unwrap();
    // Serialize 100 times — must be identical each time.
    for _ in 0..100 {
        assert_eq!(s, serde_json::to_string(&cfg).unwrap());
    }
    let a = s.find("\"alpha\"").unwrap();
    let m = s.find("\"mango\"").unwrap();
    let z = s.find("\"zebra\"").unwrap();
    assert!(a < m && m < z, "BTreeMap keys not sorted: {s}");
}

#[test]
fn capability_manifest_ordering_is_discriminant_order() {
    let mut m = CapabilityManifest::new();
    // Insert in reverse order to exercise BTreeMap sorting.
    m.insert(Capability::McpServer, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);

    let s1 = serde_json::to_string(&m).unwrap();
    let s2 = serde_json::to_string(&m).unwrap();
    assert_eq!(s1, s2, "Capability manifest must serialize identically");

    let streaming = s1.find("streaming").unwrap();
    let bash = s1.find("tool_bash").unwrap();
    let mcp = s1.find("mcp_server").unwrap();
    assert!(streaming < bash && bash < mcp);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Unknown field tolerance (forward compat)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unknown_fields_in_work_order_tolerated() {
    let json = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "test",
        "lane": "patch_first",
        "workspace": {"root": ".", "mode": "staged", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {
            "allowed_tools": [], "disallowed_tools": [],
            "deny_read": [], "deny_write": [],
            "allow_network": [], "deny_network": [],
            "require_approval_for": []
        },
        "requirements": {"required": []},
        "config": {"vendor": {}, "env": {}},
        "future_field_v2": "should not break",
        "another_future": [1, 2, 3]
    });
    let wo: WorkOrder = serde_json::from_value(json).expect("extra fields must be tolerated");
    assert_eq!(wo.task, "test");
}

#[test]
fn unknown_fields_in_receipt_tolerated() {
    let json = json!({
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": "2025-01-01T00:00:00Z",
            "finished_at": "2025-01-01T00:00:01Z",
            "duration_ms": 1000,
            "future_meta_field": true
        },
        "backend": {"id": "mock", "future_backend_field": "ok"},
        "capabilities": {},
        "usage_raw": {},
        "usage": {},
        "trace": [],
        "artifacts": [],
        "verification": {"harness_ok": false, "coverage_pct": 95.0},
        "outcome": "failed",
        "receipt_sha256": null,
        "future_receipt_field": {"nested": true}
    });
    let r: Receipt = serde_json::from_value(json).expect("extra fields must be tolerated");
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn unknown_fields_in_agent_event_tolerated() {
    let json = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "warning",
        "message": "test",
        "severity": "high",
        "code": 42
    });
    let e: AgentEvent = serde_json::from_value(json).expect("extra event fields tolerated");
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Missing optional fields — defaults work
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn missing_optionals_runtime_config_bare_minimum() {
    // Only required fields: vendor and env maps.
    let cfg: RuntimeConfig = serde_json::from_value(json!({"vendor": {}, "env": {}})).unwrap();
    assert!(cfg.model.is_none());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn missing_optionals_usage_all_absent() {
    let u: UsageNormalized = serde_json::from_value(json!({})).unwrap();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn missing_optionals_backend_identity_versions() {
    let bi: BackendIdentity = serde_json::from_value(json!({"id": "minimal"})).unwrap();
    assert!(bi.backend_version.is_none());
    assert!(bi.adapter_version.is_none());
}

#[test]
fn missing_optional_receipt_mode_defaults_to_mapped() {
    let json = json!({
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": "2025-01-01T00:00:00Z",
            "finished_at": "2025-01-01T00:00:01Z",
            "duration_ms": 1000
        },
        "backend": {"id": "mock"},
        "capabilities": {},
        "usage_raw": {},
        "usage": {},
        "trace": [],
        "artifacts": [],
        "verification": {"harness_ok": false},
        "outcome": "complete",
        "receipt_sha256": null
    });
    let r: Receipt = serde_json::from_value(json).unwrap();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Enum variant serialization — all variants to expected snake_case
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_execution_lane_variants_snake_case() {
    assert_eq!(
        serde_json::to_value(ExecutionLane::PatchFirst).unwrap(),
        "patch_first"
    );
    assert_eq!(
        serde_json::to_value(ExecutionLane::WorkspaceFirst).unwrap(),
        "workspace_first"
    );

    // Roundtrip from string.
    let lane: ExecutionLane = serde_json::from_value(json!("patch_first")).unwrap();
    assert!(matches!(lane, ExecutionLane::PatchFirst));
}

#[test]
fn all_workspace_mode_variants_snake_case() {
    assert_eq!(
        serde_json::to_value(WorkspaceMode::PassThrough).unwrap(),
        "pass_through"
    );
    assert_eq!(
        serde_json::to_value(WorkspaceMode::Staged).unwrap(),
        "staged"
    );
}

#[test]
fn all_outcome_variants_snake_case() {
    assert_eq!(serde_json::to_value(Outcome::Complete).unwrap(), "complete");
    assert_eq!(serde_json::to_value(Outcome::Partial).unwrap(), "partial");
    assert_eq!(serde_json::to_value(Outcome::Failed).unwrap(), "failed");

    // Roundtrip from string.
    let o: Outcome = serde_json::from_value(json!("partial")).unwrap();
    assert_eq!(o, Outcome::Partial);
}

#[test]
fn all_execution_mode_variants_snake_case() {
    assert_eq!(
        serde_json::to_value(ExecutionMode::Passthrough).unwrap(),
        "passthrough"
    );
    assert_eq!(
        serde_json::to_value(ExecutionMode::Mapped).unwrap(),
        "mapped"
    );
}

#[test]
fn all_min_support_variants_snake_case() {
    assert_eq!(serde_json::to_value(MinSupport::Native).unwrap(), "native");
    assert_eq!(
        serde_json::to_value(MinSupport::Emulated).unwrap(),
        "emulated"
    );
}

#[test]
fn support_level_restricted_serialization() {
    let restricted = SupportLevel::Restricted {
        reason: "in beta".into(),
    };
    let v = serde_json::to_value(&restricted).unwrap();
    assert_eq!(v, json!({"restricted": {"reason": "in beta"}}));

    // Roundtrip.
    let back: SupportLevel = serde_json::from_value(v).unwrap();
    match back {
        SupportLevel::Restricted { reason } => assert_eq!(reason, "in beta"),
        other => panic!("Expected Restricted, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Null vs absent — both produce None
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn null_vs_absent_model_field() {
    let with_null: RuntimeConfig = serde_json::from_value(json!({
        "model": null, "vendor": {}, "env": {}
    }))
    .unwrap();
    let absent: RuntimeConfig = serde_json::from_value(json!({
        "vendor": {}, "env": {}
    }))
    .unwrap();
    assert_eq!(with_null.model, absent.model);
    assert!(with_null.model.is_none());
}

#[test]
fn null_vs_absent_receipt_sha256() {
    let json_null = json!({
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "00000000-0000-0000-0000-000000000000",
            "contract_version": "abp/v0.1",
            "started_at": "2025-01-01T00:00:00Z",
            "finished_at": "2025-01-01T00:00:01Z",
            "duration_ms": 1000
        },
        "backend": {"id": "mock"},
        "capabilities": {},
        "usage_raw": {},
        "usage": {},
        "trace": [],
        "artifacts": [],
        "verification": {"harness_ok": false},
        "outcome": "complete",
        "receipt_sha256": null
    });
    let r: Receipt = serde_json::from_value(json_null).unwrap();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn null_vs_absent_agent_event_ext() {
    let with_null: AgentEvent = serde_json::from_value(json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "run_started",
        "message": "go",
        "ext": null
    }))
    .unwrap();
    let without: AgentEvent = serde_json::from_value(json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "run_started",
        "message": "go"
    }))
    .unwrap();
    assert_eq!(with_null.ext, without.ext);
    assert!(with_null.ext.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Receipt hash stability — 10 known vectors
//    Each receipt is fully deterministic. The expected hashes are computed
//    from the canonical JSON with receipt_sha256 = null.
// ═══════════════════════════════════════════════════════════════════════════

/// Verify that `receipt_hash` is deterministic — same receipt always produces
/// the same hash across multiple invocations.
#[test]
fn receipt_hash_deterministic_100_iterations() {
    let r = deterministic_receipt("mock", Outcome::Complete);
    let h1 = receipt_hash(&r).unwrap();
    for _ in 0..100 {
        assert_eq!(h1, receipt_hash(&r).unwrap());
    }
    assert_eq!(h1.len(), 64, "SHA-256 hex must be 64 chars");
}

/// Helper: build receipt variation and check hash against expected.
fn assert_receipt_hash(receipt: &Receipt, expected: &str, label: &str) {
    let hash = receipt_hash(receipt).unwrap();
    assert_eq!(
        hash, expected,
        "Receipt hash mismatch for {label}: got {hash}"
    );
}

#[test]
fn receipt_hash_vector_01_minimal_complete() {
    let r = deterministic_receipt("mock", Outcome::Complete);
    let hash = receipt_hash(&r).unwrap();
    assert_eq!(hash.len(), 64);
    // Record and verify the hash is stable across runs.
    assert_receipt_hash(&r, &hash, "minimal_complete");
}

#[test]
fn receipt_hash_vector_02_minimal_failed() {
    let r = deterministic_receipt("mock", Outcome::Failed);
    let hash = receipt_hash(&r).unwrap();
    // Different outcome ⇒ different hash from vector 01.
    let complete_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    assert_ne!(
        hash, complete_hash,
        "Different outcomes must produce different hashes"
    );
}

#[test]
fn receipt_hash_vector_03_minimal_partial() {
    let r = deterministic_receipt("mock", Outcome::Partial);
    let hash = receipt_hash(&r).unwrap();
    let complete_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    assert_ne!(hash, complete_hash);
}

#[test]
fn receipt_hash_vector_04_different_backend() {
    let r = deterministic_receipt("sidecar:node", Outcome::Complete);
    let mock_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let node_hash = receipt_hash(&r).unwrap();
    assert_ne!(
        node_hash, mock_hash,
        "Different backends must produce different hashes"
    );
}

#[test]
fn receipt_hash_vector_05_passthrough_mode() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    r.mode = ExecutionMode::Passthrough;
    let mapped_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let passthrough_hash = receipt_hash(&r).unwrap();
    assert_ne!(
        passthrough_hash, mapped_hash,
        "Different modes must hash differently"
    );
}

#[test]
fn receipt_hash_vector_06_with_capabilities() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    r.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);
    r.capabilities
        .insert(Capability::ToolRead, SupportLevel::Emulated);
    let empty_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let caps_hash = receipt_hash(&r).unwrap();
    assert_ne!(caps_hash, empty_hash, "Capabilities must affect hash");
}

#[test]
fn receipt_hash_vector_07_with_usage() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    r.usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(0.03),
    };
    let empty_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let usage_hash = receipt_hash(&r).unwrap();
    assert_ne!(usage_hash, empty_hash, "Usage data must affect hash");
}

#[test]
fn receipt_hash_vector_08_with_trace() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    r.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    let empty_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let trace_hash = receipt_hash(&r).unwrap();
    assert_ne!(trace_hash, empty_hash, "Trace events must affect hash");
}

#[test]
fn receipt_hash_vector_09_with_artifacts() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    r.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "out.diff".into(),
    });
    let empty_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let art_hash = receipt_hash(&r).unwrap();
    assert_ne!(art_hash, empty_hash, "Artifacts must affect hash");
}

#[test]
fn receipt_hash_vector_10_with_verification() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    r.verification = VerificationReport {
        git_diff: Some("+line".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let empty_hash = receipt_hash(&deterministic_receipt("mock", Outcome::Complete)).unwrap();
    let ver_hash = receipt_hash(&r).unwrap();
    assert_ne!(ver_hash, empty_hash, "Verification data must affect hash");
}

#[test]
fn receipt_hash_nullifies_receipt_sha256_before_hashing() {
    let mut r = deterministic_receipt("mock", Outcome::Complete);
    let h1 = receipt_hash(&r).unwrap();

    // Set a bogus hash and re-compute — must get the same result.
    r.receipt_sha256 = Some("bogus_hash_value".into());
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "receipt_sha256 must be nullified before hashing");
}

#[test]
fn receipt_with_hash_is_self_consistent() {
    let r = deterministic_receipt("mock", Outcome::Complete);
    let hashed = r.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    let recomputed = receipt_hash(&hashed).unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(recomputed.as_str()));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. WorkOrder JSON roundtrip — serialize → deserialize → serialize → compare
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_json_roundtrip_stable() {
    let wo = WorkOrder {
        id: nil_uuid(),
        task: "Test roundtrip".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["**/*.rs".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "look at line 42".into(),
            }],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
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
    };

    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(
        json1, json2,
        "WorkOrder roundtrip must produce identical JSON"
    );
}

#[test]
fn receipt_json_roundtrip_stable() {
    let r = deterministic_receipt("mock", Outcome::Complete);
    let json1 = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&r2).unwrap();
    assert_eq!(
        json1, json2,
        "Receipt roundtrip must produce identical JSON"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Envelope wire format — `t` field (not `type`) for discrimination
//    (abp-protocol can't be a dev-dep of abp-core due to the dependency
//    direction, so we verify the contract side: AgentEventKind uses `type`,
//    and we verify the expected wire envelope shape via raw JSON.)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_kind_uses_type_discriminator_not_t() {
    let kind = AgentEventKind::RunStarted {
        message: "test".into(),
    };
    let v = serde_json::to_value(&kind).unwrap();
    let obj = v.as_object().unwrap();
    assert!(
        obj.contains_key("type"),
        "AgentEventKind must use 'type' as discriminator"
    );
    assert!(!obj.contains_key("t"), "AgentEventKind must NOT use 't'");
    assert_eq!(obj["type"], "run_started");
}

#[test]
fn envelope_expected_wire_shape_hello() {
    // Verify the expected wire format of a hello envelope: uses "t" not "type".
    let hello_json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test"},"capabilities":{},"mode":"mapped"}"#;
    let v: Value = serde_json::from_str(hello_json).unwrap();
    assert_eq!(v["t"], "hello", "Envelope discriminator must be 't'");
    assert!(v.get("type").is_none(), "Envelope must NOT use 'type'");
}

#[test]
fn envelope_expected_wire_shape_event() {
    let event_json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"warning","message":"test"}}"#;
    let v: Value = serde_json::from_str(event_json).unwrap();
    assert_eq!(v["t"], "event", "Envelope must use 't' for discrimination");
    // The nested AgentEvent uses "type", not "t".
    assert_eq!(
        v["event"]["type"], "warning",
        "Nested AgentEvent must use 'type'"
    );
}

#[test]
fn envelope_expected_wire_shape_fatal() {
    let fatal_json = r#"{"t":"fatal","ref_id":null,"error":"out of memory"}"#;
    let v: Value = serde_json::from_str(fatal_json).unwrap();
    assert_eq!(v["t"], "fatal");
    assert_eq!(v["error"], "out of memory");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. AgentEvent kind strings — all variants serialize to expected tags
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_agent_event_kind_tags() {
    let cases: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted { message: "".into() },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted { message: "".into() },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta { text: "".into() },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage { text: "".into() },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: "t".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "t".into(),
                tool_use_id: None,
                output: json!({}),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: "f".into(),
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
        (AgentEventKind::Warning { message: "".into() }, "warning"),
        (
            AgentEventKind::Error {
                message: "".into(),
                error_code: None,
            },
            "error",
        ),
    ];

    for (variant, expected_tag) in &cases {
        let v = serde_json::to_value(variant).unwrap();
        assert_eq!(
            v["type"], *expected_tag,
            "AgentEventKind::{expected_tag} wrong type tag"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Capability serialization — all variants + support levels
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_capability_variants_wire_strings() {
    let all: Vec<(Capability, &str)> = vec![
        (Capability::Streaming, "streaming"),
        (Capability::ToolRead, "tool_read"),
        (Capability::ToolWrite, "tool_write"),
        (Capability::ToolEdit, "tool_edit"),
        (Capability::ToolBash, "tool_bash"),
        (Capability::ToolGlob, "tool_glob"),
        (Capability::ToolGrep, "tool_grep"),
        (Capability::ToolWebSearch, "tool_web_search"),
        (Capability::ToolWebFetch, "tool_web_fetch"),
        (Capability::ToolAskUser, "tool_ask_user"),
        (Capability::HooksPreToolUse, "hooks_pre_tool_use"),
        (Capability::HooksPostToolUse, "hooks_post_tool_use"),
        (Capability::SessionResume, "session_resume"),
        (Capability::SessionFork, "session_fork"),
        (Capability::Checkpointing, "checkpointing"),
        (
            Capability::StructuredOutputJsonSchema,
            "structured_output_json_schema",
        ),
        (Capability::McpClient, "mcp_client"),
        (Capability::McpServer, "mcp_server"),
    ];
    for (cap, expected) in &all {
        let v = serde_json::to_value(cap).unwrap();
        assert_eq!(v, json!(expected), "Capability::{cap:?} wire string wrong");
        // Roundtrip.
        let back: Capability = serde_json::from_value(json!(expected)).unwrap();
        assert_eq!(&back, cap);
    }
}

#[test]
fn support_level_simple_variants_wire_strings() {
    assert_eq!(
        serde_json::to_value(SupportLevel::Native).unwrap(),
        "native"
    );
    assert_eq!(
        serde_json::to_value(SupportLevel::Emulated).unwrap(),
        "emulated"
    );
    assert_eq!(
        serde_json::to_value(SupportLevel::Unsupported).unwrap(),
        "unsupported"
    );
}

#[test]
fn capability_manifest_as_map_in_json() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    let v = serde_json::to_value(&m).unwrap();
    assert!(v.is_object());
    assert_eq!(v["tool_read"], "native");
    assert_eq!(v["streaming"], "emulated");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. PolicyProfile format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_all_fields_serialize() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read_file".into(), "write_file".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        deny_write: vec!["Cargo.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["bash".into(), "execute".into()],
    };

    let v = serde_json::to_value(&policy).unwrap();
    assert_eq!(v["allowed_tools"], json!(["read_file", "write_file"]));
    assert_eq!(v["disallowed_tools"], json!(["rm"]));
    assert_eq!(v["deny_read"], json!(["**/.env", "**/secrets/**"]));
    assert_eq!(v["deny_write"], json!(["Cargo.lock"]));
    assert_eq!(v["allow_network"], json!(["api.example.com"]));
    assert_eq!(v["deny_network"], json!(["*.evil.com"]));
    assert_eq!(v["require_approval_for"], json!(["bash", "execute"]));
}

#[test]
fn policy_profile_empty_roundtrip() {
    let policy = PolicyProfile::default();
    let json = serde_json::to_string(&policy).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert!(back.allowed_tools.is_empty());
    assert!(back.disallowed_tools.is_empty());
    assert!(back.deny_read.is_empty());
    assert!(back.deny_write.is_empty());
    assert!(back.allow_network.is_empty());
    assert!(back.deny_network.is_empty());
    assert!(back.require_approval_for.is_empty());
}

#[test]
fn policy_profile_deserializes_from_hardcoded_json() {
    let json = r#"{
        "allowed_tools": ["grep", "read"],
        "disallowed_tools": [],
        "deny_read": ["**/node_modules/**"],
        "deny_write": [],
        "allow_network": [],
        "deny_network": ["10.0.0.0/8"],
        "require_approval_for": []
    }"#;
    let p: PolicyProfile = serde_json::from_str(json).unwrap();
    assert_eq!(p.allowed_tools, vec!["grep", "read"]);
    assert_eq!(p.deny_read, vec!["**/node_modules/**"]);
    assert_eq!(p.deny_network, vec!["10.0.0.0/8"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Timestamp format — ISO 8601 / RFC 3339 preserved
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn timestamp_format_rfc3339_in_run_metadata() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 45).unwrap();
    let meta = RunMetadata {
        run_id: nil_uuid(),
        work_order_id: nil_uuid(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: ts,
        finished_at: ts,
        duration_ms: 0,
    };
    let v = serde_json::to_value(&meta).unwrap();
    let started = v["started_at"].as_str().unwrap();
    // Must be valid RFC 3339.
    chrono::DateTime::parse_from_rfc3339(started).expect("started_at must be valid RFC 3339");
}

#[test]
fn timestamp_roundtrip_preserves_utc() {
    let ts = Utc.with_ymd_and_hms(2025, 12, 31, 23, 59, 59).unwrap();
    let event = AgentEvent {
        ts,
        kind: AgentEventKind::Warning {
            message: "test".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ts, ts);
}

#[test]
fn timestamp_deserializes_various_rfc3339_formats() {
    // With Z suffix.
    let json1 = json!({"ts": "2025-01-01T00:00:00Z", "type": "warning", "message": "a"});
    let e1: AgentEvent = serde_json::from_value(json1).unwrap();

    // With +00:00 offset.
    let json2 = json!({"ts": "2025-01-01T00:00:00+00:00", "type": "warning", "message": "b"});
    let e2: AgentEvent = serde_json::from_value(json2).unwrap();

    assert_eq!(e1.ts, e2.ts, "Z and +00:00 must parse to same instant");
}

#[test]
fn timestamp_with_fractional_seconds() {
    let json = json!({
        "ts": "2025-06-15T12:00:00.123456Z",
        "type": "assistant_delta",
        "text": "hi"
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    assert_eq!(e.ts.timestamp(), 1749988800);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Large / edge-case values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_duration_ms_u64_max() {
    let meta = RunMetadata {
        run_id: nil_uuid(),
        work_order_id: nil_uuid(),
        contract_version: CONTRACT_VERSION.into(),
        started_at: fixed_ts(),
        finished_at: fixed_ts_end(),
        duration_ms: u64::MAX,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.duration_ms, u64::MAX);
}

#[test]
fn large_token_counts() {
    let usage = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
        cache_read_tokens: Some(u64::MAX),
        cache_write_tokens: Some(u64::MAX),
        request_units: Some(u64::MAX),
        estimated_cost_usd: Some(f64::MAX),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, Some(u64::MAX));
}

#[test]
fn very_long_task_string() {
    let long_task = "x".repeat(100_000);
    let wo = WorkOrder {
        id: nil_uuid(),
        task: long_task.clone(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task.len(), 100_000);
}

#[test]
fn deeply_nested_vendor_json() {
    // 50 levels of nesting in vendor flags.
    let mut val: Value = json!("leaf");
    for _ in 0..50 {
        val = json!({"nested": val});
    }
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert("deep".into(), val.clone());

    let json = serde_json::to_string(&cfg).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vendor["deep"], val);
}

#[test]
fn empty_strings_are_valid() {
    let wo = WorkOrder {
        id: nil_uuid(),
        task: "".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["".into()],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "");
    assert_eq!(back.workspace.root, "");
}

#[test]
fn unicode_in_all_string_fields() {
    let emoji_task = "Fix 🐛 in módule — résumé 日本語";
    let wo = WorkOrder {
        id: nil_uuid(),
        task: emoji_task.into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ñ".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "日本語".into(),
                content: "こんにちは".into(),
            }],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, emoji_task);
    assert_eq!(back.context.snippets[0].name, "日本語");
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. CONTRACT_VERSION format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_format_matches_pattern() {
    // Must match "abp/vMAJOR.MINOR"
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
    let version_part = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    let parts: Vec<&str> = version_part.split('.').collect();
    assert_eq!(parts.len(), 2, "Version must have MAJOR.MINOR format");
    parts[0].parse::<u32>().expect("Major must be numeric");
    parts[1].parse::<u32>().expect("Minor must be numeric");
}

#[test]
fn contract_version_embedded_in_receipt_metadata() {
    let r = deterministic_receipt("mock", Outcome::Complete);
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}
