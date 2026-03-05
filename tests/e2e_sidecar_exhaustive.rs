#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Exhaustive end-to-end sidecar integration tests.
//!
//! Exercises the full sidecar lifecycle: handshake, streaming events, receipt
//! delivery, error handling, capability negotiation, graceful shutdown,
//! heartbeat monitoring, receipt hashing, policy enforcement, and rate limiting.
//!
//! Uses the Python mock sidecar at `crates/abp-host/tests/mock_sidecar.py`.
//!
//! # Running
//!
//! These tests require Python on PATH:
//!
//! ```sh
//! cargo test --test e2e_sidecar_exhaustive
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use abp_policy::PolicyEngine;
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use abp_protocol::graceful_shutdown::{
    GoodbyeResponse, GoodbyeStatus, ShutdownCoordinator, ShutdownReason, ShutdownRequest,
};
use abp_protocol::heartbeat::{HeartbeatConfig, HeartbeatMonitor, HeartbeatState};
use abp_protocol::version::ProtocolVersion;
use abp_protocol::version_negotiation::{NegotiationError, VersionOffer, negotiate};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mock_script_path() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("crates")
        .join("abp-host")
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

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "exhaustive e2e test".into(),
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

fn work_order_with_policy(policy: PolicyProfile) -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "policy test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy,
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

/// Drain all events from a sidecar run and return the receipt.
async fn drain_run(run: abp_host::SidecarRun) -> (Vec<AgentEvent>, Result<Receipt, HostError>) {
    let events: Vec<_> = run.events.collect().await;
    let receipt = match run.receipt.await {
        Ok(r) => r,
        // Channel dropped without sending — treat as a crash.
        Err(_) => Err(HostError::SidecarCrashed {
            exit_code: None,
            stderr: "receipt channel dropped".into(),
        }),
    };
    let _ = run.wait.await;
    (events, receipt)
}

// =========================================================================
// Section 1: Handshake tests
// =========================================================================

#[tokio::test]
async fn handshake_hello_completes_with_correct_identity() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(client.hello.backend.backend_version.as_deref(), Some("0.1"));
    assert_eq!(client.hello.backend.adapter_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn handshake_reports_contract_version() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn handshake_wrong_version_is_preserved() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "wrong_version"))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.contract_version, "abp/v999.0");
}

#[tokio::test]
async fn handshake_no_hello_causes_protocol_error() {
    let py = require_python!();
    let result = SidecarClient::spawn(mock_spec_with_mode(&py, "no_hello")).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Protocol(_)),
        "expected Protocol error, got: {err}"
    );
}

#[tokio::test]
async fn handshake_exit_nonzero_without_hello() {
    let py = require_python!();
    let result = SidecarClient::spawn(mock_spec_with_mode(&py, "exit_nonzero")).await;
    assert!(result.is_err(), "spawn should fail on non-zero exit");
}

#[tokio::test]
async fn handshake_extra_fields_are_tolerated() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "hello_extra_fields"))
        .await
        .expect("spawn should succeed with extra fields");

    assert_eq!(client.hello.backend.id, "mock-test");
}

// =========================================================================
// Section 2: Work order → events → receipt lifecycle
// =========================================================================

#[tokio::test]
async fn lifecycle_run_produces_events() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty(), "should produce at least one event");
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn lifecycle_first_event_is_run_started() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, _receipt) = drain_run(sidecar_run).await;
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got: {:?}",
        events[0].kind
    );
}

#[tokio::test]
async fn lifecycle_final_receipt_has_complete_outcome() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (_events, receipt) = drain_run(sidecar_run).await;
    let receipt = receipt.expect("receipt should be Ok");
    assert!(
        matches!(receipt.outcome, Outcome::Complete),
        "expected Complete, got: {:?}",
        receipt.outcome
    );
}

#[tokio::test]
async fn lifecycle_receipt_backend_matches_hello() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let hello_backend_id = client.hello.backend.id.clone();

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (_events, receipt) = drain_run(sidecar_run).await;
    let receipt = receipt.expect("receipt should be Ok");
    assert_eq!(receipt.backend.id, hello_backend_id);
}

#[tokio::test]
async fn lifecycle_receipt_contract_version_matches() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (_events, receipt) = drain_run(sidecar_run).await;
    let receipt = receipt.expect("receipt should be Ok");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn lifecycle_multi_events_all_delivered() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "multi_events"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert_eq!(events.len(), 5, "expected 5 events, got {}", events.len());
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn lifecycle_mixed_event_kinds() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "multi_event_kinds"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, _receipt) = drain_run(sidecar_run).await;
    assert_eq!(events.len(), 5);

    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

#[tokio::test]
async fn lifecycle_tool_call_events_round_trip() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "tool_call_events"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;

    let has_tool_call = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file"));
    assert!(has_tool_call, "should have a ToolCall event for read_file");

    let has_tool_result = events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolResult { tool_name, .. } if tool_name == "read_file"));
    assert!(
        has_tool_result,
        "should have a ToolResult event for read_file"
    );

    assert!(receipt.is_ok());
}

#[tokio::test]
async fn lifecycle_slow_sidecar_completes_within_timeout() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "slow"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events = tokio::time::timeout(
        Duration::from_secs(10),
        sidecar_run.events.collect::<Vec<_>>(),
    )
    .await
    .expect("events should arrive within 10s");

    assert_eq!(events.len(), 3);

    let receipt = tokio::time::timeout(Duration::from_secs(5), sidecar_run.receipt)
        .await
        .expect("receipt should arrive within 5s")
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn lifecycle_unicode_content_preserved() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "unicode_content"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert_eq!(events.len(), 2);

    // Verify Unicode survived the round-trip.
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.contains("你好世界"),
            "Unicode CJK should be preserved"
        );
        assert!(message.contains("🌍"), "Unicode emoji should be preserved");
    } else {
        panic!("expected RunStarted, got: {:?}", events[0].kind);
    }

    assert!(receipt.is_ok());
}

#[tokio::test]
async fn lifecycle_large_payload_delivered() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "large_payload"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert_eq!(events.len(), 1);

    if let AgentEventKind::AssistantMessage { text } = &events[0].kind {
        assert_eq!(text.len(), 100_000, "large payload should be 100KB");
        assert!(text.chars().all(|c| c == 'A'));
    } else {
        panic!("expected AssistantMessage, got: {:?}", events[0].kind);
    }

    assert!(receipt.is_ok());
}

#[tokio::test]
async fn lifecycle_empty_lines_ignored() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "empty_lines"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert_eq!(events.len(), 2, "empty lines should be skipped");
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn lifecycle_graceful_exit_code_zero() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "graceful_exit"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(events.len() >= 2, "should have at least 2 events");
    assert!(receipt.is_ok());
}

// =========================================================================
// Section 3: Error scenarios
// =========================================================================

#[tokio::test]
async fn error_malformed_json_midstream() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "bad_json_midstream"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(
        !events.is_empty(),
        "valid events before bad JSON should arrive"
    );
    assert!(
        matches!(receipt, Err(HostError::Protocol(_))),
        "receipt should be Protocol error, got: {receipt:?}"
    );
}

#[tokio::test]
async fn error_fatal_envelope() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "fatal"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty(), "events before fatal should arrive");

    match receipt {
        Err(HostError::Fatal(msg)) => {
            assert_eq!(msg, "something went wrong");
        }
        other => panic!("expected Fatal error, got: {other:?}"),
    }
}

#[tokio::test]
async fn error_hanging_sidecar_receipt_times_out() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "hang"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    // First event should arrive.
    let mut events = sidecar_run.events;
    let first = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("first event should arrive")
        .expect("should have at least one event");
    assert!(matches!(&first.kind, AgentEventKind::RunStarted { .. }));

    // Receipt should NOT arrive because sidecar is hanging.
    let result = tokio::time::timeout(Duration::from_millis(500), sidecar_run.receipt).await;
    assert!(result.is_err(), "receipt should timeout");

    sidecar_run.wait.abort();
}

#[tokio::test]
async fn error_no_final_then_close() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "no_final"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty(), "events before close should arrive");
    assert!(receipt.is_err(), "missing final should produce an error");
}

#[tokio::test]
async fn error_drop_midstream_is_detected() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "drop_midstream"))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty());
    assert!(receipt.is_err(), "abrupt exit should result in error");
}

#[tokio::test]
async fn error_invalid_json_on_spawn() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "print('not json')".into()];

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Protocol(_)),
        "expected Protocol error on invalid JSON hello"
    );
}

#[tokio::test]
async fn error_immediate_exit_without_hello() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "import sys; sys.exit(0)".into()];

    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

// =========================================================================
// Section 4: Capability advertisement and negotiation
// =========================================================================

#[tokio::test]
async fn capability_hello_returns_capability_manifest() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    // The mock sidecar returns an empty capabilities map — that's valid.
    let _caps = &client.hello.capabilities;
    // No panic means success; capabilities can be empty.
}

#[test]
fn capability_version_negotiation_exact_match() {
    let host = VersionOffer::new(vec![ProtocolVersion { major: 0, minor: 1 }]);
    let sidecar = VersionOffer::new(vec![ProtocolVersion { major: 0, minor: 1 }]);
    let sel = negotiate(&host, &sidecar).unwrap();
    assert_eq!(sel.selected, ProtocolVersion { major: 0, minor: 1 });
}

#[test]
fn capability_version_negotiation_picks_highest_common() {
    let host = VersionOffer::new(vec![
        ProtocolVersion { major: 0, minor: 1 },
        ProtocolVersion { major: 0, minor: 2 },
        ProtocolVersion { major: 0, minor: 3 },
    ]);
    let sidecar = VersionOffer::new(vec![
        ProtocolVersion { major: 0, minor: 2 },
        ProtocolVersion { major: 0, minor: 3 },
    ]);
    let sel = negotiate(&host, &sidecar).unwrap();
    assert_eq!(sel.selected, ProtocolVersion { major: 0, minor: 3 });
}

#[test]
fn capability_version_negotiation_no_overlap_fails() {
    let host = VersionOffer::new(vec![ProtocolVersion { major: 0, minor: 1 }]);
    let sidecar = VersionOffer::new(vec![ProtocolVersion { major: 1, minor: 0 }]);
    let err = negotiate(&host, &sidecar).unwrap_err();
    assert!(matches!(err, NegotiationError::NoOverlap { .. }));
}

#[test]
fn capability_version_negotiation_empty_offer_fails() {
    let host = VersionOffer::new(vec![]);
    let sidecar = VersionOffer::new(vec![ProtocolVersion { major: 0, minor: 1 }]);
    let err = negotiate(&host, &sidecar).unwrap_err();
    assert!(matches!(err, NegotiationError::EmptyOffer));
}

#[test]
fn capability_version_compatibility_check() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", CONTRACT_VERSION));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
}

#[test]
fn capability_parse_version_contract() {
    let (major, minor) = parse_version(CONTRACT_VERSION).expect("CONTRACT_VERSION should parse");
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

#[test]
fn capability_support_level_satisfies_min_support() {
    use abp_core::MinSupport;

    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Any));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Any));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// =========================================================================
// Section 5: Graceful shutdown protocol
// =========================================================================

#[test]
fn shutdown_request_basic_construction() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(30));
    assert_eq!(req.reason(), &ShutdownReason::Normal);
    assert_eq!(req.deadline(), Duration::from_secs(30));
    assert!(req.message().is_none());
}

#[test]
fn shutdown_request_with_message() {
    let req = ShutdownRequest::new(ShutdownReason::HostShutdown, Duration::from_secs(5))
        .with_message("shutting down");
    assert_eq!(req.message(), Some("shutting down"));
}

#[test]
fn shutdown_request_expiration() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_millis(100));
    assert!(!req.is_expired(Duration::from_millis(50)));
    assert!(req.is_expired(Duration::from_millis(100)));
    assert!(req.is_expired(Duration::from_millis(200)));
}

#[test]
fn shutdown_goodbye_clean_response() {
    let resp = GoodbyeResponse::new(GoodbyeStatus::Clean).with_completed(5);
    assert!(resp.is_clean());
    assert_eq!(resp.completed_requests(), 5);
    assert_eq!(resp.abandoned_requests(), 0);
    assert!(resp.error().is_none());
}

#[test]
fn shutdown_goodbye_partial_response() {
    let resp = GoodbyeResponse::new(GoodbyeStatus::Partial)
        .with_completed(3)
        .with_abandoned(2);
    assert!(!resp.is_clean());
    assert_eq!(resp.completed_requests(), 3);
    assert_eq!(resp.abandoned_requests(), 2);
}

#[test]
fn shutdown_goodbye_error_response() {
    let resp = GoodbyeResponse::new(GoodbyeStatus::Error).with_error("disk full");
    assert!(!resp.is_clean());
    assert_eq!(resp.error(), Some("disk full"));
}

#[test]
fn shutdown_coordinator_lifecycle() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_secs(60));
    let mut coord = ShutdownCoordinator::new(req);

    assert!(!coord.is_complete());
    assert!(!coord.is_expired());
    assert!(coord.time_remaining() > Duration::from_secs(59));

    coord.record_response(GoodbyeResponse::new(GoodbyeStatus::Clean));
    assert!(coord.is_complete());
    assert!(!coord.is_expired());
}

#[test]
fn shutdown_coordinator_expired_without_response() {
    let req = ShutdownRequest::new(ShutdownReason::Normal, Duration::from_millis(1));
    let coord = ShutdownCoordinator::new(req);
    std::thread::sleep(Duration::from_millis(10));
    assert!(coord.is_expired());
}

#[test]
fn shutdown_reason_all_variants_serde() {
    let reasons = vec![
        ShutdownReason::Normal,
        ShutdownReason::ResourceLimit,
        ShutdownReason::Replacement,
        ShutdownReason::HostShutdown,
        ShutdownReason::PolicyViolation,
        ShutdownReason::Custom("test reason".into()),
    ];
    for reason in reasons {
        let json = serde_json::to_string(&reason).unwrap();
        let decoded: ShutdownReason = serde_json::from_str(&json).unwrap();
        assert_eq!(reason, decoded);
    }
}

#[tokio::test]
async fn shutdown_close_stdin_causes_sidecar_exit() {
    let py = require_python!();

    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    // Use "graceful_exit" mode which sends hello immediately.
    let mut child = Command::new(&py)
        .arg(mock_script_path())
        .arg("graceful_exit")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Read hello.
    let mut hello_line = String::new();
    reader.read_line(&mut hello_line).await.unwrap();
    assert!(
        hello_line.contains("\"t\"") && hello_line.contains("hello"),
        "expected hello envelope, got: {hello_line}"
    );

    // Close stdin — sidecar should exit.
    drop(stdin);

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("sidecar should exit within 5s")
        .expect("wait should succeed");

    // Process should have exited (code may vary).
    assert!(status.code().is_some());
}

// =========================================================================
// Section 6: Heartbeat / ping-pong timing
// =========================================================================

#[test]
fn heartbeat_initial_state_is_idle() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mon = HeartbeatMonitor::new(cfg);
    assert_eq!(*mon.state(), HeartbeatState::Idle);
    assert!(!mon.is_stalled());
    assert!(!mon.is_alive());
    assert_eq!(mon.total_pings(), 0);
    assert_eq!(mon.total_pongs(), 0);
}

#[test]
fn heartbeat_ping_pong_transitions_to_alive() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    let ping = mon.next_ping();
    assert_eq!(ping.seq, 0);
    assert_eq!(mon.total_pings(), 1);

    mon.record_pong(ping.seq);
    assert!(mon.is_alive());
    assert_eq!(mon.total_pongs(), 1);
    assert_eq!(mon.consecutive_missed(), 0);
}

#[test]
fn heartbeat_multiple_cycles() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    for i in 0..10u64 {
        let ping = mon.next_ping();
        assert_eq!(ping.seq, i);
        mon.record_pong(ping.seq);
    }
    assert_eq!(mon.total_pings(), 10);
    assert_eq!(mon.total_pongs(), 10);
    assert!(mon.is_alive());
}

#[test]
fn heartbeat_single_miss_becomes_degraded() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    let _ping = mon.next_ping();
    mon.record_miss();
    assert_eq!(*mon.state(), HeartbeatState::Degraded { missed: 1 });
    assert!(!mon.is_stalled());
}

#[test]
fn heartbeat_max_misses_becomes_stalled() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    for _ in 0..3 {
        let _ping = mon.next_ping();
        mon.record_miss();
    }
    assert!(mon.is_stalled());
    assert_eq!(*mon.state(), HeartbeatState::Stalled { missed: 3 });
}

#[test]
fn heartbeat_pong_recovery_after_misses() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    // Two misses.
    let _p = mon.next_ping();
    mon.record_miss();
    let _p = mon.next_ping();
    mon.record_miss();
    assert_eq!(mon.consecutive_missed(), 2);

    // Recovery via successful pong.
    let ping = mon.next_ping();
    mon.record_pong(ping.seq);
    assert!(mon.is_alive());
    assert_eq!(mon.consecutive_missed(), 0);
}

#[test]
fn heartbeat_wrong_seq_pong_ignored() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    let _ping = mon.next_ping();
    mon.record_pong(999);
    assert_eq!(*mon.state(), HeartbeatState::Idle);
    assert_eq!(mon.total_pongs(), 1);
}

#[test]
fn heartbeat_reset_clears_all_state() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);

    let ping = mon.next_ping();
    mon.record_pong(ping.seq);
    assert!(mon.is_alive());

    mon.reset();
    assert_eq!(*mon.state(), HeartbeatState::Idle);
    assert_eq!(mon.total_pings(), 0);
    assert_eq!(mon.total_pongs(), 0);
    assert_eq!(mon.consecutive_missed(), 0);
}

#[test]
fn heartbeat_config_stall_threshold() {
    let cfg = HeartbeatConfig::new(Duration::from_secs(5), Duration::from_secs(2), 4);
    assert_eq!(cfg.stall_threshold(), Duration::from_secs(8));
}

#[test]
fn heartbeat_should_ping_initially_true() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mon = HeartbeatMonitor::new(cfg);
    assert!(mon.should_ping());
}

#[test]
fn heartbeat_time_since_last_pong() {
    let cfg = HeartbeatConfig::new(Duration::from_millis(100), Duration::from_millis(50), 3);
    let mut mon = HeartbeatMonitor::new(cfg);
    assert!(mon.time_since_last_pong().is_none());

    let ping = mon.next_ping();
    mon.record_pong(ping.seq);
    let elapsed = mon.time_since_last_pong();
    assert!(elapsed.is_some());
    assert!(elapsed.unwrap() < Duration::from_secs(1));
}

// =========================================================================
// Section 7: Concurrent runs
// =========================================================================

#[tokio::test]
async fn concurrent_two_independent_sidecar_runs() {
    let py = require_python!();

    // Spawn two independent sidecar processes concurrently.
    let client_a = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn A should succeed");
    let client_b = SidecarClient::spawn(mock_spec_with_mode(&py, "multi_events"))
        .await
        .expect("spawn B should succeed");

    let run_a = client_a
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .expect("run A should succeed");
    let run_b = client_b
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .expect("run B should succeed");

    let (events_a, receipt_a) = drain_run(run_a).await;
    let (events_b, receipt_b) = drain_run(run_b).await;

    assert!(!events_a.is_empty());
    assert_eq!(events_b.len(), 5);
    assert!(receipt_a.is_ok());
    assert!(receipt_b.is_ok());
}

#[tokio::test]
async fn concurrent_parallel_spawns_all_succeed() {
    let py = require_python!();

    let futs: Vec<_> = (0..3)
        .map(|_| {
            let py = py.clone();
            async move {
                let client = SidecarClient::spawn(mock_spec(&py)).await?;
                let run_id = Uuid::new_v4().to_string();
                let sidecar_run = client.run(run_id, test_work_order()).await?;
                let (events, receipt) = drain_run(sidecar_run).await;
                Ok::<_, HostError>((events, receipt))
            }
        })
        .collect();

    let results = futures::future::join_all(futs).await;
    for result in results {
        let (events, receipt) = result.expect("parallel run should succeed");
        assert!(!events.is_empty());
        assert!(receipt.is_ok());
    }
}

// =========================================================================
// Section 8: Receipt hashing verification
// =========================================================================

#[tokio::test]
async fn receipt_hash_verification_after_completion() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (_events, receipt) = drain_run(sidecar_run).await;
    let receipt = receipt.expect("receipt should be Ok");

    // Compute hash and verify it's stable.
    let hash1 = abp_core::receipt_hash(&receipt).expect("hash should succeed");
    let hash2 = abp_core::receipt_hash(&receipt).expect("hash should be deterministic");
    assert_eq!(hash1, hash2, "receipt hash should be deterministic");
    assert!(!hash1.is_empty());
}

#[tokio::test]
async fn receipt_with_hash_method_works() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (_events, receipt) = drain_run(sidecar_run).await;
    let receipt = receipt.expect("receipt should be Ok");

    let hashed = receipt.with_hash().expect("with_hash should succeed");
    assert!(
        hashed.receipt_sha256.is_some(),
        "receipt_sha256 should be set after with_hash()"
    );

    // Verify the embedded hash matches a fresh computation.
    let expected = abp_core::receipt_hash(&hashed).expect("hash should succeed");
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(expected.as_str()));
}

#[test]
fn receipt_hash_excludes_sha256_field() {
    // Build a receipt manually and verify hashing works.
    let receipt = abp_core::ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();

    let hash1 = abp_core::receipt_hash(&receipt).expect("hash should succeed");

    // Setting receipt_sha256 should not change the hash output.
    let mut receipt2 = receipt;
    receipt2.receipt_sha256 = Some("bogus".into());
    let hash2 = abp_core::receipt_hash(&receipt2).expect("hash should succeed");

    assert_eq!(
        hash1, hash2,
        "receipt_sha256 field must be excluded from hash"
    );
}

#[test]
fn receipt_hash_is_valid_hex_sha256() {
    let receipt = abp_core::ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .build();

    let hash = abp_core::receipt_hash(&receipt).expect("hash should succeed");
    assert_eq!(hash.len(), 64, "SHA-256 hex string should be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should contain only hex digits"
    );
}

// =========================================================================
// Section 9: Policy enforcement
// =========================================================================

#[test]
fn policy_default_allows_all_tools() {
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).expect("engine creation should succeed");

    let decision = engine.can_use_tool("read_file");
    assert!(decision.allowed, "default policy should allow read_file");

    let decision = engine.can_use_tool("bash");
    assert!(decision.allowed, "default policy should allow bash");
}

#[test]
fn policy_disallowed_tool_is_denied() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("engine creation should succeed");

    let decision = engine.can_use_tool("bash");
    assert!(!decision.allowed, "bash should be denied");
    assert!(decision.reason.is_some(), "denial should have a reason");

    let decision = engine.can_use_tool("read_file");
    assert!(decision.allowed, "read_file should still be allowed");
}

#[test]
fn policy_deny_read_path() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("engine creation should succeed");

    let decision = engine.can_read_path(Path::new(".env"));
    assert!(!decision.allowed, ".env should be denied for reading");

    let decision = engine.can_read_path(Path::new("src/main.rs"));
    assert!(decision.allowed, "src/main.rs should be allowed");
}

#[test]
fn policy_deny_write_path() {
    let policy = PolicyProfile {
        deny_write: vec!["**/Cargo.lock".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("engine creation should succeed");

    let decision = engine.can_write_path(Path::new("Cargo.lock"));
    assert!(!decision.allowed, "Cargo.lock should be denied for writing");

    let decision = engine.can_write_path(Path::new("src/lib.rs"));
    assert!(decision.allowed, "src/lib.rs should be allowed for writing");
}

#[test]
fn policy_combined_deny_read_and_write() {
    let policy = PolicyProfile {
        deny_read: vec!["**/secrets/**".into()],
        deny_write: vec!["**/config/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("engine creation should succeed");

    assert!(!engine.can_read_path(Path::new("secrets/key.pem")).allowed);
    assert!(engine.can_write_path(Path::new("secrets/key.pem")).allowed);
    assert!(engine.can_read_path(Path::new("config/app.toml")).allowed);
    assert!(!engine.can_write_path(Path::new("config/app.toml")).allowed);
}

#[tokio::test]
async fn policy_enforcement_during_sidecar_tool_events() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "tool_call_events"))
        .await
        .expect("spawn should succeed");

    let policy = PolicyProfile {
        disallowed_tools: vec!["write_file".into()],
        deny_read: vec!["**/secret.txt".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("engine creation should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, _receipt) = drain_run(sidecar_run).await;

    // Verify policy can be applied to tool call events.
    for event in &events {
        if let AgentEventKind::ToolCall { tool_name, .. } = &event.kind {
            let decision = engine.can_use_tool(tool_name);
            // read_file is allowed by our policy.
            if tool_name == "read_file" {
                assert!(decision.allowed);
            }
        }
    }

    // Verify disallowed tool is denied.
    let decision = engine.can_use_tool("write_file");
    assert!(!decision.allowed);
}

// =========================================================================
// Section 10: Rate limiting integration
// =========================================================================

#[test]
fn rate_limit_unlimited_always_allows() {
    let policy = RateLimitPolicy::unlimited();
    let result = policy.check_rate_limit(1000, 1_000_000, 100);
    assert!(result.is_allowed());
}

#[test]
fn rate_limit_rpm_exceeded_throttles() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        max_tokens_per_minute: None,
        max_concurrent: None,
    };
    let result = policy.check_rate_limit(10, 0, 0);
    assert!(result.is_throttled(), "RPM at limit should throttle");
}

#[test]
fn rate_limit_rpm_under_limit_allows() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(10),
        max_tokens_per_minute: None,
        max_concurrent: None,
    };
    let result = policy.check_rate_limit(5, 0, 0);
    assert!(result.is_allowed(), "RPM under limit should allow");
}

#[test]
fn rate_limit_tpm_exceeded_throttles() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: None,
        max_tokens_per_minute: Some(1000),
        max_concurrent: None,
    };
    let result = policy.check_rate_limit(0, 1000, 0);
    assert!(result.is_throttled(), "TPM at limit should throttle");
}

#[test]
fn rate_limit_concurrent_exceeded_denies() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: None,
        max_tokens_per_minute: None,
        max_concurrent: Some(5),
    };
    let result = policy.check_rate_limit(0, 0, 5);
    assert!(result.is_denied(), "concurrent at limit should deny");
}

#[test]
fn rate_limit_concurrent_under_limit_allows() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: None,
        max_tokens_per_minute: None,
        max_concurrent: Some(5),
    };
    let result = policy.check_rate_limit(0, 0, 3);
    assert!(result.is_allowed());
}

#[test]
fn rate_limit_concurrent_takes_precedence_over_rpm() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(100),
        max_tokens_per_minute: None,
        max_concurrent: Some(2),
    };
    // Concurrent limit exceeded even though RPM is fine.
    let result = policy.check_rate_limit(1, 0, 2);
    assert!(result.is_denied());
}

#[test]
fn rate_limit_all_limits_set_most_restrictive_wins() {
    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        max_tokens_per_minute: Some(10_000),
        max_concurrent: Some(10),
    };

    // All under limit.
    assert!(policy.check_rate_limit(30, 5000, 5).is_allowed());

    // Concurrent exceeded.
    assert!(policy.check_rate_limit(30, 5000, 10).is_denied());

    // RPM exceeded but concurrent OK.
    assert!(policy.check_rate_limit(60, 5000, 5).is_throttled());
}

#[tokio::test]
async fn rate_limit_applied_before_sidecar_run() {
    let py = require_python!();

    let policy = RateLimitPolicy {
        max_requests_per_minute: Some(100),
        max_tokens_per_minute: None,
        max_concurrent: Some(10),
    };

    // Check rate limit before spawning.
    let result = policy.check_rate_limit(5, 0, 1);
    assert!(result.is_allowed(), "should be allowed to run");

    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (_events, receipt) = drain_run(sidecar_run).await;
    assert!(receipt.is_ok());
}

// =========================================================================
// Section 11: JSONL codec integration
// =========================================================================

#[test]
fn codec_hello_envelope_round_trip() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        CapabilityManifest::new(),
    );

    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'));
    assert!(encoded.contains("\"t\":\"hello\""));

    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn codec_fatal_envelope_round_trip() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-123".into()),
        error: "out of memory".into(),
        error_code: None,
    };

    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-123".into()));
            assert_eq!(error, "out of memory");
        }
        other => panic!("expected Fatal, got: {other:?}"),
    }
}

#[test]
fn codec_decode_invalid_json_returns_error() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
}

#[test]
fn codec_decode_stream_skips_blank_lines() {
    use std::io::BufReader;

    let input = format!(
        "{}\n\n{}\n\n",
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        })
        .unwrap(),
        serde_json::to_string(&Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        })
        .unwrap(),
    );

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

// =========================================================================
// Section 12: Environment and CWD propagation
// =========================================================================

#[tokio::test]
async fn env_var_propagation_to_sidecar() {
    let py = require_python!();
    let mut spec = mock_spec_with_mode(&py, "echo_env");
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());

    let client = SidecarClient::spawn(spec)
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty());

    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.contains("hello_from_test"),
            "env var should be propagated, got: {message}"
        );
    } else {
        panic!("expected RunStarted, got: {:?}", events[0].kind);
    }
    assert!(receipt.is_ok());
}

#[tokio::test]
async fn cwd_propagation_to_sidecar() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "echo_cwd");

    let client = SidecarClient::spawn(spec)
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty());

    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.starts_with("cwd="),
            "should report cwd, got: {message}"
        );
    } else {
        panic!("expected RunStarted, got: {:?}", events[0].kind);
    }
    assert!(receipt.is_ok());
}

// =========================================================================
// Section 13: Work order builder integration
// =========================================================================

#[tokio::test]
async fn builder_work_order_flows_through_sidecar() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py))
        .await
        .expect("spawn should succeed");

    let wo = WorkOrderBuilder::new("builder test task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .max_turns(10)
        .build();

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), wo)
        .await
        .expect("run should succeed");

    let (events, receipt) = drain_run(sidecar_run).await;
    assert!(!events.is_empty());
    assert!(receipt.is_ok());
}
