// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style tests for Agent Backplane work order scenarios.

use cucumber::{World as _, given, then, when};
use tokio_stream::StreamExt;

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, MinSupport, Outcome, Receipt,
    WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
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
}

// ---------------------------------------------------------------------------
// Given
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
async fn work_order_with_unsatisfiable_cap(w: &mut AbpWorld, task: String, cap_name: String) {
    let capability = match cap_name.as_str() {
        "mcp_client" => Capability::McpClient,
        "mcp_server" => Capability::McpServer,
        "streaming" => Capability::Streaming,
        "session_resume" => Capability::SessionResume,
        other => panic!("unknown capability in test: {other}"),
    };
    let wo = WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    w.work_order = Some(wo);
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
            // Drain the event stream so the run can complete.
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

// ---------------------------------------------------------------------------
// Then
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

#[then("the capability check fails")]
async fn capability_check_fails(w: &mut AbpWorld) {
    assert!(
        w.cap_check_error.is_some(),
        "expected capability check to fail, but it passed"
    );
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    AbpWorld::run("tests/features").await;
}
