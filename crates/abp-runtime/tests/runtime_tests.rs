#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::WorkOrder;
use abp_runtime::{RunHandle, Runtime};
use tokio_stream::StreamExt;

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::nil(),
        task: "test".into(),
        lane: abp_core::ExecutionLane::PatchFirst,
        workspace: abp_core::WorkspaceSpec {
            root: ".".into(),
            mode: abp_core::WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: abp_core::PolicyProfile::default(),
        requirements: abp_core::CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

#[test]
fn new_has_no_backends() {
    let rt = Runtime::new();
    assert!(rt.backend_names().is_empty());
}

#[test]
fn with_default_backends_includes_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn register_backend_adds_to_list() {
    let mut rt = Runtime::new();
    assert!(!rt.backend_names().contains(&"mock".to_string()));
    rt.register_backend("mock", abp_integrations::MockBackend);
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn backend_returns_none_for_unknown() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("nonexistent").is_none());
}

#[test]
fn backend_returns_some_for_registered() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend("mock").is_some());
}

#[tokio::test]
async fn run_streaming_with_mock_backend() {
    let rt = Runtime::with_default_backends();
    let wo = test_work_order();

    let RunHandle {
        run_id: _,
        mut events,
        receipt,
    } = rt.run_streaming("mock", wo).await.expect("run_streaming");

    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    assert!(!collected.is_empty(), "should receive at least one event");

    let receipt = receipt.await.expect("join").expect("receipt");
    assert!(
        receipt.receipt_sha256.is_some(),
        "receipt should have a hash"
    );
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn run_streaming_with_unknown_backend_fails() {
    let rt = Runtime::with_default_backends();
    let wo = test_work_order();

    let result = rt.run_streaming("nonexistent", wo).await;
    let msg = match result {
        Err(e) => format!("{e}"),
        Ok(_) => panic!("should fail for unknown backend"),
    };
    assert!(
        msg.contains("nonexistent"),
        "error should mention the backend name: {msg}"
    );
}

#[tokio::test]
async fn run_handle_events_stream_completes() {
    let rt = Runtime::with_default_backends();
    let wo = test_work_order();

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    // Collect all events — stream must terminate.
    let all: Vec<_> = handle.events.collect().await;
    assert!(!all.is_empty());

    // Receipt must also resolve.
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn receipt_has_verification_fields() {
    let rt = Runtime::with_default_backends();
    let wo = test_work_order();

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    // Drain events so the run can complete.
    let _: Vec<_> = handle.events.collect().await;

    let receipt = handle.receipt.await.expect("join").expect("receipt");
    // In PassThrough mode the runtime still fills verification fields.
    // git_diff and git_status may be None or Some depending on workspace,
    // but the verification struct itself must be present.
    let _v = &receipt.verification;
    // harness_ok is set by the mock backend.
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn multiple_runs_produce_different_run_ids() {
    let rt = Runtime::with_default_backends();

    let h1 = rt
        .run_streaming("mock", test_work_order())
        .await
        .expect("run 1");
    let h2 = rt
        .run_streaming("mock", test_work_order())
        .await
        .expect("run 2");

    assert_ne!(h1.run_id, h2.run_id, "run_ids must differ");

    // Clean up: drain events and receipts.
    let _: Vec<_> = h1.events.collect().await;
    let _: Vec<_> = h2.events.collect().await;
    let _ = h1.receipt.await;
    let _ = h2.receipt.await;
}
