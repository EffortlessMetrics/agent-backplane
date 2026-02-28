// SPDX-License-Identifier: MIT OR Apache-2.0
//! Negative and edge-case tests for abp-core contract types.

use abp_core::*;
use chrono::{TimeZone, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────

fn minimal_receipt() -> Receipt {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!(null),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ── 1. WorkOrder with empty task string ─────────────────────────────

#[test]
fn work_order_empty_task_round_trips() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "");
}

// ── 2. WorkOrder with very long task string (10 KB) ─────────────────

#[test]
fn work_order_long_task_round_trips() {
    let long_task = "x".repeat(10 * 1024);
    let wo = WorkOrderBuilder::new(&long_task).build();
    assert_eq!(wo.task.len(), 10 * 1024);
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, long_task);
}

// ── 3. Receipt with all optional fields as None ─────────────────────

#[test]
fn receipt_all_optional_none_round_trips() {
    let r = minimal_receipt();
    // Verify all optional fields are None
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.cache_read_tokens.is_none());
    assert!(r.usage.cache_write_tokens.is_none());
    assert!(r.usage.request_units.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
    assert!(r.receipt_sha256.is_none());

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(r2.receipt_sha256.is_none());
    assert!(r2.usage.input_tokens.is_none());
}

// ── 4. CapabilityRequirements with empty required list ──────────────

#[test]
fn empty_capability_requirements_round_trips() {
    let reqs = CapabilityRequirements { required: vec![] };
    let json = serde_json::to_string(&reqs).unwrap();
    let reqs2: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert!(reqs2.required.is_empty());
}

#[test]
fn empty_capability_requirements_via_default() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

// ── 5. Deserialize WorkOrder from invalid JSON (should error) ───────

#[test]
fn work_order_from_invalid_json_errors() {
    let result = serde_json::from_str::<WorkOrder>("not json at all");
    assert!(result.is_err());
}

#[test]
fn work_order_from_empty_string_errors() {
    let result = serde_json::from_str::<WorkOrder>("");
    assert!(result.is_err());
}

#[test]
fn work_order_from_truncated_json_errors() {
    let wo = WorkOrderBuilder::new("test").build();
    let json = serde_json::to_string(&wo).unwrap();
    let truncated = &json[..json.len() / 2];
    let result = serde_json::from_str::<WorkOrder>(truncated);
    assert!(result.is_err());
}

// ── 6. Deserialize WorkOrder with missing required fields ───────────

#[test]
fn work_order_missing_task_field_errors() {
    let json = json!({
        "id": "00000000-0000-0000-0000-000000000000",
        "lane": "patch_first",
        "workspace": { "root": ".", "mode": "staged", "include": [], "exclude": [] },
        "context": { "files": [], "snippets": [] },
        "policy": {
            "allowed_tools": [], "disallowed_tools": [],
            "deny_read": [], "deny_write": [],
            "allow_network": [], "deny_network": [],
            "require_approval_for": []
        },
        "requirements": { "required": [] },
        "config": { "model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null }
    });
    let result = serde_json::from_value::<WorkOrder>(json);
    assert!(result.is_err());
}

#[test]
fn work_order_missing_id_field_errors() {
    let json = json!({
        "task": "hello",
        "lane": "patch_first",
        "workspace": { "root": ".", "mode": "staged", "include": [], "exclude": [] },
        "context": { "files": [], "snippets": [] },
        "policy": {
            "allowed_tools": [], "disallowed_tools": [],
            "deny_read": [], "deny_write": [],
            "allow_network": [], "deny_network": [],
            "require_approval_for": []
        },
        "requirements": { "required": [] },
        "config": { "model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null }
    });
    let result = serde_json::from_value::<WorkOrder>(json);
    assert!(result.is_err());
}

// ── 7. Deserialize WorkOrder with unknown fields (should succeed) ───

#[test]
fn work_order_with_unknown_fields_succeeds() {
    let wo = WorkOrderBuilder::new("test").build();
    let mut val = serde_json::to_value(&wo).unwrap();
    // Inject unknown fields
    val.as_object_mut()
        .unwrap()
        .insert("totally_unknown".into(), json!("surprise"));
    val.as_object_mut()
        .unwrap()
        .insert("extra_number".into(), json!(42));
    let result = serde_json::from_value::<WorkOrder>(val);
    assert!(result.is_ok());
}

// ── 8. receipt_hash on receipt with pre-existing hash ────────────────

#[test]
fn receipt_hash_clears_preexisting_hash() {
    let r = minimal_receipt();
    // Compute fresh hash
    let hash1 = receipt_hash(&r).unwrap();

    // Now set a bogus pre-existing hash and recompute
    let mut r2 = r;
    r2.receipt_sha256 = Some("bogus_hash_value".into());
    let hash2 = receipt_hash(&r2).unwrap();

    // Hash must be the same regardless of pre-existing receipt_sha256
    assert_eq!(hash1, hash2);
}

#[test]
fn with_hash_overwrites_preexisting_hash() {
    let mut r = minimal_receipt();
    r.receipt_sha256 = Some("old_stale_hash".into());
    let r = r.with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_ne!(r.receipt_sha256.as_deref().unwrap(), "old_stale_hash");
}

#[test]
fn with_hash_idempotent() {
    let r = minimal_receipt();
    let r1 = r.clone().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

// ── 9. UUID edge cases (nil UUID) ───────────────────────────────────

#[test]
fn nil_uuid_work_order_round_trips() {
    let wo = WorkOrder {
        id: Uuid::nil(),
        task: "nil uuid task".into(),
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
    assert_eq!(wo.id, Uuid::nil());
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("00000000-0000-0000-0000-000000000000"));
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.id, Uuid::nil());
}

#[test]
fn nil_uuid_in_receipt_metadata() {
    let r = minimal_receipt();
    assert_eq!(r.meta.run_id, Uuid::nil());
    assert_eq!(r.meta.work_order_id, Uuid::nil());
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.meta.run_id, Uuid::nil());
    assert_eq!(r2.meta.work_order_id, Uuid::nil());
}
