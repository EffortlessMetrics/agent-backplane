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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep protocol conformance tests for the ABP JSONL protocol.
//!
//! Tests cover envelope parsing, handshake validation, event streaming,
//! error envelopes, receipt handling, protocol edge cases, concurrent
//! sessions, and timeout behaviour.

use abp_core::{
    AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, CapabilityRequirements,
    ContextPacket, ExecutionLane, PolicyProfile, Receipt, RuntimeConfig, WorkOrder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "deep conformance test".into(),
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

fn deep_mock_script_path() -> String {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests")
        .join("mock_sidecar_deep.py")
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

fn spec_mode(py: &str, mode: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![mock_script_path(), mode.to_string()];
    spec
}

fn deep_spec_mode(py: &str, mode: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![deep_mock_script_path(), mode.to_string()];
    spec
}

// ===========================================================================
// 1. Envelope Parsing — all envelope types
// ===========================================================================

#[test]
fn parse_hello_envelope() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn parse_run_envelope() {
    let wo = test_work_order();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn parse_event_envelope() {
    let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2024-01-01T00:00:00Z","type":"run_started","message":"hello"}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
}

#[test]
fn parse_final_envelope() {
    let json = r#"{"t":"final","ref_id":"run-1","receipt":{"meta":{"run_id":"00000000-0000-0000-0000-000000000000","work_order_id":"00000000-0000-0000-0000-000000000000","contract_version":"abp/v0.1","started_at":"2024-01-01T00:00:00Z","finished_at":"2024-01-01T00:00:00Z","duration_ms":0},"backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped","usage_raw":{},"usage":{"input_tokens":0,"output_tokens":0},"trace":[],"artifacts":[],"verification":{"harness_ok":true},"outcome":"complete","receipt_sha256":null}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Final { .. }));
}

#[test]
fn parse_fatal_envelope() {
    let json = r#"{"t":"fatal","ref_id":"run-1","error":"something broke"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { error, .. } if error == "something broke"));
}

#[test]
fn parse_fatal_envelope_null_ref_id() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"global failure"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "global failure");
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn parse_malformed_json_returns_error() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn parse_empty_object_returns_error() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn parse_missing_discriminator_returns_error() {
    let result = JsonlCodec::decode(r#"{"ref_id":"run-1","error":"boom"}"#);
    assert!(result.is_err());
}

#[test]
fn parse_unknown_envelope_type_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":42}"#);
    assert!(result.is_err());
}

// ===========================================================================
// 2. Handshake Validation
// ===========================================================================

#[test]
fn hello_with_correct_fields() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains("\"t\":\"hello\""));
    assert!(line.contains("\"contract_version\":\"abp/v0.1\""));
    assert!(line.contains("\"test-backend\""));
}

#[test]
fn hello_round_trip_preserves_all_fields() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "rt-test".into(),
            backend_version: Some("2.0".into()),
            adapter_version: Some("1.5".into()),
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "rt-test");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("1.5"));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_missing_backend_id_still_parses() {
    // An empty id is valid JSON but should be caught by validation.
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello { backend, .. } => assert!(backend.id.is_empty()),
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn hello_wrong_version_string_still_parses() {
    let json = r#"{"t":"hello","contract_version":"not-a-version","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "not-a-version");
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires python"]
async fn handshake_completes_with_correct_version() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .expect("spawn should succeed");
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
#[ignore = "requires python"]
async fn handshake_wrong_version_is_preserved() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "wrong_version"))
        .await
        .expect("spawn should succeed");
    assert_eq!(client.hello.contract_version, "abp/v999.0");
}

#[tokio::test]
#[ignore = "requires python"]
async fn handshake_missing_hello_is_exit_error() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "import sys; sys.exit(0)".into()];
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    assert!(matches!(err, HostError::Exited { .. }));
}

#[tokio::test]
#[ignore = "requires python"]
async fn handshake_non_hello_first_is_protocol_error() {
    let py = require_python!();
    let err = SidecarClient::spawn(spec_mode(&py, "no_hello"))
        .await
        .unwrap_err();
    assert!(matches!(err, HostError::Protocol(_)));
}

#[tokio::test]
#[ignore = "requires python"]
async fn handshake_invalid_json_first_line() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "print('NOT JSON')".into()];
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    assert!(matches!(err, HostError::Protocol(_)));
}

// ===========================================================================
// 3. Event Streaming
// ===========================================================================

#[tokio::test]
#[ignore = "requires python"]
async fn events_arrive_in_order() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "multi_events"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5);
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn events_have_correct_kinds() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "multi_event_kinds"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5);

    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(events[3].kind, AgentEventKind::FileChanged { .. }));
    assert!(matches!(
        events[4].kind,
        AgentEventKind::RunCompleted { .. }
    ));

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn wrong_ref_id_events_are_dropped() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "wrong_ref_id"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    // The mock sends 3 events, but one has a wrong ref_id → should be dropped.
    assert_eq!(events.len(), 2);
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn tool_call_events_stream_correctly() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "tool_call_events"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 4); // run_started, tool_call, tool_result, run_completed

    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file")));
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolResult { tool_name, .. } if tool_name == "read_file")));

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn single_event_run() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ===========================================================================
// 4. Error Envelopes — Fatal message format, error codes
// ===========================================================================

#[tokio::test]
#[ignore = "requires python"]
async fn fatal_envelope_delivers_error_message() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "fatal")).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let result = sidecar_run.receipt.await.unwrap();
    match result {
        Err(HostError::Fatal(msg)) => assert_eq!(msg, "something went wrong"),
        other => panic!("expected Fatal, got {other:?}"),
    }
    let _ = sidecar_run.wait.await;
}

#[test]
fn fatal_with_error_code_round_trips() {
    let env = Envelope::fatal_with_code(
        Some("run-42".into()),
        "rate limited",
        abp_error::ErrorCode::BackendRateLimited,
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::BackendRateLimited)
    );
}

#[test]
fn fatal_without_error_code() {
    let json = r#"{"t":"fatal","ref_id":"run-1","error":"plain error"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(env.error_code().is_none());
}

#[test]
fn fatal_with_null_ref_id_and_code() {
    let env = Envelope::fatal_with_code(None, "crashed", abp_error::ErrorCode::BackendCrashed);
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal {
            ref_id,
            error,
            error_code,
        } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "crashed");
            assert_eq!(error_code, Some(abp_error::ErrorCode::BackendCrashed));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires python"]
async fn fatal_with_error_code_from_sidecar() {
    let py = require_python!();
    let client = SidecarClient::spawn(deep_spec_mode(&py, "fatal_with_code"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let result = sidecar_run.receipt.await.unwrap();
    assert!(matches!(result, Err(HostError::Fatal(_))));
    let _ = sidecar_run.wait.await;
}

// ===========================================================================
// 5. Receipt Envelope — Final with receipt, hash verification
// ===========================================================================

#[tokio::test]
#[ignore = "requires python"]
async fn receipt_has_correct_backend_identity() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock-test");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn receipt_has_correct_contract_version() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn receipt_outcome_is_complete() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    let _ = sidecar_run.wait.await;
}

#[test]
fn receipt_with_hash_produces_sha256() {
    let receipt = make_minimal_receipt();
    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    let hash = hashed.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_is_deterministic() {
    let receipt = make_minimal_receipt();
    let h1 = receipt.clone().with_hash().unwrap().receipt_sha256.unwrap();
    let h2 = receipt.with_hash().unwrap().receipt_sha256.unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_excludes_self() {
    let receipt = make_minimal_receipt();
    let hashed = receipt.clone().with_hash().unwrap();
    // Re-hashing the already-hashed receipt should give the same hash,
    // because with_hash sets receipt_sha256 to null before hashing.
    let rehashed = hashed.with_hash().unwrap();
    assert_eq!(
        receipt.with_hash().unwrap().receipt_sha256,
        rehashed.receipt_sha256
    );
}

#[test]
fn final_envelope_round_trip_with_receipt() {
    let receipt = make_minimal_receipt();
    let env = Envelope::Final {
        ref_id: "run-99".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-99");
            assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

// ===========================================================================
// 6. Protocol Edge Cases
// ===========================================================================

#[tokio::test]
#[ignore = "requires python"]
async fn empty_lines_between_events_ignored() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "empty_lines"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 2);
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn large_payload_is_handled() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "large_payload"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text.len(), 100_000);
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn unicode_content_preserved() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "unicode_content"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 2);
    match &events[0].kind {
        AgentEventKind::RunStarted { message } => {
            assert!(message.contains("你好世界"));
            assert!(message.contains('🌍'));
        }
        other => panic!("expected RunStarted, got {other:?}"),
    }
    match &events[1].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(text.contains('🚀'));
            assert!(text.contains('∑'));
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn extra_fields_in_hello_ignored() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "hello_extra_fields"))
        .await
        .unwrap();
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
    assert_eq!(client.hello.backend.id, "mock-test");
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    let _ = sidecar_run.wait.await;
}

#[test]
fn partial_json_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"hello","contract_version":"#);
    assert!(result.is_err());
}

#[test]
fn binary_in_json_string_parses() {
    // JSON allows escaped unicode; ensure binary-ish content in strings works.
    let json = r#"{"t":"fatal","ref_id":null,"error":"binary \u0000\u0001\u0002"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert!(error.contains('\u{0000}')),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn very_long_error_string_parses() {
    let long_msg = "x".repeat(100_000);
    let json = format!(r#"{{"t":"fatal","ref_id":null,"error":"{long_msg}"}}"#);
    let env = JsonlCodec::decode(&json).unwrap();
    match env {
        Envelope::Fatal { error, .. } => assert_eq!(error.len(), 100_000),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[test]
fn whitespace_only_line_via_decode_stream() {
    let input = "   \n\t\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert!(envelopes.is_empty());
}

#[test]
fn decode_stream_mixed_valid_and_blank() {
    let input = r#"
{"t":"fatal","ref_id":null,"error":"one"}

{"t":"fatal","ref_id":null,"error":"two"}
   
"#;
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn encode_always_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    assert_eq!(line.matches('\n').count(), 1);
}

#[test]
fn envelope_json_uses_t_discriminator() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "t-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_value(&env).unwrap();
    assert_eq!(json["t"], "hello");
}

// ===========================================================================
// 7. Concurrent Protocol Sessions — multiple ref_ids
// ===========================================================================

#[tokio::test]
#[ignore = "requires python"]
async fn two_sequential_runs_different_ids() {
    let py = require_python!();

    // First run
    let client1 = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .unwrap();
    let run_id1 = Uuid::new_v4().to_string();
    let sidecar_run1 = client1.run(run_id1, test_work_order()).await.unwrap();
    let events1: Vec<_> = sidecar_run1.events.collect().await;
    let receipt1 = sidecar_run1.receipt.await.unwrap().unwrap();
    let _ = sidecar_run1.wait.await;

    // Second run (independent sidecar)
    let client2 = SidecarClient::spawn(spec_mode(&py, "default"))
        .await
        .unwrap();
    let run_id2 = Uuid::new_v4().to_string();
    let sidecar_run2 = client2.run(run_id2, test_work_order()).await.unwrap();
    let events2: Vec<_> = sidecar_run2.events.collect().await;
    let receipt2 = sidecar_run2.receipt.await.unwrap().unwrap();
    let _ = sidecar_run2.wait.await;

    assert!(!events1.is_empty());
    assert!(!events2.is_empty());
    assert_eq!(receipt1.backend.id, receipt2.backend.id);
}

#[tokio::test]
#[ignore = "requires python"]
async fn parallel_sidecars_independent() {
    let py = require_python!();
    let (client1, client2) = tokio::join!(
        SidecarClient::spawn(spec_mode(&py, "default")),
        SidecarClient::spawn(spec_mode(&py, "multi_events")),
    );
    let client1 = client1.unwrap();
    let client2 = client2.unwrap();

    let run_id1 = Uuid::new_v4().to_string();
    let run_id2 = Uuid::new_v4().to_string();

    let (run1, run2) = tokio::join!(
        client1.run(run_id1, test_work_order()),
        client2.run(run_id2, test_work_order()),
    );
    let run1 = run1.unwrap();
    let run2 = run2.unwrap();

    let (events1, events2) = tokio::join!(
        run1.events.collect::<Vec<_>>(),
        run2.events.collect::<Vec<_>>(),
    );
    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 5);

    let (r1, r2) = tokio::join!(run1.receipt, run2.receipt);
    assert!(r1.unwrap().is_ok());
    assert!(r2.unwrap().is_ok());

    let _ = run1.wait.await;
    let _ = run2.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn mismatched_ref_id_in_final_handled() {
    let py = require_python!();
    let client = SidecarClient::spawn(deep_spec_mode(&py, "wrong_ref_final"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    // The final has a wrong ref_id so it gets dropped; the sidecar then exits.
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), sidecar_run.receipt).await;
    // Either timeout (final was dropped) or an error.
    match result {
        Ok(Ok(Err(_))) => {} // error receipt
        Ok(Err(_)) => {}     // channel dropped
        Err(_) => {}         // timeout — also acceptable
        Ok(Ok(Ok(_))) => panic!("should not get a successful receipt with wrong ref_id"),
    }
    sidecar_run.wait.abort();
}

// ===========================================================================
// 8. Timeout Handling
// ===========================================================================

#[tokio::test]
#[ignore = "requires python"]
async fn slow_hello_still_completes() {
    let py = require_python!();
    let client = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        SidecarClient::spawn(deep_spec_mode(&py, "slow_hello")),
    )
    .await
    .expect("hello should arrive within 10s")
    .unwrap();
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
#[ignore = "requires python"]
async fn slow_events_still_complete() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "slow")).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sidecar_run.events.collect::<Vec<_>>(),
    )
    .await
    .expect("events should arrive within 10s");
    assert_eq!(events.len(), 3);
    let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), sidecar_run.receipt)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn hung_sidecar_receipt_times_out() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "hang")).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let mut events = sidecar_run.events;
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), events.next())
        .await
        .expect("first event should arrive");
    assert!(first.is_some());
    let result =
        tokio::time::timeout(std::time::Duration::from_millis(500), sidecar_run.receipt).await;
    assert!(result.is_err(), "receipt should timeout");
    sidecar_run.wait.abort();
}

#[tokio::test]
#[ignore = "requires python"]
async fn sidecar_crash_midstream() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "drop_midstream"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());
    // Receipt channel may be dropped (RecvError) or return an error.
    match sidecar_run.receipt.await {
        Ok(Err(_)) => {} // error receipt — expected
        Err(_) => {}     // channel dropped — also acceptable
        Ok(Ok(_)) => panic!("should not get a successful receipt after crash"),
    }
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn no_final_is_exit_error() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "no_final"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());
    // Receipt channel may be dropped (RecvError) or return an error.
    match sidecar_run.receipt.await {
        Ok(Err(_)) => {} // error receipt — expected
        Err(_) => {}     // channel dropped — also acceptable
        Ok(Ok(_)) => panic!("should not get a successful receipt without final"),
    }
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn nonzero_exit_before_hello() {
    let py = require_python!();
    let err = SidecarClient::spawn(spec_mode(&py, "exit_nonzero"))
        .await
        .unwrap_err();
    assert!(matches!(err, HostError::Exited { .. }));
}

#[tokio::test]
#[ignore = "requires python"]
async fn malformed_json_midstream_is_protocol_error() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "bad_json_midstream"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let result = sidecar_run.receipt.await.unwrap();
    assert!(matches!(result, Err(HostError::Protocol(_))));
    let _ = sidecar_run.wait.await;
}

// ===========================================================================
// Additional validation & protocol tests
// ===========================================================================

#[test]
fn version_parsing_valid() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(abp_protocol::parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn version_parsing_invalid() {
    assert!(abp_protocol::parse_version("invalid").is_none());
    assert!(abp_protocol::parse_version("").is_none());
    assert!(abp_protocol::parse_version("abp/v").is_none());
    assert!(abp_protocol::parse_version("abp/v1").is_none());
    assert!(abp_protocol::parse_version("v0.1").is_none());
}

#[test]
fn version_compatibility() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!abp_protocol::is_compatible_version("invalid", "abp/v0.1"));
}

#[test]
fn validate_hello_envelope_valid() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let hello = Envelope::hello(
        BackendIdentity {
            id: "valid".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        CapabilityManifest::new(),
    );
    let result = validator.validate(&hello);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validate_hello_empty_backend_id() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: abp_core::ExecutionMode::Mapped,
    };
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, abp_protocol::validate::ValidationError::EmptyField { field } if field == "backend.id")));
}

#[test]
fn validate_hello_invalid_version() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let hello = Envelope::Hello {
        contract_version: "not_valid".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: abp_core::ExecutionMode::Mapped,
    };
    let result = validator.validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        abp_protocol::validate::ValidationError::InvalidVersion { .. }
    )));
}

#[test]
fn validate_sequence_happy_path() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let wo = test_work_order();
    let sequence = vec![
        Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        ),
        Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "run-1".into(),
            event: abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "hi".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: make_minimal_receipt(),
        },
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.is_empty(), "expected no errors, got {errors:?}");
}

#[test]
fn validate_sequence_missing_hello() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let sequence = vec![Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    }];
    let errors = validator.validate_sequence(&sequence);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, abp_protocol::validate::SequenceError::MissingHello))
    );
}

#[test]
fn validate_sequence_ref_id_mismatch() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let wo = test_work_order();
    let sequence = vec![
        Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        ),
        Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: "WRONG".into(),
            event: abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "x".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "run-1".into(),
            receipt: make_minimal_receipt(),
        },
    ];
    let errors = validator.validate_sequence(&sequence);
    assert!(errors.iter().any(|e| matches!(
        e,
        abp_protocol::validate::SequenceError::RefIdMismatch { .. }
    )));
}

#[test]
fn validate_fatal_empty_error() {
    let validator = abp_protocol::validate::EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: String::new(),
        error_code: None,
    };
    let result = validator.validate(&env);
    assert!(!result.valid);
}

#[tokio::test]
#[ignore = "requires python"]
async fn graceful_exit_after_final() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "graceful_exit"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 2);
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn multi_final_only_first_taken() {
    let py = require_python!();
    let client = SidecarClient::spawn(spec_mode(&py, "multi_final"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
#[ignore = "requires python"]
async fn env_vars_passed_to_sidecar() {
    let py = require_python!();
    let mut spec = spec_mode(&py, "echo_env");
    spec.env
        .insert("ABP_TEST_VAR".into(), "deep_test_value".into());
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunStarted { message } => {
            assert!(message.contains("deep_test_value"));
        }
        other => panic!("expected RunStarted, got {other:?}"),
    }
    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ===========================================================================
// Helpers (non-test)
// ===========================================================================

fn make_minimal_receipt() -> Receipt {
    let now = chrono::Utc::now();
    Receipt {
        meta: abp_core::RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: abp_core::ExecutionMode::Mapped,
        usage_raw: serde_json::Value::Object(serde_json::Map::new()),
        usage: abp_core::UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: abp_core::VerificationReport::default(),
        outcome: abp_core::Outcome::Complete,
        receipt_sha256: None,
    }
}
