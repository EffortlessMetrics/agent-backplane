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
//! Comprehensive sidecar lifecycle tests for abp-host.
//!
//! Covers handshake, event streaming, completion, error handling, and edge
//! cases for the full sidecar process lifecycle managed by `SidecarClient`.

use abp_core::{
    AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, CapabilityRequirements,
    ContextPacket, ExecutionLane, Outcome, PolicyProfile, RuntimeConfig, WorkOrder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use std::error::Error;
use std::time::Duration;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "lifecycle test".into(),
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
    mock_spec_with_mode(py, "default")
}

fn mock_spec_with_mode(py: &str, mode: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![mock_script_path(), mode.to_string()];
    spec
}

fn hosts_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("hosts")
}

// ===========================================================================
// SECTION 1: HANDSHAKE TESTS (5 tests)
// ===========================================================================

// ---------------------------------------------------------------------------
// 1.1 Valid hello envelope accepted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handshake_valid_hello_accepted() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed with valid hello");

    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
    assert!(client.hello.backend.backend_version.is_some());
}

// ---------------------------------------------------------------------------
// 1.2 Missing hello fails with ProtocolError
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handshake_missing_hello_fails_with_protocol_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "no_hello");

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err(), "spawn should fail without hello");

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            HostError::Protocol(ProtocolError::UnexpectedMessage { .. })
        ),
        "expected UnexpectedMessage protocol error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 1.3 Wrong contract version in hello
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handshake_wrong_contract_version_still_accepted() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "wrong_version"))
        .await
        .expect("spawn should succeed even with wrong version");

    // The client stores whatever version the sidecar reported; version
    // negotiation/rejection is a higher-level concern.
    assert_eq!(client.hello.contract_version, "abp/v999.0");
    assert_eq!(client.hello.backend.id, "mock-test");
}

// ---------------------------------------------------------------------------
// 1.4 Hello with extra fields accepted (forward compat)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handshake_extra_fields_forward_compatible() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "hello_extra_fields"))
        .await
        .expect("spawn should succeed with extra hello fields");

    assert_eq!(client.hello.backend.id, "mock-test");

    // Verify we can still run normally after extra-field hello.
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should succeed");
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());
    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel open")
        .expect("receipt Ok");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 1.5 Hello timeout handling — sidecar that never sends hello
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handshake_hello_timeout_via_process_exit() {
    // Instead of actually waiting for a long timeout, test that a sidecar
    // which exits immediately without sending hello produces the right error.
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "exit_nonzero");

    let result = SidecarClient::spawn(spec).await;
    assert!(
        result.is_err(),
        "should fail when sidecar exits before hello"
    );

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited error, got: {err}"
    );
}

// ===========================================================================
// SECTION 2: EVENT STREAMING TESTS (8 tests)
// ===========================================================================

// ---------------------------------------------------------------------------
// 2.1 Single event received correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_single_event_received() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 1, "default mode sends exactly one event");
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunStarted { message } if message == "mock test started"
    ));

    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2.2 Multiple events in sequence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_multiple_in_sequence() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "multi_events"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5, "multi_events mode sends 5 events");

    // Verify ordering by message content.
    for (i, event) in events.iter().enumerate() {
        if let AgentEventKind::RunStarted { message } = &event.kind {
            assert_eq!(message, &format!("event {i}"));
        }
    }

    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2.3 Events with different kinds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_different_kinds() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "multi_event_kinds"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5, "multi_event_kinds sends 5 varied events");

    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::AssistantMessage { text } if text == "Hello world"
    ));
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::FileChanged { path, .. } if path == "test.txt"
    ));
    assert!(matches!(
        &events[4].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2.4 Malformed event line causes protocol error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_malformed_line_errors() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "bad_json_midstream"))
        .await
        .expect("hello should succeed before bad JSON");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(
        !events.is_empty(),
        "should get at least one event before the bad JSON"
    );

    let receipt_result = sidecar_run.receipt.await.expect("channel");
    assert!(receipt_result.is_err(), "bad JSON should cause error");
    assert!(matches!(
        receipt_result.unwrap_err(),
        HostError::Protocol(_)
    ));
}

// ---------------------------------------------------------------------------
// 2.5 Empty event lines ignored
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_empty_lines_ignored() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "empty_lines"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    // Should get exactly 2 events, empty lines ignored.
    assert_eq!(events.len(), 2, "empty lines should be silently skipped");

    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2.6 Very large event payload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_large_payload() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "large_payload"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 1);

    if let AgentEventKind::AssistantMessage { text } = &events[0].kind {
        assert_eq!(text.len(), 100_000, "should receive full 100KB payload");
        assert!(text.chars().all(|c| c == 'A'));
    } else {
        panic!("expected AssistantMessage, got: {:?}", events[0].kind);
    }

    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2.7 Unicode in event content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_unicode_content() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "unicode_content"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 2);

    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("你好世界"), "missing Chinese characters");
        assert!(message.contains("🌍"), "missing globe emoji");
        assert!(message.contains("こんにちは"), "missing Japanese");
        assert!(message.contains("мир"), "missing Cyrillic");
    } else {
        panic!("expected RunStarted, got: {:?}", events[0].kind);
    }

    if let AgentEventKind::AssistantMessage { text } = &events[1].kind {
        assert!(text.contains("🚀"), "missing rocket emoji");
        assert!(text.contains("∑"), "missing math symbols");
    } else {
        panic!("expected AssistantMessage, got: {:?}", events[1].kind);
    }

    sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2.8 Event ref_id correlation — mismatched ref_ids are dropped
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_ref_id_correlation() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "wrong_ref_id"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    // The mock sends 3 events: correct ref, wrong ref, correct ref.
    // The wrong ref_id event should be silently dropped.
    assert_eq!(events.len(), 2, "event with wrong ref_id should be dropped");

    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunStarted { message } if message == "correct ref"
    ));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::RunCompleted { message } if message == "correct again"
    ));

    sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    sidecar_run.wait.await.unwrap().unwrap();
}

// ===========================================================================
// SECTION 3: COMPLETION TESTS (5 tests)
// ===========================================================================

// ---------------------------------------------------------------------------
// 3.1 Final envelope produces receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn completion_final_produces_receipt() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");
    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt Ok");

    assert_eq!(receipt.backend.id, "mock-test");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 3.2 Fatal envelope produces error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn completion_fatal_produces_error() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "fatal"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");
    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt_result = sidecar_run.receipt.await.expect("channel");
    assert!(receipt_result.is_err());

    let err = receipt_result.unwrap_err();
    assert!(
        matches!(&err, HostError::Fatal(msg) if msg.contains("something went wrong")),
        "expected Fatal with message, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 3.3 Unexpected disconnect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn completion_unexpected_disconnect() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "drop_midstream"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty(), "should get events before disconnect");

    let receipt_result = sidecar_run.receipt.await;
    match receipt_result {
        Ok(Err(HostError::Exited { .. })) => { /* expected */ }
        Ok(Err(_)) => { /* any error is acceptable on abrupt disconnect */ }
        Err(_) => { /* sender dropped — acceptable */ }
        Ok(Ok(_)) => panic!("should not receive valid receipt after disconnect"),
    }
}

// ---------------------------------------------------------------------------
// 3.4 Sidecar exit code 0 vs non-zero
// ---------------------------------------------------------------------------

#[tokio::test]
async fn completion_exit_code_nonzero() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "exit_nonzero");

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    // Exit before hello → Exited error.
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited, got: {err}"
    );
}

#[tokio::test]
async fn completion_exit_code_zero_graceful() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "graceful_exit"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(events.len() >= 2, "should get multiple events");

    let receipt = sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt Ok");
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 3.5 Graceful shutdown — no final means process exits without receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn completion_graceful_shutdown_no_final() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "no_final"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());

    // Without a final envelope, we should get an error or dropped channel.
    let receipt_result = sidecar_run.receipt.await;
    match receipt_result {
        Ok(Err(HostError::Exited { .. })) => { /* expected */ }
        Ok(Err(_)) => { /* any error acceptable */ }
        Err(_) => { /* sender dropped */ }
        Ok(Ok(_)) => panic!("should not get receipt without final"),
    }
}

// ===========================================================================
// SECTION 4: ERROR HANDLING TESTS (7 tests)
// ===========================================================================

// ---------------------------------------------------------------------------
// 4.1 SidecarCrashed error contains exit code and stderr
// ---------------------------------------------------------------------------

#[test]
fn error_sidecar_crashed_contains_details() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "Killed by signal 9".into(),
    };

    let display = err.to_string();
    assert!(
        display.contains("137"),
        "should contain exit code: {display}"
    );
    assert!(
        display.contains("Killed by signal 9"),
        "should contain stderr: {display}"
    );
}

#[test]
fn error_sidecar_crashed_none_exit_code() {
    let err = HostError::SidecarCrashed {
        exit_code: None,
        stderr: "unknown crash".into(),
    };

    let display = err.to_string();
    assert!(
        display.contains("None"),
        "should represent None exit code: {display}"
    );
    assert!(display.contains("unknown crash"));
}

// ---------------------------------------------------------------------------
// 4.2 Timeout error contains duration
// ---------------------------------------------------------------------------

#[test]
fn error_timeout_contains_duration() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };

    let display = err.to_string();
    assert!(
        display.contains("30"),
        "should contain timeout duration: {display}"
    );
    assert!(
        display.contains("timed out"),
        "should mention timeout: {display}"
    );
}

#[test]
fn error_timeout_subsecond_precision() {
    let err = HostError::Timeout {
        duration: Duration::from_millis(1500),
    };

    let display = err.to_string();
    // Duration debug format shows "1.5s" or similar.
    assert!(
        display.contains("1.5") || display.contains("1500"),
        "should show sub-second precision: {display}"
    );
}

// ---------------------------------------------------------------------------
// 4.3 Protocol violation error messages
// ---------------------------------------------------------------------------

#[test]
fn error_protocol_violation_messages() {
    let err = HostError::Protocol(ProtocolError::Violation(
        "missing required field 'ref_id'".into(),
    ));
    let display = err.to_string();
    assert!(
        display.contains("missing required field"),
        "should propagate violation detail: {display}"
    );

    let err = HostError::Protocol(ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    });
    let display = err.to_string();
    assert!(
        display.contains("hello"),
        "should mention expected message: {display}"
    );
    assert!(
        display.contains("event"),
        "should mention actual message: {display}"
    );

    let err = HostError::Violation("sidecar stdin unavailable".into());
    let display = err.to_string();
    assert!(
        display.contains("stdin unavailable"),
        "should contain violation reason: {display}"
    );
}

// ---------------------------------------------------------------------------
// 4.4 Multiple errors in sequence — spawn failures don't poison state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn error_multiple_spawn_failures_recoverable() {
    let py = require_python!();

    // Multiple failures in a row.
    for _ in 0..3 {
        let bad = SidecarSpec::new("nonexistent-binary-multi-error-test");
        assert!(SidecarClient::spawn(bad).await.is_err());
    }

    // Then succeed.
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("should recover after multiple failures");
    assert_eq!(client.hello.backend.id, "mock-test");
}

// ---------------------------------------------------------------------------
// 4.5 Recovery after error — can run again after protocol error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn error_recovery_after_protocol_error() {
    let py = require_python!();

    // First run: protocol error (no hello).
    let result = SidecarClient::spawn(mock_spec_with_mode(&py, "no_hello")).await;
    assert!(result.is_err());

    // Second run: fatal error.
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "fatal"))
        .await
        .expect("spawn after protocol error");
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.expect("channel");
    assert!(receipt.is_err());

    // Third run: success.
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("should succeed after errors");
    assert_eq!(client.hello.backend.id, "mock-test");
}

// ---------------------------------------------------------------------------
// 4.6 Error types implement Send + Sync
// ---------------------------------------------------------------------------

#[test]
fn error_types_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HostError>();
    assert_send_sync::<ProtocolError>();
}

// ---------------------------------------------------------------------------
// 4.7 Error display messages are useful and non-empty
// ---------------------------------------------------------------------------

#[test]
fn error_display_messages_useful() {
    let errors: Vec<(HostError, &[&str])> = vec![
        (
            HostError::Spawn(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "not found",
            )),
            &["spawn", "not found"],
        ),
        (
            HostError::Stdout(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            )),
            &["stdout", "broken pipe"],
        ),
        (
            HostError::Stdin(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            )),
            &["stdin", "broken pipe"],
        ),
        (
            HostError::Protocol(ProtocolError::Violation("bad frame".into())),
            &["protocol"],
        ),
        (
            HostError::Violation("unexpected envelope type".into()),
            &["violation", "unexpected"],
        ),
        (
            HostError::Fatal("out of memory".into()),
            &["fatal", "out of memory"],
        ),
        (HostError::Exited { code: Some(1) }, &["exited", "1"]),
        (HostError::Exited { code: None }, &["exited"]),
        (
            HostError::SidecarCrashed {
                exit_code: Some(137),
                stderr: "OOM killed".into(),
            },
            &["crashed", "137"],
        ),
        (
            HostError::Timeout {
                duration: Duration::from_secs(10),
            },
            &["timed out"],
        ),
    ];

    for (err, keywords) in &errors {
        let display = err.to_string().to_lowercase();
        assert!(
            !display.is_empty(),
            "Display must be non-empty for {:?}",
            err
        );
        for kw in *keywords {
            assert!(
                display.contains(&kw.to_lowercase()),
                "'{kw}' not found in error display: {display}"
            );
        }
        // Verify Debug is also non-empty.
        assert!(!format!("{err:?}").is_empty());
    }
}

// ---------------------------------------------------------------------------
// 4.7b Error source chains are correct
// ---------------------------------------------------------------------------

#[test]
fn error_source_chains() {
    // Errors wrapping inner errors should expose them via source().
    let err = HostError::Spawn(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Spawn should have a source");

    let err = HostError::Stdout(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Stdout should have a source");

    let err = HostError::Stdin(std::io::Error::other("inner"));
    assert!(err.source().is_some(), "Stdin should have a source");

    let err = HostError::Protocol(ProtocolError::Violation("v".into()));
    assert!(err.source().is_some(), "Protocol should have a source");

    // Plain string variants have no source.
    let err = HostError::Violation("v".into());
    assert!(err.source().is_none());

    let err = HostError::Fatal("f".into());
    assert!(err.source().is_none());

    let err = HostError::Exited { code: Some(1) };
    assert!(err.source().is_none());
}

// ===========================================================================
// SECTION 5: PROTOCOL-LEVEL UNIT TESTS (Rust-only, no process spawn)
// ===========================================================================

// ---------------------------------------------------------------------------
// 5.1 Hello envelope round-trip encoding
// ---------------------------------------------------------------------------

#[test]
fn protocol_hello_roundtrip() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );

    let encoded = JsonlCodec::encode(&hello).expect("encode");
    assert!(encoded.ends_with('\n'));
    assert!(encoded.contains("\"t\":\"hello\""));

    let decoded = JsonlCodec::decode(encoded.trim()).expect("decode");
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-backend");
        }
        other => panic!("expected Hello, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5.2 Fatal envelope round-trip
// ---------------------------------------------------------------------------

#[test]
fn protocol_fatal_roundtrip() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-42".into()),
        error: "something broke".into(),
        error_code: None,
    };

    let encoded = JsonlCodec::encode(&fatal).expect("encode");
    let decoded = JsonlCodec::decode(encoded.trim()).expect("decode");

    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-42"));
            assert_eq!(error, "something broke");
        }
        other => panic!("expected Fatal, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5.3 Invalid JSON decoding produces ProtocolError::Json
// ---------------------------------------------------------------------------

#[test]
fn protocol_invalid_json_produces_error() {
    let result = JsonlCodec::decode("not valid json {{{");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

// ---------------------------------------------------------------------------
// 5.4 HostError From<ProtocolError> conversion
// ---------------------------------------------------------------------------

#[test]
fn protocol_error_converts_to_host_error() {
    let proto_err = ProtocolError::Violation("test".into());
    let host_err: HostError = proto_err.into();
    assert!(matches!(host_err, HostError::Protocol(_)));
}

// ===========================================================================
// SECTION 6: INTEGRATION — ORIGINAL LIFECYCLE TESTS (preserved)
// ===========================================================================

// ---------------------------------------------------------------------------
// 6.1 Normal lifecycle — spawn, run, verify clean exit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_normal_spawn_run_exit() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.backend.id, "mock-test");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty(), "should receive at least one event");
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. })),
        "expected RunStarted event"
    );

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel open")
        .expect("receipt Ok");

    assert_eq!(receipt.backend.id, "mock-test");
    assert!(matches!(receipt.outcome, Outcome::Complete));

    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 6.2 Env vars forwarded
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_env_vars_forwarded() {
    let py = require_python!();
    let mut spec = mock_spec_with_mode(&py, "echo_env");
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());

    let client = SidecarClient::spawn(spec)
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should succeed");

    let events: Vec<_> = sidecar_run.events.collect().await;
    let msg = events
        .iter()
        .find_map(|e| match &e.kind {
            AgentEventKind::RunStarted { message, .. } => Some(message.clone()),
            _ => None,
        })
        .expect("should have RunStarted event");

    assert!(
        msg.contains("ABP_TEST_VAR=hello_from_test"),
        "env var not propagated; got: {msg}"
    );

    let _ = sidecar_run.receipt.await;
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 6.3 Multiple sequential spawns
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_multiple_sequential_spawns() {
    let py = require_python!();

    for i in 0..3 {
        let client = SidecarClient::spawn(mock_spec(&py))
            .await
            .unwrap_or_else(|e| panic!("spawn #{i} failed: {e}"));

        assert_eq!(client.hello.backend.id, "mock-test");

        let run_id = Uuid::new_v4().to_string();
        let sidecar_run = client
            .run(run_id, test_work_order())
            .await
            .unwrap_or_else(|e| panic!("run #{i} failed: {e}"));

        let _events: Vec<_> = sidecar_run.events.collect().await;

        let receipt = sidecar_run
            .receipt
            .await
            .expect("receipt channel open")
            .expect("receipt Ok");
        assert!(matches!(receipt.outcome, Outcome::Complete));

        sidecar_run.wait.await.unwrap().unwrap();
    }
}

// ---------------------------------------------------------------------------
// 6.4 Registry + spawn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_registry_then_spawn() {
    let py = require_python!();

    let mut registry = SidecarRegistry::default();
    let spec = mock_spec(&py);
    let mut config = SidecarConfig::new("mock", &spec.command);
    config.args = spec.args.clone();
    config.env = spec.env.clone();
    registry.register(config).unwrap();

    let cfg = registry
        .get("mock")
        .expect("registered sidecar should be retrievable");
    let spec = cfg.to_spec();

    let client = SidecarClient::spawn(spec)
        .await
        .expect("spawn from registry spec should succeed");

    assert_eq!(client.hello.backend.id, "mock-test");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, test_work_order())
        .await
        .expect("run should succeed");

    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel open")
        .expect("receipt Ok");
    assert!(matches!(receipt.outcome, Outcome::Complete));

    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 6.5 Registry discovers sidecars from hosts/ directory
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_discover_from_hosts_dir() {
    let hosts = hosts_dir();
    if !hosts.is_dir() {
        eprintln!("SKIP: hosts/ directory not found");
        return;
    }

    let registry = SidecarRegistry::discover_from_dir(&hosts).expect("discover should succeed");

    let names = registry.list();
    assert!(
        names.contains(&"node"),
        "expected 'node' in discovered sidecars: {names:?}"
    );
    assert!(
        names.contains(&"python"),
        "expected 'python' in discovered sidecars: {names:?}"
    );

    let node_spec = registry.get("node").unwrap();
    assert_eq!(node_spec.command, "node");
    let py_spec = registry.get("python").unwrap();
    assert_eq!(py_spec.command, "python");
}

// ---------------------------------------------------------------------------
// 6.6 Registry list returns sorted names
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_registry_list_sorted() {
    let mut registry = SidecarRegistry::default();
    registry.register(SidecarConfig::new("zeta", "z")).unwrap();
    registry.register(SidecarConfig::new("alpha", "a")).unwrap();
    registry.register(SidecarConfig::new("mu", "m")).unwrap();

    let names = registry.list();
    assert_eq!(names, vec!["alpha", "mu", "zeta"]);
}

// ===========================================================================
// SECTION 7: TOOL CALL EVENT TESTS
// ===========================================================================

// ---------------------------------------------------------------------------
// 7.1 Tool call and tool result events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_tool_call_and_result() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "tool_call_events"))
        .await
        .expect("spawn");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.expect("run");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 4, "expected 4 events from tool_call mode");

    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::ToolCall { tool_name, tool_use_id, .. }
            if tool_name == "read_file" && tool_use_id.as_deref() == Some("tc-1")
    ));
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::ToolResult { tool_name, is_error, .. }
            if tool_name == "read_file" && !is_error
    ));
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    sidecar_run
        .receipt
        .await
        .expect("channel")
        .expect("receipt");
    sidecar_run.wait.await.unwrap().unwrap();
}
