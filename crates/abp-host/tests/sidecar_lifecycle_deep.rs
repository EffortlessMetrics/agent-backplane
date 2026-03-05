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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive deep tests for sidecar process management lifecycle.
//!
//! Categories:
//!  1. Process spawn
//!  2. Hello handshake
//!  3. Protocol violations
//!  4. Process termination
//!  5. Process crash
//!  6. Timeout
//!  7. Stderr capture
//!  8. Multiple sidecars
//!  9. Environment variables
//! 10. Working directory
//! 11. Binary resolution
//! 12. Registration
//! 13. Event streaming
//! 14. Final / Fatal envelopes
//! 15. Sidecar-kit helpers

use abp_core::{
    AgentEventKind, BackendIdentity, CapabilityManifest, CapabilityRequirements, ContextPacket,
    ExecutionLane, ExecutionMode, Outcome, PolicyProfile, Receipt, RunMetadata, RuntimeConfig,
    UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{compute_delay, is_retryable, RetryConfig, RetryMetadata};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec};
use std::time::Duration;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

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

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "lifecycle-test".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.1".into()),
    }
}

fn test_receipt(run_id: Uuid) -> Receipt {
    let now = chrono::Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::Value::Null,
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Process spawn
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn spawn_default_mode_succeeds() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await;
    assert!(client.is_ok(), "spawn should succeed: {:?}", client.err());
}

#[tokio::test]
async fn spawn_returns_backend_id() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
}

#[tokio::test]
async fn spawn_returns_backend_version() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn spawn_returns_contract_version() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn spawn_nonexistent_binary_fails() {
    let spec = SidecarSpec::new("__nonexistent_binary_xyz_42__");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Spawn(_)));
}

#[test]
fn sidecar_spec_new_defaults() {
    let spec = SidecarSpec::new("cmd");
    assert_eq!(spec.command, "cmd");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_with_args() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["index.js".into(), "--port".into(), "3000".into()];
    assert_eq!(spec.args.len(), 3);
}

#[test]
fn sidecar_spec_serde_roundtrip() {
    let mut spec = SidecarSpec::new("python");
    spec.args = vec!["script.py".into()];
    spec.env.insert("KEY".into(), "VAL".into());
    spec.cwd = Some("/tmp".into());

    let json = serde_json::to_string(&spec).unwrap();
    let back: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.command, "python");
    assert_eq!(back.args, vec!["script.py"]);
    assert_eq!(back.env["KEY"], "VAL");
    assert_eq!(back.cwd.as_deref(), Some("/tmp"));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Hello handshake
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn hello_handshake_default_mode() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.contract_version, "abp/v0.1");
    assert_eq!(client.hello.backend.id, "mock-test");
}

#[tokio::test]
async fn hello_with_extra_fields_tolerated() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "hello_extra_fields");
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
}

#[tokio::test]
async fn hello_wrong_version_still_parses() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "wrong_version");
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.contract_version, "abp/v999.0");
}

#[test]
fn hello_envelope_codec_roundtrip() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_envelope_has_correct_tag() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let json = serde_json::to_string(&env).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["t"], "hello");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Protocol violations
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_hello_is_protocol_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "no_hello");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Protocol(_)),
        "expected Protocol error, got: {err}"
    );
}

#[tokio::test]
async fn bad_json_midstream_terminates_run() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "bad_json_midstream");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap();
    assert!(receipt.is_err(), "bad JSON should produce an error receipt");
}

#[test]
fn decode_garbage_returns_error() {
    let result = JsonlCodec::decode("this is not JSON {{{");
    assert!(result.is_err());
}

#[test]
fn unexpected_envelope_type_detected() {
    let event_json = r#"{"t":"event","ref_id":"x","event":{"ts":"2024-01-01T00:00:00Z","type":"run_started","message":"hi"}}"#;
    let env: Envelope = serde_json::from_str(event_json).unwrap();
    assert!(matches!(env, Envelope::Event { .. }));
    // When used in place of hello, library would reject this.
}

#[test]
fn run_envelope_not_accepted_as_hello() {
    let run_json = r#"{"t":"run","id":"r1","work_order":{}}"#;
    // Attempting to decode as Envelope should work, but it won't match Hello.
    let env: Result<Envelope, _> = serde_json::from_str(run_json);
    // Envelope::Run expects a full WorkOrder; this will fail deserialization.
    assert!(env.is_err() || !matches!(env.as_ref().unwrap(), Envelope::Hello { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Process termination (clean shutdown)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn graceful_exit_code_zero() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "graceful_exit");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());
    let receipt = sidecar_run.receipt.await.unwrap();
    assert!(receipt.is_ok(), "graceful exit should produce Ok receipt");
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn graceful_exit_receipt_is_complete() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "graceful_exit");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[test]
fn process_status_exited_code() {
    let status = ProcessStatus::Exited { code: 0 };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("exited"));
    assert!(json.contains('0'));
}

#[test]
fn process_status_killed() {
    let status = ProcessStatus::Killed;
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("killed"));
}

#[test]
fn process_info_is_terminated_after_exit() {
    let spec = SidecarSpec::new("cmd");
    let config = ProcessConfig::default();
    let mut info = ProcessInfo::new(spec, config);
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(info.is_terminated());
    assert!(!info.is_running());
}

#[test]
fn process_info_is_terminated_after_killed() {
    let spec = SidecarSpec::new("cmd");
    let config = ProcessConfig::default();
    let mut info = ProcessInfo::new(spec, config);
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_after_timeout() {
    let spec = SidecarSpec::new("cmd");
    let config = ProcessConfig::default();
    let mut info = ProcessInfo::new(spec, config);
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Process crash
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn nonzero_exit_before_hello_is_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "exit_nonzero");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited error, got: {err}"
    );
}

#[tokio::test]
async fn drop_midstream_produces_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "drop_midstream");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt_result = sidecar_run.receipt.await;
    // Either the channel was closed (RecvError) or the receipt is an error.
    match receipt_result {
        Err(_) => {}     // RecvError: task exited without sending — expected
        Ok(Err(_)) => {} // HostError from the run — expected
        Ok(Ok(_)) => panic!("mid-stream drop should not produce Ok receipt"),
    }
}

#[tokio::test]
async fn no_final_produces_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "no_final");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty(), "should receive events before crash");
    let receipt_result = sidecar_run.receipt.await;
    // Either the channel was closed (RecvError) or the receipt is an error.
    match receipt_result {
        Err(_) => {}     // RecvError: task exited without sending — expected
        Ok(Err(_)) => {} // HostError from the run — expected
        Ok(Ok(_)) => panic!("missing final should be an error"),
    }
}

#[test]
fn host_error_exited_display_with_code() {
    let err = HostError::Exited { code: Some(42) };
    let msg = err.to_string();
    assert!(msg.contains("42"));
    assert!(msg.contains("exited"));
}

#[test]
fn host_error_exited_display_without_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("None"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"));
    assert!(msg.contains("segfault"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Timeout
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(5),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"));
}

#[test]
fn retry_config_default_has_overall_timeout() {
    let cfg = RetryConfig::default();
    assert!(cfg.overall_timeout > Duration::ZERO);
    assert!(cfg.max_retries > 0);
}

#[test]
fn timeout_error_is_retryable() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(5),
    };
    assert!(is_retryable(&err));
}

#[test]
fn protocol_error_is_not_retryable() {
    let err = HostError::Protocol(abp_protocol::ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "event".into(),
    });
    assert!(!is_retryable(&err));
}

#[test]
fn violation_error_is_not_retryable() {
    let err = HostError::Violation("bad protocol".into());
    assert!(!is_retryable(&err));
}

#[test]
fn spawn_error_is_retryable() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    assert!(is_retryable(&err));
}

#[test]
fn exited_error_is_retryable() {
    let err = HostError::Exited { code: Some(1) };
    assert!(is_retryable(&err));
}

#[test]
fn crashed_error_is_retryable() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "boom".into(),
    };
    assert!(is_retryable(&err));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Stderr capture
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn stderr_does_not_block_sidecar() {
    // The mock sidecar in "default" mode may emit nothing on stderr.
    // Verify the run still completes.
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap();
    assert!(receipt.is_ok());
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken pipe",
    ));
    assert!(err.to_string().contains("stdout"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken pipe",
    ));
    assert!(err.to_string().contains("stdin"));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Multiple sidecars
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multiple_concurrent_spawns() {
    let py = require_python!();
    let futures: Vec<_> = (0..3)
        .map(|_| {
            let spec = mock_spec(&py);
            SidecarClient::spawn(spec)
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    for (i, r) in results.into_iter().enumerate() {
        assert!(r.is_ok(), "sidecar {i} should spawn successfully");
    }
}

#[tokio::test]
async fn multiple_concurrent_runs() {
    let py = require_python!();

    let mut handles = Vec::new();
    for _ in 0..3 {
        let spec = mock_spec(&py);
        let client = SidecarClient::spawn(spec).await.unwrap();
        let run_id = Uuid::new_v4().to_string();
        let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
        handles.push(sidecar_run);
    }

    for (i, sr) in handles.into_iter().enumerate() {
        let _events: Vec<_> = sr.events.collect().await;
        let receipt = sr.receipt.await.unwrap();
        assert!(receipt.is_ok(), "run {i} receipt should be Ok");
    }
}

#[test]
fn pool_multiple_entries() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(pool.add("s3"));
    assert_eq!(pool.total_count(), 3);
    assert_eq!(pool.idle_count(), 3);
}

#[test]
fn pool_acquire_marks_busy() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.state, PoolEntryState::Busy);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_max_size_enforced() {
    let config = PoolConfig {
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3"), "should reject when at max_size");
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Environment variables
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn env_var_passed_to_sidecar() {
    let py = require_python!();
    let mut spec = mock_spec_with_mode(&py, "echo_env");
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());
    let msg = match &events[0].kind {
        AgentEventKind::RunStarted { message } => message.clone(),
        other => panic!("expected RunStarted, got: {other:?}"),
    };
    assert!(
        msg.contains("hello_from_test"),
        "expected env var in message, got: {msg}"
    );
}

#[tokio::test]
async fn env_var_unset_by_default() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "echo_env");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    let msg = match &events[0].kind {
        AgentEventKind::RunStarted { message } => message.clone(),
        other => panic!("expected RunStarted, got: {other:?}"),
    };
    assert!(
        msg.contains("<unset>"),
        "expected <unset> for missing env var, got: {msg}"
    );
}

#[test]
fn sidecar_spec_env_btreemap_is_sorted() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("Z_KEY".into(), "z".into());
    spec.env.insert("A_KEY".into(), "a".into());
    let keys: Vec<_> = spec.env.keys().collect();
    assert_eq!(keys, &["A_KEY", "Z_KEY"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Working directory
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn cwd_passed_to_sidecar() {
    let py = require_python!();
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_string_lossy().into_owned();
    let mut spec = mock_spec_with_mode(&py, "echo_cwd");
    spec.cwd = Some(dir_path.clone());
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    let msg = match &events[0].kind {
        AgentEventKind::RunStarted { message } => message.clone(),
        other => panic!("expected RunStarted, got: {other:?}"),
    };
    // Extract just the directory name for a robust cross-platform check.
    let dir_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(
        msg.contains(&dir_name),
        "expected dir name '{dir_name}' in message, got: {msg}"
    );
}

#[test]
fn process_config_working_dir_default_is_none() {
    let config = ProcessConfig::default();
    assert!(config.working_dir.is_none());
}

#[test]
fn process_config_with_working_dir() {
    let mut config = ProcessConfig::default();
    config.working_dir = Some("/workspace".into());
    assert_eq!(
        config.working_dir.as_ref().unwrap().to_str().unwrap(),
        "/workspace"
    );
}

#[test]
fn process_config_serde_roundtrip() {
    let mut config = ProcessConfig::default();
    config.working_dir = Some("/tmp".into());
    config.timeout = Some(Duration::from_secs(60));
    config.inherit_env = false;
    config.env_vars.insert("KEY".into(), "VALUE".into());
    let json = serde_json::to_string(&config).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.working_dir.unwrap().to_str().unwrap(), "/tmp");
    assert_eq!(back.timeout.unwrap(), Duration::from_secs(60));
    assert!(!back.inherit_env);
    assert_eq!(back.env_vars["KEY"], "VALUE");
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Binary resolution
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_discover_from_dir() {
    let dir = tempfile::tempdir().unwrap();
    let node_dir = dir.path().join("mynode");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// node sidecar").unwrap();

    let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(registry.get("mynode").is_some());
    let cfg = registry.get("mynode").unwrap();
    assert_eq!(cfg.command, "node");
}

#[test]
fn registry_discover_python_host() {
    let dir = tempfile::tempdir().unwrap();
    let py_dir = dir.path().join("mypy");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "# python sidecar").unwrap();

    let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    let cfg = registry.get("mypy").unwrap();
    assert_eq!(cfg.command, "python");
}

#[test]
fn registry_discover_bash_host() {
    let dir = tempfile::tempdir().unwrap();
    let sh_dir = dir.path().join("mysh");
    std::fs::create_dir(&sh_dir).unwrap();
    std::fs::write(sh_dir.join("host.sh"), "#!/bin/bash").unwrap();

    let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    let cfg = registry.get("mysh").unwrap();
    assert_eq!(cfg.command, "bash");
}

#[test]
fn registry_discover_ignores_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("not_a_dir.txt"), "ignored").unwrap();
    let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(registry.list().is_empty());
}

#[test]
fn registry_discover_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(registry.list().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Registration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn register_and_lookup() {
    let mut registry = SidecarRegistry::default();
    let config = SidecarConfig::new("test-sc", "python");
    registry.register(config).unwrap();
    assert!(registry.get("test-sc").is_some());
}

#[test]
fn register_duplicate_fails() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("sc", "python"))
        .unwrap();
    let result = registry.register(SidecarConfig::new("sc", "node"));
    assert!(result.is_err());
}

#[test]
fn register_empty_name_fails() {
    let mut registry = SidecarRegistry::default();
    let result = registry.register(SidecarConfig::new("", "python"));
    assert!(result.is_err());
}

#[test]
fn register_empty_command_fails() {
    let mut registry = SidecarRegistry::default();
    let result = registry.register(SidecarConfig::new("sc", ""));
    assert!(result.is_err());
}

#[test]
fn registry_list_returns_sorted() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("zulu", "python"))
        .unwrap();
    registry
        .register(SidecarConfig::new("alpha", "node"))
        .unwrap();
    registry
        .register(SidecarConfig::new("mike", "bash"))
        .unwrap();
    let names = registry.list();
    assert_eq!(names, &["alpha", "mike", "zulu"]);
}

#[test]
fn registry_remove_existing() {
    let mut registry = SidecarRegistry::default();
    registry
        .register(SidecarConfig::new("sc", "python"))
        .unwrap();
    assert!(registry.remove("sc"));
    assert!(registry.get("sc").is_none());
}

#[test]
fn registry_remove_nonexistent() {
    let mut registry = SidecarRegistry::default();
    assert!(!registry.remove("nope"));
}

#[test]
fn sidecar_config_to_spec() {
    let mut cfg = SidecarConfig::new("test", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("KEY".into(), "VAL".into());
    cfg.working_dir = Some("/tmp".into());

    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env["KEY"], "VAL");
    assert!(spec.cwd.is_some());
}

#[test]
fn sidecar_config_validate_ok() {
    let cfg = SidecarConfig::new("sc", "cmd");
    assert!(cfg.validate().is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Event streaming
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn multi_events_all_received() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5, "expected 5 events, got {}", events.len());
}

#[tokio::test]
async fn multi_event_kinds_parsed() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_event_kinds");
    let client = SidecarClient::spawn(spec).await.unwrap();
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
}

#[tokio::test]
async fn tool_call_events_streamed() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "tool_call_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    // run_started, tool_call, tool_result, run_completed
    assert_eq!(events.len(), 4);
    assert!(matches!(events[1].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(events[2].kind, AgentEventKind::ToolResult { .. }));
}

#[tokio::test]
async fn unicode_content_events() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "unicode_content");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 2);
    match &events[0].kind {
        AgentEventKind::RunStarted { message } => {
            assert!(message.contains("你好世界"));
        }
        other => panic!("expected RunStarted, got: {other:?}"),
    }
}

#[tokio::test]
async fn large_payload_event() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "large_payload");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text.len(), 100_000);
        }
        other => panic!("expected AssistantMessage, got: {other:?}"),
    }
}

#[tokio::test]
async fn empty_lines_ignored() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "empty_lines");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn wrong_ref_id_events_skipped() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "wrong_ref_id");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    // The wrong-ref event is dropped; we get run_started, run_completed (skipping wrong ref)
    assert!(events.len() >= 2, "got {} events", events.len());
}

#[tokio::test]
async fn slow_events_still_arrive() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "slow");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Final / Fatal envelopes
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn fatal_envelope_produces_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "fatal");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap();
    assert!(receipt.is_err());
    let err = receipt.unwrap_err();
    assert!(
        matches!(err, HostError::Fatal(_)),
        "expected Fatal error, got: {err}"
    );
}

#[tokio::test]
async fn fatal_error_message_preserved() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "fatal");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap();
    match receipt.unwrap_err() {
        HostError::Fatal(msg) => {
            assert!(
                msg.contains("something went wrong"),
                "expected error message, got: {msg}"
            );
        }
        err => panic!("expected Fatal, got: {err}"),
    }
}

#[tokio::test]
async fn final_receipt_backend_id() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock-test");
}

#[tokio::test]
async fn final_receipt_outcome_complete() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[test]
fn final_envelope_codec_roundtrip() {
    let run_id = Uuid::new_v4();
    let receipt = test_receipt(run_id);
    let env = Envelope::Final {
        ref_id: run_id.to_string(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Final { .. }));
}

#[test]
fn fatal_envelope_codec_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "test error".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("boom".into());
    assert!(err.to_string().contains("fatal"));
    assert!(err.to_string().contains("boom"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("bad protocol".into());
    assert!(err.to_string().contains("violation"));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Sidecar-kit helpers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kit_frame_hello_roundtrip() {
    use sidecar_kit::{Frame, JsonlCodec};
    let frame = sidecar_kit::hello_frame("test-backend");
    let line = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend["id"], "test-backend");
        }
        other => panic!("expected Hello, got: {other:?}"),
    }
}

#[test]
fn kit_event_builders() {
    let delta = sidecar_kit::event_text_delta("Hello ");
    assert_eq!(delta["type"], "assistant_delta");
    assert_eq!(delta["text"], "Hello ");

    let msg = sidecar_kit::event_text_message("Hello world");
    assert_eq!(msg["type"], "assistant_message");

    let started = sidecar_kit::event_run_started("starting");
    assert_eq!(started["type"], "run_started");

    let completed = sidecar_kit::event_run_completed("done");
    assert_eq!(completed["type"], "run_completed");
}

#[test]
fn kit_event_tool_call_builder() {
    let tc = sidecar_kit::event_tool_call(
        "read_file",
        Some("tc-1"),
        serde_json::json!({"path": "test.txt"}),
    );
    assert_eq!(tc["type"], "tool_call");
    assert_eq!(tc["tool_name"], "read_file");
    assert_eq!(tc["tool_use_id"], "tc-1");
}

#[test]
fn kit_event_tool_result_builder() {
    let tr = sidecar_kit::event_tool_result(
        "read_file",
        Some("tc-1"),
        serde_json::json!({"content": "hello"}),
        false,
    );
    assert_eq!(tr["type"], "tool_result");
    assert!(!tr["is_error"].as_bool().unwrap());
}

#[test]
fn kit_event_error_builder() {
    let e = sidecar_kit::event_error("something broke");
    assert_eq!(e["type"], "error");
    assert_eq!(e["message"], "something broke");
}

#[test]
fn kit_event_warning_builder() {
    let w = sidecar_kit::event_warning("watch out");
    assert_eq!(w["type"], "warning");
    assert_eq!(w["message"], "watch out");
}

#[test]
fn kit_event_file_changed_builder() {
    let fc = sidecar_kit::event_file_changed("test.txt", "created");
    assert_eq!(fc["type"], "file_changed");
    assert_eq!(fc["path"], "test.txt");
}

#[test]
fn kit_event_command_executed_builder() {
    let ce = sidecar_kit::event_command_executed("ls", Some(0), Some("output"));
    assert_eq!(ce["type"], "command_executed");
    assert_eq!(ce["exit_code"], 0);
}

#[test]
fn kit_event_frame_builder() {
    let event = sidecar_kit::event_text_message("hello");
    let frame = sidecar_kit::event_frame("run-1", event);
    match frame {
        sidecar_kit::Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(event["type"], "assistant_message");
        }
        other => panic!("expected Event frame, got: {other:?}"),
    }
}

#[test]
fn kit_fatal_frame_builder() {
    let frame = sidecar_kit::fatal_frame(Some("run-1"), "broken");
    match frame {
        sidecar_kit::Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id.unwrap(), "run-1");
            assert_eq!(error, "broken");
        }
        other => panic!("expected Fatal frame, got: {other:?}"),
    }
}

#[test]
fn kit_fatal_frame_no_ref() {
    let frame = sidecar_kit::fatal_frame(None, "global error");
    match frame {
        sidecar_kit::Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "global error");
        }
        other => panic!("expected Fatal frame, got: {other:?}"),
    }
}

#[test]
fn kit_receipt_builder_complete() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-1", "test-backend")
        .input_tokens(100)
        .output_tokens(50)
        .build();
    assert_eq!(receipt["outcome"], "complete");
    assert_eq!(receipt["backend"]["id"], "test-backend");
    assert_eq!(receipt["usage"]["input_tokens"], 100);
    assert_eq!(receipt["usage"]["output_tokens"], 50);
}

#[test]
fn kit_receipt_builder_failed() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-1", "test-backend")
        .failed()
        .build();
    assert_eq!(receipt["outcome"], "failed");
}

#[test]
fn kit_receipt_builder_partial() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-1", "test-backend")
        .partial()
        .build();
    assert_eq!(receipt["outcome"], "partial");
}

#[test]
fn kit_receipt_builder_with_artifacts() {
    let receipt = sidecar_kit::ReceiptBuilder::new("run-1", "test-backend")
        .artifact("patch", "diff.patch")
        .artifact("log", "run.log")
        .build();
    let arts = receipt["artifacts"].as_array().unwrap();
    assert_eq!(arts.len(), 2);
    assert_eq!(arts[0]["kind"], "patch");
    assert_eq!(arts[1]["path"], "run.log");
}

#[test]
fn kit_protocol_state_happy_path() {
    use sidecar_kit::{Frame, ProtocolPhase, ProtocolState};

    let mut state = ProtocolState::new();
    assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);

    let hello = sidecar_kit::hello_frame("test");
    state.advance(&hello).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::AwaitingRun);

    let run = Frame::Run {
        id: "r1".into(),
        work_order: serde_json::json!({}),
    };
    state.advance(&run).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Streaming);

    let event = sidecar_kit::event_frame("r1", sidecar_kit::event_run_started("go"));
    state.advance(&event).unwrap();
    assert_eq!(state.events_seen(), 1);

    let fin = Frame::Final {
        ref_id: "r1".into(),
        receipt: serde_json::json!({}),
    };
    state.advance(&fin).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Completed);
    assert!(state.is_terminal());
}

#[test]
fn kit_protocol_state_event_before_hello_faults() {
    use sidecar_kit::{ProtocolPhase, ProtocolState};

    let mut state = ProtocolState::new();
    let event = sidecar_kit::event_frame("r1", sidecar_kit::event_run_started("go"));
    let result = state.advance(&event);
    assert!(result.is_err());
    assert_eq!(state.phase(), ProtocolPhase::Faulted);
    assert!(state.fault_reason().is_some());
}

#[test]
fn kit_protocol_state_reset() {
    use sidecar_kit::{ProtocolPhase, ProtocolState};

    let mut state = ProtocolState::new();
    let event = sidecar_kit::event_frame("r1", sidecar_kit::event_run_started("go"));
    let _ = state.advance(&event);
    assert_eq!(state.phase(), ProtocolPhase::Faulted);

    state.reset();
    assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
    assert!(state.fault_reason().is_none());
}

#[test]
fn kit_protocol_state_fatal_during_streaming() {
    use sidecar_kit::{Frame, ProtocolPhase, ProtocolState};

    let mut state = ProtocolState::new();
    state.advance(&sidecar_kit::hello_frame("test")).unwrap();
    state
        .advance(&Frame::Run {
            id: "r1".into(),
            work_order: serde_json::json!({}),
        })
        .unwrap();
    state
        .advance(&sidecar_kit::event_frame(
            "r1",
            sidecar_kit::event_run_started("go"),
        ))
        .unwrap();

    let fatal = Frame::Fatal {
        ref_id: Some("r1".into()),
        error: "crash".into(),
    };
    state.advance(&fatal).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Completed);
}

#[test]
fn kit_cancel_token_default_not_cancelled() {
    let token = sidecar_kit::CancelToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn kit_cancel_token_cancel_signals() {
    let token = sidecar_kit::CancelToken::new();
    let clone = token.clone();
    token.cancel();
    assert!(token.is_cancelled());
    assert!(clone.is_cancelled());
}

#[test]
fn kit_frame_writer_reader_roundtrip() {
    use sidecar_kit::{read_all_frames, write_frames, Frame};

    let frames = vec![
        sidecar_kit::hello_frame("test"),
        sidecar_kit::event_frame("r1", sidecar_kit::event_run_started("hi")),
        Frame::Final {
            ref_id: "r1".into(),
            receipt: serde_json::json!({}),
        },
    ];

    let mut buf = Vec::new();
    write_frames(&mut buf, &frames).unwrap();

    let reader = sidecar_kit::buf_reader_from_bytes(&buf);
    let read_back = read_all_frames(reader).unwrap();
    assert_eq!(read_back.len(), 3);
}

#[test]
fn kit_validate_frame_valid_hello() {
    let frame = sidecar_kit::hello_frame("test");
    let result = sidecar_kit::validate_frame(&frame, 1024 * 1024);
    assert!(result.valid, "issues: {:?}", result.issues);
}

#[test]
fn kit_validate_frame_empty_contract_version() {
    use sidecar_kit::Frame;
    let frame = Frame::Hello {
        contract_version: "".into(),
        backend: serde_json::json!({"id": "test"}),
        capabilities: serde_json::json!({}),
        mode: serde_json::Value::Null,
    };
    let result = sidecar_kit::validate_frame(&frame, 1024 * 1024);
    assert!(!result.valid);
    assert!(result.issues.iter().any(|i| i.contains("contract_version")));
}

#[test]
fn kit_validate_frame_oversized() {
    let frame = sidecar_kit::hello_frame("test");
    let result = sidecar_kit::validate_frame(&frame, 10); // tiny limit
    assert!(!result.valid);
    assert!(result.issues.iter().any(|i| i.contains("exceeds")));
}

#[test]
fn kit_diagnostics_collector() {
    use sidecar_kit::diagnostics::{DiagnosticCollector, DiagnosticLevel};

    let mut collector = DiagnosticCollector::new();
    collector.add_info("SK001", "started processing");
    collector.add_warning("SK002", "slow response");
    collector.add_error("SK003", "connection lost");

    assert_eq!(collector.diagnostics().len(), 3);
    assert!(collector.has_errors());
    assert_eq!(collector.error_count(), 1);

    let summary = collector.summary();
    assert_eq!(summary.info_count, 1);
    assert_eq!(summary.warning_count, 1);
    assert_eq!(summary.error_count, 1);
    assert_eq!(summary.total, 3);

    let errors = collector.by_level(DiagnosticLevel::Error);
    assert_eq!(errors.len(), 1);
}

#[test]
fn kit_diagnostics_clear() {
    use sidecar_kit::diagnostics::DiagnosticCollector;

    let mut collector = DiagnosticCollector::new();
    collector.add_info("SK001", "test");
    collector.clear();
    assert_eq!(collector.diagnostics().len(), 0);
    assert!(!collector.has_errors());
}

// ═══════════════════════════════════════════════════════════════════════
// Additional lifecycle state machine tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_initial_state() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
    assert!(mgr.history().is_empty());
    assert!(mgr.uptime().is_none());
}

#[test]
fn lifecycle_valid_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 6);
}

#[test]
fn lifecycle_invalid_transition() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Running, None);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        LifecycleError::InvalidTransition { .. }
    ));
}

#[test]
fn lifecycle_already_in_state() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Uninitialized, None);
    assert!(matches!(
        result.unwrap_err(),
        LifecycleError::AlreadyInState(_)
    ));
}

#[test]
fn lifecycle_failed_from_any_state() {
    for initial_state in [
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ] {
        let mut mgr = LifecycleManager::new();
        // Transition to the initial state through valid transitions.
        match initial_state {
            LifecycleState::Uninitialized => {}
            LifecycleState::Starting => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
            }
            LifecycleState::Ready => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
            }
            LifecycleState::Running => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Running, None).unwrap();
            }
            LifecycleState::Stopping => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Stopping, None).unwrap();
            }
            _ => {}
        }
        assert!(
            mgr.transition(LifecycleState::Failed, Some("test failure".into()))
                .is_ok(),
            "should transition to Failed from {initial_state}"
        );
    }
}

#[test]
fn lifecycle_uptime_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert!(mgr.uptime().is_some());
}

#[test]
fn lifecycle_transition_reason_preserved() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("init requested".into()))
        .unwrap();
    let h = mgr.history();
    assert_eq!(h[0].reason.as_deref(), Some("init requested"));
}

// ═══════════════════════════════════════════════════════════════════════
// Additional retry and health tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compute_delay_increases_exponentially() {
    let config = RetryConfig {
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let d0 = compute_delay(&config, 0);
    let d1 = compute_delay(&config, 1);
    let d2 = compute_delay(&config, 2);
    assert!(d1 > d0, "d1={d1:?} should be > d0={d0:?}");
    assert!(d2 > d1, "d2={d2:?} should be > d1={d1:?}");
}

#[test]
fn compute_delay_capped_at_max() {
    let config = RetryConfig {
        max_delay: Duration::from_millis(500),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let d10 = compute_delay(&config, 10);
    assert!(d10 <= Duration::from_millis(500));
}

#[test]
fn retry_metadata_to_receipt_metadata() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(250),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(250u64));
}

#[test]
fn retry_config_serde_roundtrip() {
    let cfg = RetryConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_retries, cfg.max_retries);
}

#[test]
fn health_monitor_record_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Healthy,
        Some(Duration::from_millis(10)),
    );
    assert!(monitor.all_healthy());
    assert_eq!(monitor.total_checks(), 1);
}

#[test]
fn health_monitor_unhealthy_tracked() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());
    assert_eq!(monitor.unhealthy_sidecars().len(), 1);
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    let check = monitor.get_status("sc1").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_recovery_resets_failures() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    let check = monitor.get_status("sc1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check("sc1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    let pct = monitor.uptime_percentage("sc1");
    assert!((pct - 75.0).abs() < 0.01);
}

#[test]
fn health_report_overall_status() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc1", HealthStatus::Healthy, None);
    monitor.record_check("sc2", HealthStatus::Healthy, None);
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Healthy));

    monitor.record_check(
        "sc2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// Additional pool tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pool_release_makes_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    assert_eq!(pool.idle_count(), 0);
    pool.release(&entry.id);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_mark_failed() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.mark_failed("s1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.idle, 0);
}

#[test]
fn pool_drain_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.drain("s1");
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_remove_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(pool.total_count(), 0);
}

#[test]
fn pool_stats_utilization() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    let _ = pool.acquire();
    let stats = pool.stats();
    assert!((stats.utilization() - 0.5).abs() < 0.01);
}

#[test]
fn pool_acquire_returns_none_when_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_acquire_returns_none_when_all_busy() {
    let config = PoolConfig {
        max_size: 1,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    let _ = pool.acquire().unwrap();
    assert!(pool.acquire().is_none());
}
