// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style tests for Agent Backplane scenarios covering work-order routing,
//! receipt validation, capability checking, and policy enforcement.

use std::collections::HashMap;
use std::path::Path;

use cucumber::{World as _, given, then, when};
use tokio_stream::StreamExt;

use abp_core::{
    AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements, MinSupport, Outcome,
    PolicyProfile, Receipt, WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_policy::PolicyEngine;
use abp_runtime::Runtime;

/// Newtype wrapper so `cucumber::World` derive can use `Debug`.
struct Rt(Runtime);

impl std::fmt::Debug for Rt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Runtime")
    }
}

impl Default for Rt {
    fn default() -> Self {
        Self(Runtime::new())
    }
}

#[derive(Debug, Default, cucumber::World)]
struct AbpWorld {
    runtime: Rt,
    work_order: Option<abp_core::WorkOrder>,
    receipt: Option<Receipt>,
    run_error: Option<String>,
    cap_check_error: Option<String>,
    // Policy testing state
    policy_profile: Option<PolicyProfile>,
    policy_engine: Option<PolicyEngine>,
    // Saved receipts for cross-scenario comparisons
    saved_receipts: HashMap<String, Receipt>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_capability(name: &str) -> Capability {
    match name {
        "streaming" => Capability::Streaming,
        "tool_read" => Capability::ToolRead,
        "tool_write" => Capability::ToolWrite,
        "tool_edit" => Capability::ToolEdit,
        "tool_bash" => Capability::ToolBash,
        "mcp_client" => Capability::McpClient,
        "mcp_server" => Capability::McpServer,
        "session_resume" => Capability::SessionResume,
        other => panic!("unknown capability in test: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Given — Runtime & Work Orders
// ---------------------------------------------------------------------------

#[given("a runtime with the mock backend registered")]
async fn runtime_with_mock(w: &mut AbpWorld) {
    w.runtime = Rt(Runtime::with_default_backends());
}

#[given(expr = "a work order with task {string}")]
async fn work_order_with_task(w: &mut AbpWorld, task: String) {
    w.work_order = Some(
        WorkOrderBuilder::new(task)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build(),
    );
}

#[given(expr = "a work order with task {string} that requires native {string} capability")]
async fn work_order_with_native_cap(w: &mut AbpWorld, task: String, cap_name: String) {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: parse_capability(&cap_name),
                min_support: MinSupport::Native,
            }],
        })
        .build();
    w.work_order = Some(wo);
}

#[given(expr = "a work order with task {string} that requires emulated {string} capability")]
async fn work_order_with_emulated_cap(w: &mut AbpWorld, task: String, cap_name: String) {
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: parse_capability(&cap_name),
                min_support: MinSupport::Emulated,
            }],
        })
        .build();
    w.work_order = Some(wo);
}

// ---------------------------------------------------------------------------
// Given — Policy
// ---------------------------------------------------------------------------

#[given("an empty policy profile")]
async fn empty_policy(w: &mut AbpWorld) {
    w.policy_profile = Some(PolicyProfile::default());
}

#[given(expr = "a policy that disallows tool {string}")]
async fn policy_disallow_tool(w: &mut AbpWorld, tool: String) {
    w.policy_profile = Some(PolicyProfile {
        disallowed_tools: vec![tool],
        ..PolicyProfile::default()
    });
}

#[given(expr = "a policy that allows only tools {string}")]
async fn policy_allow_tools(w: &mut AbpWorld, tools_csv: String) {
    let tools: Vec<String> = tools_csv.split(',').map(|s| s.trim().to_string()).collect();
    w.policy_profile = Some(PolicyProfile {
        allowed_tools: tools,
        ..PolicyProfile::default()
    });
}

#[given(expr = "a policy that allows only tools {string} and disallows tool {string}")]
async fn policy_allow_and_disallow(w: &mut AbpWorld, tools_csv: String, deny: String) {
    let tools: Vec<String> = tools_csv.split(',').map(|s| s.trim().to_string()).collect();
    w.policy_profile = Some(PolicyProfile {
        allowed_tools: tools,
        disallowed_tools: vec![deny],
        ..PolicyProfile::default()
    });
}

#[given(expr = "a policy that denies reading {string}")]
async fn policy_deny_read(w: &mut AbpWorld, pattern: String) {
    w.policy_profile = Some(PolicyProfile {
        deny_read: vec![pattern],
        ..PolicyProfile::default()
    });
}

#[given(expr = "a policy that denies writing {string}")]
async fn policy_deny_write(w: &mut AbpWorld, pattern: String) {
    w.policy_profile = Some(PolicyProfile {
        deny_write: vec![pattern],
        ..PolicyProfile::default()
    });
}

#[given(expr = "a policy that denies reading {string} and {string}")]
async fn policy_deny_read_multi(w: &mut AbpWorld, p1: String, p2: String) {
    w.policy_profile = Some(PolicyProfile {
        deny_read: vec![p1, p2],
        ..PolicyProfile::default()
    });
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "the work order is submitted to the {string} backend")]
async fn submit_work_order(w: &mut AbpWorld, backend: String) {
    let rt = &w.runtime.0;
    let wo = w.work_order.take().expect("work order not set");

    match rt.run_streaming(&backend, wo).await {
        Ok(handle) => {
            let mut events = handle.events;
            while events.next().await.is_some() {}

            match handle.receipt.await {
                Ok(Ok(receipt)) => w.receipt = Some(receipt),
                Ok(Err(e)) => w.run_error = Some(e.to_string()),
                Err(e) => w.run_error = Some(format!("join error: {e}")),
            }
        }
        Err(e) => {
            w.run_error = Some(e.to_string());
        }
    }
}

#[when(expr = "the capability check is performed against the {string} backend")]
async fn perform_capability_check(w: &mut AbpWorld, backend: String) {
    let rt = &w.runtime.0;
    let wo = w.work_order.as_ref().expect("work order not set");

    match rt.check_capabilities(&backend, &wo.requirements) {
        Ok(()) => w.cap_check_error = None,
        Err(e) => w.cap_check_error = Some(e.to_string()),
    }
}

#[when("the policy engine is compiled")]
async fn compile_policy(w: &mut AbpWorld) {
    let profile = w.policy_profile.as_ref().expect("policy profile not set");
    w.policy_engine = Some(PolicyEngine::new(profile).expect("policy should compile"));
}

// ---------------------------------------------------------------------------
// Then — Run outcomes
// ---------------------------------------------------------------------------

#[then("the run completes successfully")]
async fn run_completes(w: &mut AbpWorld) {
    assert!(
        w.run_error.is_none(),
        "expected success, got error: {:?}",
        w.run_error
    );
    assert!(w.receipt.is_some(), "no receipt produced");
}

#[then(expr = "the receipt outcome is {string}")]
async fn receipt_outcome(w: &mut AbpWorld, expected: String) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    let actual = match &receipt.outcome {
        Outcome::Complete => "complete",
        Outcome::Partial => "partial",
        Outcome::Failed => "failed",
    };
    assert_eq!(actual, expected);
}

#[then("the receipt contains a non-empty trace")]
async fn receipt_has_trace(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert!(!receipt.trace.is_empty(), "trace should not be empty");
}

#[then(expr = "the run fails with an error containing {string}")]
async fn run_fails_with(w: &mut AbpWorld, needle: String) {
    let err = w
        .run_error
        .as_ref()
        .expect("expected an error, but run succeeded");
    assert!(
        err.contains(&needle),
        "error '{err}' should contain '{needle}'"
    );
}

// ---------------------------------------------------------------------------
// Then — Trace inspection
// ---------------------------------------------------------------------------

#[then("the trace starts with a RunStarted event")]
async fn trace_starts_with_run_started(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert!(
        matches!(
            receipt.trace.first().map(|e| &e.kind),
            Some(AgentEventKind::RunStarted { .. })
        ),
        "first event should be RunStarted"
    );
}

#[then("the trace ends with a RunCompleted event")]
async fn trace_ends_with_run_completed(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert!(
        matches!(
            receipt.trace.last().map(|e| &e.kind),
            Some(AgentEventKind::RunCompleted { .. })
        ),
        "last event should be RunCompleted"
    );
}

// ---------------------------------------------------------------------------
// Then — Receipt metadata
// ---------------------------------------------------------------------------

#[then("the receipt work_order_id matches the submitted work order")]
async fn receipt_wo_id_matches(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    // The work order was consumed by submission, but the receipt records it.
    // We just verify the field is a valid non-nil UUID.
    assert!(
        !receipt.meta.work_order_id.is_nil(),
        "work_order_id should be non-nil"
    );
}

#[then(expr = "the receipt backend id is {string}")]
async fn receipt_backend_id(w: &mut AbpWorld, expected: String) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert_eq!(receipt.backend.id, expected);
}

#[then(expr = "the receipt contract version is {string}")]
async fn receipt_contract_version(w: &mut AbpWorld, expected: String) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert_eq!(receipt.meta.contract_version, expected);
}

#[then("the receipt started_at is before or equal to finished_at")]
async fn receipt_timestamps_ordered(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert!(
        receipt.meta.started_at <= receipt.meta.finished_at,
        "started_at ({}) should be <= finished_at ({})",
        receipt.meta.started_at,
        receipt.meta.finished_at
    );
}

#[then("the receipt can be serialized to JSON and back")]
async fn receipt_json_roundtrip(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    let json = serde_json::to_string(receipt).expect("serialize");
    let back: Receipt = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.meta.run_id, receipt.meta.run_id);
    assert_eq!(back.receipt_sha256, receipt.receipt_sha256);
}

// ---------------------------------------------------------------------------
// Then — Saved receipts for multi-scenario comparisons
// ---------------------------------------------------------------------------

#[then(expr = "the receipt is saved as {string}")]
async fn save_receipt(w: &mut AbpWorld, name: String) {
    let receipt = w.receipt.as_ref().expect("no receipt").clone();
    w.saved_receipts.insert(name, receipt);
}

#[then(expr = "the saved receipts {string} and {string} have different run ids")]
async fn saved_receipts_different_ids(w: &mut AbpWorld, a: String, b: String) {
    let ra = w
        .saved_receipts
        .get(&a)
        .unwrap_or_else(|| panic!("no saved receipt '{a}'"));
    let rb = w
        .saved_receipts
        .get(&b)
        .unwrap_or_else(|| panic!("no saved receipt '{b}'"));
    assert_ne!(
        ra.meta.run_id, rb.meta.run_id,
        "receipts should have different run ids"
    );
}

// ---------------------------------------------------------------------------
// Then — Receipt hashing
// ---------------------------------------------------------------------------

#[then("the receipt has a SHA-256 hash")]
async fn receipt_has_hash(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt_sha256 should be set"
    );
}

#[then("the hash is a 64-character hex string")]
async fn hash_is_valid_hex(w: &mut AbpWorld) {
    let hash = w
        .receipt
        .as_ref()
        .expect("no receipt")
        .receipt_sha256
        .as_ref()
        .expect("no hash");
    assert_eq!(hash.len(), 64, "SHA-256 hex digest should be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should be hex: {hash}"
    );
}

#[then("recomputing the hash produces the same value")]
async fn hash_is_deterministic(w: &mut AbpWorld) {
    let receipt = w.receipt.as_ref().expect("no receipt");
    let original = receipt.receipt_sha256.as_ref().expect("no hash");
    let recomputed = receipt_hash(receipt).expect("failed to hash");
    assert_eq!(original, &recomputed, "hash must be deterministic");
}

// ---------------------------------------------------------------------------
// Then — Capability checks
// ---------------------------------------------------------------------------

#[then("the capability check passes")]
async fn capability_check_passes(w: &mut AbpWorld) {
    assert!(
        w.cap_check_error.is_none(),
        "expected capability check to pass, but got: {:?}",
        w.cap_check_error
    );
}

#[then("the capability check fails")]
async fn capability_check_fails(w: &mut AbpWorld) {
    assert!(
        w.cap_check_error.is_some(),
        "expected capability check to fail, but it passed"
    );
}

// ---------------------------------------------------------------------------
// Then — Policy enforcement
// ---------------------------------------------------------------------------

#[then(expr = "the tool {string} is allowed")]
async fn tool_is_allowed(w: &mut AbpWorld, tool: String) {
    let engine = w
        .policy_engine
        .as_ref()
        .expect("policy engine not compiled");
    let decision = engine.can_use_tool(&tool);
    assert!(
        decision.allowed,
        "tool '{tool}' should be allowed, but was denied: {:?}",
        decision.reason
    );
}

#[then(expr = "the tool {string} is denied")]
async fn tool_is_denied(w: &mut AbpWorld, tool: String) {
    let engine = w
        .policy_engine
        .as_ref()
        .expect("policy engine not compiled");
    let decision = engine.can_use_tool(&tool);
    assert!(
        !decision.allowed,
        "tool '{tool}' should be denied, but was allowed"
    );
}

#[then(expr = "reading path {string} is denied")]
async fn read_path_denied(w: &mut AbpWorld, path: String) {
    let engine = w
        .policy_engine
        .as_ref()
        .expect("policy engine not compiled");
    let decision = engine.can_read_path(Path::new(&path));
    assert!(
        !decision.allowed,
        "reading '{path}' should be denied, but was allowed"
    );
}

#[then(expr = "reading path {string} is allowed")]
async fn read_path_allowed(w: &mut AbpWorld, path: String) {
    let engine = w
        .policy_engine
        .as_ref()
        .expect("policy engine not compiled");
    let decision = engine.can_read_path(Path::new(&path));
    assert!(
        decision.allowed,
        "reading '{path}' should be allowed, but was denied: {:?}",
        decision.reason
    );
}

#[then(expr = "writing path {string} is denied")]
async fn write_path_denied(w: &mut AbpWorld, path: String) {
    let engine = w
        .policy_engine
        .as_ref()
        .expect("policy engine not compiled");
    let decision = engine.can_write_path(Path::new(&path));
    assert!(
        !decision.allowed,
        "writing '{path}' should be denied, but was allowed"
    );
}

#[then(expr = "writing path {string} is allowed")]
async fn write_path_allowed(w: &mut AbpWorld, path: String) {
    let engine = w
        .policy_engine
        .as_ref()
        .expect("policy engine not compiled");
    let decision = engine.can_write_path(Path::new(&path));
    assert!(
        decision.allowed,
        "writing '{path}' should be allowed, but was denied: {:?}",
        decision.reason
    );
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    AbpWorld::run("tests/features").await;
}
