// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol conformance tests for abp-host sidecar protocol.
//!
//! Validates JSONL protocol behaviour by spawning mock sidecars and
//! verifying handshake, event streaming, receipt delivery, and error handling.

use abp_core::{
    CONTRACT_VERSION, CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile,
    RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "conformance test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn mock_script_path() -> String {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests")
        .join("mock_sidecar.py")
        .to_string_lossy()
        .into_owned()
}

fn python_cmd() -> Option<String> {
    for cmd in &["python3", "python"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(cmd.to_string());
        }
    }
    None
}

macro_rules! require_python {
    () => {
        match python_cmd() {
            Some(cmd) => cmd,
            None => {
                eprintln!("SKIP: python not found");
                return;
            }
        }
    };
}

fn mock_spec(py: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![mock_script_path()];
    spec
}

// ---------------------------------------------------------------------------
// 1. Hello handshake completes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_hello_handshake_completes() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(client.hello.backend.backend_version.as_deref(), Some("0.1"));
    assert_eq!(client.hello.backend.adapter_version.as_deref(), Some("0.1"));
}

// ---------------------------------------------------------------------------
// 2. Run envelope sent correctly (events are emitted)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_run_envelope_sent_correctly() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(
        !events.is_empty(),
        "sidecar should emit at least one event after receiving run"
    );

    // Clean up.
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ---------------------------------------------------------------------------
// 3. Event streaming works (correct ref_id)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_event_streaming_works() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    // The mock sidecar emits a run_started event; verify it arrives.
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(
        events
            .iter()
            .any(|e| { matches!(&e.kind, abp_core::AgentEventKind::RunStarted { .. }) }),
        "expected a RunStarted event, got: {events:?}"
    );

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ---------------------------------------------------------------------------
// 4. Final receipt received
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_final_receipt_received() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    // Drain events so the background task reaches the final envelope.
    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert_eq!(receipt.backend.id, "mock-test");
    assert!(
        matches!(receipt.outcome, abp_core::Outcome::Complete),
        "expected Complete outcome, got: {:?}",
        receipt.outcome
    );
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);

    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 5. Protocol version matches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_protocol_version_matches() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    assert_eq!(
        client.hello.contract_version, CONTRACT_VERSION,
        "hello contract_version should match CONTRACT_VERSION"
    );
}

// ---------------------------------------------------------------------------
// 6. Missing hello causes error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_missing_hello_causes_error() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "import sys; sys.exit(0)".into()];

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err(), "spawn should fail when hello is missing");

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 7. Invalid JSON causes error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conformance_invalid_json_causes_error() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "print('not json')".into()];

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err(), "spawn should fail on invalid JSON");

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Protocol(_)),
        "expected Protocol error, got: {err}"
    );
}
