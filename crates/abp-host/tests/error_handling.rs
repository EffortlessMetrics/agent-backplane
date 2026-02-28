// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error handling and resilience tests for abp-host.
//!
//! Covers protocol violations, abnormal sidecar exits, invalid JSON,
//! missing handshake, missing final, duplicate finals, and mid-stream drops.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use abp_protocol::ProtocolError;
use std::error::Error;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "error handling test".into(),
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

fn mock_spec_with_mode(py: &str, mode: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![mock_script_path(), mode.to_string()];
    spec
}

// ---------------------------------------------------------------------------
// 1. Sidecar process exit with non-zero code
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_exit_nonzero_returns_exited_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "exit_nonzero");

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err(), "should fail when sidecar exits immediately");

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 2. Sidecar sends invalid JSON mid-stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_invalid_json_midstream_yields_protocol_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "bad_json_midstream");

    let client = SidecarClient::spawn(spec)
        .await
        .expect("hello should succeed before bad JSON");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should start");

    // Drain events; the stream should terminate when bad JSON arrives.
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(
        !events.is_empty(),
        "should receive at least one event before the bad JSON"
    );

    // The receipt should carry a protocol error.
    let receipt_result = sidecar_run.receipt.await.expect("receipt channel open");
    assert!(
        receipt_result.is_err(),
        "receipt should be an error due to invalid JSON"
    );

    let err = receipt_result.unwrap_err();
    assert!(
        matches!(err, HostError::Protocol(_)),
        "expected Protocol error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 3. Sidecar sends events without hello
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_no_hello_returns_protocol_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "no_hello");

    let result = SidecarClient::spawn(spec).await;
    assert!(
        result.is_err(),
        "spawn should fail when sidecar doesn't send hello"
    );

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Protocol(ProtocolError::UnexpectedMessage { .. })),
        "expected UnexpectedMessage protocol error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 4. Sidecar never sends final (closes after events)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_no_final_yields_exited_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "no_final");

    let client = SidecarClient::spawn(spec)
        .await
        .expect("hello should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should start");

    // Drain events.
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(
        !events.is_empty(),
        "should receive events before close"
    );

    // The receipt channel may deliver an error or be dropped.
    let receipt_result = sidecar_run.receipt.await;
    match receipt_result {
        Ok(Err(HostError::Exited { .. })) => { /* expected: process exited without final */ }
        Ok(Err(other)) => {
            // Some protocol-level error is also acceptable.
            let _ = other;
        }
        Err(_recv_err) => {
            // Receipt sender was dropped without sending — acceptable.
        }
        Ok(Ok(_receipt)) => {
            panic!("should not receive a valid receipt when sidecar never sends final");
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Sidecar sends multiple finals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_multi_final_first_wins() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_final");

    let client = SidecarClient::spawn(spec)
        .await
        .expect("hello should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should start");

    // Drain events.
    let _events: Vec<_> = sidecar_run.events.collect().await;

    // The host should accept the first final and ignore (or tolerate) the second.
    let receipt_result = sidecar_run.receipt.await.expect("receipt channel open");
    assert!(
        receipt_result.is_ok(),
        "first final should produce a valid receipt, got: {:?}",
        receipt_result.err()
    );

    let receipt = receipt_result.unwrap();
    assert_eq!(receipt.backend.id, "mock-test");
}

// ---------------------------------------------------------------------------
// 6. Connection drop mid-stream (sidecar exits abruptly)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_drop_midstream_handled_gracefully() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "drop_midstream");

    let client = SidecarClient::spawn(spec)
        .await
        .expect("hello should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should start");

    // Drain events — may get some before the drop.
    let events: Vec<_> = sidecar_run.events.collect().await;
    // We should receive at least the one event sent before drop.
    assert!(
        !events.is_empty(),
        "should receive at least one event before connection drop"
    );

    // Receipt should be an error since the process died.
    let receipt_result = sidecar_run.receipt.await;
    match receipt_result {
        Ok(Err(HostError::Exited { .. })) => { /* expected */ }
        Ok(Err(other)) => {
            // Any error is acceptable when the process drops.
            let _ = other;
        }
        Err(_recv_err) => {
            // Sender dropped — acceptable.
        }
        Ok(Ok(_)) => {
            panic!("should not get a valid receipt when sidecar drops mid-stream");
        }
    }
}

// ---------------------------------------------------------------------------
// 7. Sidecar sends fatal error envelope
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sidecar_fatal_error_propagated() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "fatal");

    let client = SidecarClient::spawn(spec)
        .await
        .expect("hello should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should start");

    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt_result = sidecar_run.receipt.await.expect("receipt channel open");
    assert!(
        receipt_result.is_err(),
        "fatal envelope should produce an error"
    );

    let err = receipt_result.unwrap_err();
    assert!(
        matches!(err, HostError::Fatal(_)),
        "expected Fatal error, got: {err}"
    );
    assert!(
        err.to_string().contains("something went wrong"),
        "error should contain the fatal message: {}",
        err
    );
}

// ---------------------------------------------------------------------------
// 8. HostError variants are all constructable and displayable
// ---------------------------------------------------------------------------

#[test]
fn host_error_all_variants_constructable_and_displayable() {
    let variants: Vec<HostError> = vec![
        HostError::Spawn(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        )),
        HostError::Stdout(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "broken pipe",
        )),
        HostError::Stdin(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "broken pipe",
        )),
        HostError::Protocol(ProtocolError::Violation("test violation".into())),
        HostError::Violation("test protocol violation".into()),
        HostError::Fatal("fatal error message".into()),
        HostError::Exited { code: Some(1) },
        HostError::Exited { code: None },
    ];

    for variant in &variants {
        let display = variant.to_string();
        let debug = format!("{variant:?}");
        assert!(!display.is_empty(), "Display must be non-empty for {debug}");
        assert!(!debug.is_empty(), "Debug must be non-empty");
    }
}

// ---------------------------------------------------------------------------
// 9. HostError source chains
// ---------------------------------------------------------------------------

#[test]
fn host_error_source_chains() {
    // Spawn wraps io::Error
    let err = HostError::Spawn(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Spawn should have a source");

    // Stdout wraps io::Error
    let err = HostError::Stdout(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Stdout should have a source");

    // Stdin wraps io::Error
    let err = HostError::Stdin(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Stdin should have a source");

    // Protocol wraps ProtocolError
    let err = HostError::Protocol(ProtocolError::Violation("v".into()));
    assert!(err.source().is_some(), "Protocol should have a source");

    // Violation is a plain string — no source
    let err = HostError::Violation("v".into());
    assert!(err.source().is_none(), "Violation should not have a source");

    // Fatal is a plain string — no source
    let err = HostError::Fatal("f".into());
    assert!(err.source().is_none(), "Fatal should not have a source");

    // Exited has no inner error
    let err = HostError::Exited { code: Some(1) };
    assert!(err.source().is_none(), "Exited should not have a source");
}

// ---------------------------------------------------------------------------
// 10. HostError Protocol variant converts from ProtocolError
// ---------------------------------------------------------------------------

#[test]
fn host_error_from_protocol_error() {
    let proto_err = ProtocolError::Violation("test".into());
    let host_err: HostError = proto_err.into();
    assert!(
        matches!(host_err, HostError::Protocol(_)),
        "From<ProtocolError> should produce Protocol variant"
    );
}

// ---------------------------------------------------------------------------
// 11. Spawn failure doesn't prevent subsequent spawn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spawn_failure_then_success() {
    let py = require_python!();

    // First: fail to spawn.
    let bad_spec = SidecarSpec::new("nonexistent-binary-error-handling-test");
    let result = SidecarClient::spawn(bad_spec).await;
    assert!(result.is_err());

    // Second: succeed with valid spec.
    let spec = mock_spec_with_mode(&py, "default");
    let client = SidecarClient::spawn(spec)
        .await
        .expect("should succeed after prior failure");
    assert_eq!(client.hello.backend.id, "mock-test");
}
