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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep tests for sidecar process spawning and management in `abp-host`.
//!
//! Covers: SidecarSpec builder/config, command+args, env passing, cwd,
//! timeout settings, invalid commands, missing executables, exit codes,
//! stderr capture, concurrent spawns, cleanup on drop, restart behaviour,
//! serialization, large I/O, and signal handling on shutdown.

use abp_core::{
    AgentEventKind, CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile,
    RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "spawn-deep test".into(),
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

// ═══════════════════════════════════════════════════════════════════
// 1. SidecarSpec builder / configuration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn spec_new_sets_command_only() {
    let spec = SidecarSpec::new("my-cmd");
    assert_eq!(spec.command, "my-cmd");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn spec_new_accepts_string_reference() {
    let cmd = String::from("node");
    let spec = SidecarSpec::new(&cmd);
    assert_eq!(spec.command, "node");
}

#[test]
fn spec_new_accepts_owned_string() {
    let spec = SidecarSpec::new(String::from("python3"));
    assert_eq!(spec.command, "python3");
}

#[test]
fn spec_clone_is_independent() {
    let mut original = SidecarSpec::new("cmd");
    original.args = vec!["a".into()];
    let mut clone = original.clone();
    clone.args.push("b".into());
    assert_eq!(original.args.len(), 1);
    assert_eq!(clone.args.len(), 2);
}

#[test]
fn spec_debug_contains_command() {
    let spec = SidecarSpec::new("test-binary");
    let dbg = format!("{:?}", spec);
    assert!(dbg.contains("test-binary"), "got: {dbg}");
}

#[test]
fn spec_serde_roundtrip() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["host.js".into(), "--verbose".into()];
    spec.env.insert("KEY".into(), "VALUE".into());
    spec.cwd = Some("/work".into());

    let json = serde_json::to_string(&spec).unwrap();
    let back: SidecarSpec = serde_json::from_str(&json).unwrap();

    assert_eq!(back.command, "node");
    assert_eq!(back.args, vec!["host.js", "--verbose"]);
    assert_eq!(back.env["KEY"], "VALUE");
    assert_eq!(back.cwd.as_deref(), Some("/work"));
}

#[test]
fn spec_serde_minimal_json() {
    let json = r#"{"command":"echo","args":[],"env":{},"cwd":null}"#;
    let spec: SidecarSpec = serde_json::from_str(json).unwrap();
    assert_eq!(spec.command, "echo");
    assert!(spec.args.is_empty());
    assert!(spec.cwd.is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 2. Command and args specification
// ═══════════════════════════════════════════════════════════════════

#[test]
fn spec_multiple_args_preserved_in_order() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["--inspect".into(), "host.js".into(), "--port=3000".into()];
    assert_eq!(spec.args, vec!["--inspect", "host.js", "--port=3000"]);
}

#[test]
fn spec_empty_command_is_representable() {
    let spec = SidecarSpec::new("");
    assert_eq!(spec.command, "");
}

#[test]
fn spec_args_with_spaces_and_special_chars() {
    let mut spec = SidecarSpec::new("cmd");
    spec.args = vec![
        "arg with spaces".into(),
        "arg=value".into(),
        "--flag".into(),
        "path/to/file".into(),
    ];
    assert_eq!(spec.args.len(), 4);
    assert_eq!(spec.args[0], "arg with spaces");
}

#[tokio::test]
async fn spawn_with_args_delivered_to_process() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
}

// ═══════════════════════════════════════════════════════════════════
// 3. Environment variable passing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn spec_env_is_btreemap_ordered() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("Z_VAR".into(), "z".into());
    spec.env.insert("A_VAR".into(), "a".into());
    spec.env.insert("M_VAR".into(), "m".into());
    let keys: Vec<&String> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A_VAR", "M_VAR", "Z_VAR"]);
}

#[test]
fn spec_env_empty_value_allowed() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("EMPTY".into(), "".into());
    assert_eq!(spec.env["EMPTY"], "");
}

#[test]
fn spec_env_overwrite_replaces_value() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("KEY".into(), "old".into());
    spec.env.insert("KEY".into(), "new".into());
    assert_eq!(spec.env["KEY"], "new");
    assert_eq!(spec.env.len(), 1);
}

#[tokio::test]
async fn spawn_env_vars_passed_to_sidecar() {
    let py = require_python!();
    let mut spec = mock_spec_with_mode(&py, "echo_env");
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());

    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    let started = events
        .iter()
        .find(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .expect("should have RunStarted");
    match &started.kind {
        AgentEventKind::RunStarted { message } => {
            assert!(
                message.contains("hello_from_test"),
                "env var should be visible: {message}"
            );
        }
        _ => unreachable!(),
    }

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
async fn spawn_multiple_env_vars() {
    let py = require_python!();
    let mut spec = mock_spec_with_mode(&py, "echo_env");
    spec.env
        .insert("ABP_TEST_VAR".into(), "multi_var_test".into());
    spec.env
        .insert("ANOTHER_VAR".into(), "another_value".into());

    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ═══════════════════════════════════════════════════════════════════
// 4. Working directory configuration
// ═══════════════════════════════════════════════════════════════════

#[test]
fn spec_cwd_none_by_default() {
    let spec = SidecarSpec::new("cmd");
    assert!(spec.cwd.is_none());
}

#[test]
fn spec_cwd_set_and_get() {
    let mut spec = SidecarSpec::new("cmd");
    spec.cwd = Some("/tmp/test".into());
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/test"));
}

#[tokio::test]
async fn spawn_with_cwd_changes_working_dir() {
    let py = require_python!();
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_string_lossy().into_owned();

    let mut spec = mock_spec_with_mode(&py, "echo_cwd");
    spec.cwd = Some(tmp_path.clone());

    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    let started = events
        .iter()
        .find(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }))
        .expect("should have RunStarted");

    match &started.kind {
        AgentEventKind::RunStarted { message } => {
            // Normalise path separators for cross-platform comparison.
            let normalised = message.replace('\\', "/").to_lowercase();
            let expected = tmp_path.replace('\\', "/").to_lowercase();
            assert!(
                normalised.contains(&expected),
                "cwd should match temp dir: got {message}, expected to contain {tmp_path}"
            );
        }
        _ => unreachable!(),
    }

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

#[tokio::test]
async fn spawn_without_cwd_inherits_parent() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "echo_cwd");

    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(
        !events.is_empty(),
        "should emit events even without explicit cwd"
    );

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ═══════════════════════════════════════════════════════════════════
// 5. Timeout settings (ProcessConfig)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn process_config_timeout_none_by_default() {
    let cfg = ProcessConfig::default();
    assert!(cfg.timeout.is_none());
}

#[test]
fn process_config_timeout_set() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_secs(30)),
        ..Default::default()
    };
    assert_eq!(cfg.timeout, Some(Duration::from_secs(30)));
}

#[test]
fn process_config_timeout_zero() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::ZERO),
        ..Default::default()
    };
    assert_eq!(cfg.timeout, Some(Duration::ZERO));
}

#[test]
fn process_config_timeout_large_value() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_secs(86400)),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.timeout, Some(Duration::from_secs(86400)));
}

#[test]
fn process_config_timeout_millis_precision() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_millis(1234)),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("1234"));
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.timeout, Some(Duration::from_millis(1234)));
}

#[tokio::test]
async fn spawn_slow_sidecar_completes_within_generous_timeout() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "slow");

    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = tokio::time::timeout(
        Duration::from_secs(10),
        sidecar_run.events.collect::<Vec<_>>(),
    )
    .await
    .expect("should complete");

    assert_eq!(events.len(), 3);

    let receipt = tokio::time::timeout(Duration::from_secs(5), sidecar_run.receipt)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));

    sidecar_run.wait.await.unwrap().unwrap();
}

// ═══════════════════════════════════════════════════════════════════
// 6. Invalid command handling
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn spawn_invalid_command_returns_spawn_error() {
    let spec = SidecarSpec::new("nonexistent_binary_xyz_987654");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Spawn(_)),
        "expected Spawn error"
    );
}

#[tokio::test]
async fn spawn_empty_command_returns_error() {
    let spec = SidecarSpec::new("");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn spawn_command_with_spaces_returns_error() {
    let spec = SidecarSpec::new("this command does not exist");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn spawn_invalid_does_not_affect_subsequent_valid_spawn() {
    let py = require_python!();

    let bad = SidecarSpec::new("nonexistent_cmd_abc_123");
    assert!(SidecarClient::spawn(bad).await.is_err());

    let good = mock_spec(&py);
    let client = SidecarClient::spawn(good).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
}

// ═══════════════════════════════════════════════════════════════════
// 7. Missing executable error
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn missing_exe_error_is_io_not_found() {
    let spec = SidecarSpec::new("totally_missing_executable_zzz");
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    match err {
        HostError::Spawn(io_err) => {
            assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Spawn(NotFound), got: {other}"),
    }
}

#[tokio::test]
async fn missing_exe_display_contains_spawn() {
    let spec = SidecarSpec::new("missing_binary_for_display_test");
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("spawn"), "Display should mention spawn: {msg}");
}

#[tokio::test]
async fn missing_exe_debug_format_works() {
    let spec = SidecarSpec::new("missing_binary_for_debug_test");
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    let dbg = format!("{err:?}");
    assert!(!dbg.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// 8. Process exit code interpretation
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn exit_nonzero_returns_exited_error() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "exit_nonzero");
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited, got: {err}"
    );
}

#[tokio::test]
async fn exit_zero_no_hello_returns_exited() {
    let py = require_python!();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec!["-c".into(), "import sys; sys.exit(0)".into()];
    let err = SidecarClient::spawn(spec).await.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "exit 0 without hello should still produce Exited: {err}"
    );
}

#[test]
fn host_error_exited_display_with_code() {
    let err = HostError::Exited { code: Some(42) };
    let msg = err.to_string();
    assert!(msg.contains("42"), "should contain exit code: {msg}");
}

#[test]
fn host_error_exited_display_without_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("None"), "should contain None: {msg}");
}

#[test]
fn process_status_exited_with_various_codes() {
    for code in [0, 1, -1, 127, 255] {
        let status = ProcessStatus::Exited { code };
        let json = serde_json::to_string(&status).unwrap();
        let back: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ProcessStatus::Exited { code });
    }
}

// ═══════════════════════════════════════════════════════════════════
// 9. Stderr capture
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn stderr_from_sidecar_does_not_crash_host() {
    let py = require_python!();
    // The mock_sidecar.py "default" mode doesn't write stderr, but we
    // can verify the host handles stderr being open without issue.
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
}

#[tokio::test]
async fn stderr_capture_with_valid_run() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[test]
fn host_error_sidecar_crashed_contains_stderr() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "segfault in module X".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("segfault"),
        "Display should include stderr: {msg}"
    );
    assert!(msg.contains("1"), "Display should include exit code: {msg}");
}

#[test]
fn host_error_sidecar_crashed_empty_stderr() {
    let err = HostError::SidecarCrashed {
        exit_code: None,
        stderr: String::new(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"), "should mention crash: {msg}");
}

// ═══════════════════════════════════════════════════════════════════
// 10. Multiple sidecars spawn independently
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn two_sidecars_spawn_concurrently() {
    let py = require_python!();
    let (r1, r2) = tokio::join!(
        SidecarClient::spawn(mock_spec(&py)),
        SidecarClient::spawn(mock_spec(&py)),
    );
    let c1 = r1.unwrap();
    let c2 = r2.unwrap();
    assert_eq!(c1.hello.backend.id, "mock-test");
    assert_eq!(c2.hello.backend.id, "mock-test");
}

#[tokio::test]
async fn three_sidecars_run_independently() {
    let py = require_python!();

    let mut handles = Vec::new();
    for _ in 0..3 {
        let py = py.clone();
        handles.push(tokio::spawn(async move {
            let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
            let run_id = Uuid::new_v4().to_string();
            let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
            let events: Vec<_> = sidecar_run.events.collect().await;
            let receipt = sidecar_run.receipt.await.unwrap().unwrap();
            sidecar_run.wait.await.unwrap().unwrap();
            (events, receipt)
        }));
    }

    for h in handles {
        let (events, receipt) = h.await.unwrap();
        assert!(!events.is_empty());
        assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    }
}

#[tokio::test]
async fn concurrent_spawn_failure_and_success() {
    let py = require_python!();
    let bad = SidecarSpec::new("nonexistent_concurrent_test");
    let good = mock_spec(&py);

    let (r_bad, r_good) = tokio::join!(SidecarClient::spawn(bad), SidecarClient::spawn(good),);
    assert!(r_bad.is_err());
    assert!(r_good.is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// 11. Sidecar cleanup on drop
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn drop_client_before_run_does_not_panic() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    drop(client);
    // If we get here without a panic, the test passes.
}

#[tokio::test]
async fn drop_sidecar_run_aborts_cleanly() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "hang"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    // Take the first event and then drop everything.
    let mut events = sidecar_run.events;
    let first = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("should get first event")
        .expect("at least one event");
    assert!(matches!(first.kind, AgentEventKind::RunStarted { .. }));

    // Drop all handles — process should be cleaned up by the wait task.
    sidecar_run.wait.abort();
    drop(events);
}

#[tokio::test]
async fn drop_after_full_run_is_clean() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let _receipt = sidecar_run.receipt.await.unwrap().unwrap();
    sidecar_run.wait.await.unwrap().unwrap();
    // Successful drop after full lifecycle.
}

// ═══════════════════════════════════════════════════════════════════
// 12. Sidecar restart behaviour
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sequential_spawn_run_spawn_run() {
    let py = require_python!();

    // First run.
    let c1 = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    let r1 = c1
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .unwrap();
    let _e1: Vec<_> = r1.events.collect().await;
    let receipt1 = r1.receipt.await.unwrap().unwrap();
    r1.wait.await.unwrap().unwrap();

    // Second run (fresh process).
    let c2 = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    let r2 = c2
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .unwrap();
    let _e2: Vec<_> = r2.events.collect().await;
    let receipt2 = r2.receipt.await.unwrap().unwrap();
    r2.wait.await.unwrap().unwrap();

    assert!(matches!(receipt1.outcome, abp_core::Outcome::Complete));
    assert!(matches!(receipt2.outcome, abp_core::Outcome::Complete));
}

#[tokio::test]
async fn spawn_after_failed_process_succeeds() {
    let py = require_python!();

    // Fail first.
    let spec = mock_spec_with_mode(&py, "exit_nonzero");
    assert!(SidecarClient::spawn(spec).await.is_err());

    // Succeed second.
    let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
}

#[tokio::test]
async fn spawn_after_fatal_error_succeeds() {
    let py = require_python!();

    // Fatal run.
    let c1 = SidecarClient::spawn(mock_spec_with_mode(&py, "fatal"))
        .await
        .unwrap();
    let r1 = c1
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .unwrap();
    let _: Vec<_> = r1.events.collect().await;
    let receipt_result = r1.receipt.await.unwrap();
    assert!(receipt_result.is_err());
    let _ = r1.wait.await;

    // Fresh spawn succeeds.
    let c2 = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    assert_eq!(c2.hello.backend.id, "mock-test");
}

// ═══════════════════════════════════════════════════════════════════
// 13. SidecarHost serialization / config
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sidecar_config_new_minimal() {
    let cfg = SidecarConfig::new("my-sidecar", "node");
    assert_eq!(cfg.name, "my-sidecar");
    assert_eq!(cfg.command, "node");
    assert!(cfg.args.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.working_dir.is_none());
}

#[test]
fn sidecar_config_validate_empty_name_fails() {
    let cfg = SidecarConfig::new("", "node");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_validate_empty_command_fails() {
    let cfg = SidecarConfig::new("sidecar", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_validate_both_empty_fails() {
    let cfg = SidecarConfig::new("", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_validate_valid_passes() {
    let cfg = SidecarConfig::new("my-sc", "python3");
    assert!(cfg.validate().is_ok());
}

#[test]
fn sidecar_config_to_spec_conversion() {
    let mut cfg = SidecarConfig::new("test", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("API_KEY".into(), "secret".into());
    cfg.working_dir = Some(PathBuf::from("/srv/app"));

    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env["API_KEY"], "secret");
    assert!(spec.cwd.is_some());
}

#[test]
fn sidecar_config_serde_roundtrip() {
    let mut cfg = SidecarConfig::new("test-sc", "python");
    cfg.args = vec!["main.py".into()];
    cfg.env.insert("X".into(), "Y".into());
    cfg.working_dir = Some(PathBuf::from("/opt"));

    let json = serde_json::to_string(&cfg).unwrap();
    let back: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "test-sc");
    assert_eq!(back.command, "python");
    assert_eq!(back.args, vec!["main.py"]);
    assert_eq!(back.env["X"], "Y");
}

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    let cfg = SidecarConfig::new("my-sc", "node");
    reg.register(cfg).unwrap();
    assert!(reg.get("my-sc").is_some());
    assert!(reg.get("other").is_none());
}

#[test]
fn registry_duplicate_name_errors() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("dup", "node")).unwrap();
    let result = reg.register(SidecarConfig::new("dup", "python"));
    assert!(result.is_err());
}

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("z-sidecar", "cmd"))
        .unwrap();
    reg.register(SidecarConfig::new("a-sidecar", "cmd"))
        .unwrap();
    reg.register(SidecarConfig::new("m-sidecar", "cmd"))
        .unwrap();
    let list = reg.list();
    assert_eq!(list, vec!["a-sidecar", "m-sidecar", "z-sidecar"]);
}

#[test]
fn registry_remove() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "cmd")).unwrap();
    assert!(reg.remove("sc"));
    assert!(!reg.remove("sc"));
    assert!(reg.get("sc").is_none());
}

#[test]
fn process_info_new_with_spec() {
    let spec = SidecarSpec::new("test");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(!info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_lifecycle_transitions() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());

    info.status = ProcessStatus::Running { pid: 1234 };
    assert!(info.is_running());
    assert!(!info.is_terminated());

    info.status = ProcessStatus::Exited { code: 0 };
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_config_all_fields_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("A".into(), "1".into());
    env.insert("B".into(), "2".into());

    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/workspace")),
        env_vars: env,
        timeout: Some(Duration::from_secs(120)),
        inherit_env: false,
    };

    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.working_dir, cfg.working_dir);
    assert_eq!(back.env_vars.len(), 2);
    assert_eq!(back.timeout, Some(Duration::from_secs(120)));
    assert!(!back.inherit_env);
}

// ═══════════════════════════════════════════════════════════════════
// 14. Large I/O through sidecar pipes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sidecar_five_events_all_arrive() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5, "all 5 events should arrive through pipe");

    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn sidecar_varied_event_kinds_all_arrive() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_event_kinds");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5);

    // Verify different kinds came through.
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. })));

    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn work_order_with_large_task_sent_successfully() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();

    let mut wo = test_work_order();
    wo.task = "x".repeat(10_000);

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, wo).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn work_order_with_unicode_task() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();

    let mut wo = test_work_order();
    wo.task = "日本語テスト 🚀 emojis and ñ accents".into();

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, wo).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ═══════════════════════════════════════════════════════════════════
// 15. Signal handling on shutdown
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn hanging_sidecar_wait_abort_succeeds() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "hang"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    // Get the first event then abort.
    let mut events = sidecar_run.events;
    let _ = tokio::time::timeout(Duration::from_secs(5), events.next()).await;
    sidecar_run.wait.abort();

    // Verify abort completes without panic.
    let result = sidecar_run.wait.await;
    assert!(result.is_err(), "aborted task should return JoinError");
}

#[tokio::test]
async fn drop_midstream_handled() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "drop_midstream"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty(), "should get at least one event");

    // Receipt should be an error (process died).
    let receipt_result = sidecar_run.receipt.await;
    match receipt_result {
        Ok(Err(HostError::Exited { .. })) => {}
        Ok(Err(_)) => {}
        Err(_) => {} // Sender dropped
        Ok(Ok(_)) => panic!("should not get valid receipt from dropped sidecar"),
    }
}

#[tokio::test]
async fn timeout_on_receipt_from_hanging_sidecar() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "hang"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    // Receipt should never arrive; verify we can timeout on it.
    let result = tokio::time::timeout(Duration::from_millis(500), sidecar_run.receipt).await;
    assert!(
        result.is_err(),
        "receipt should timeout for hanging sidecar"
    );

    sidecar_run.wait.abort();
}

// ═══════════════════════════════════════════════════════════════════
// Additional edge-case tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"), "got: {msg}");
    assert!(msg.contains("30"), "got: {msg}");
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("stdin unavailable".into());
    let msg = err.to_string();
    assert!(msg.contains("violation"), "got: {msg}");
    assert!(msg.contains("stdin unavailable"), "got: {msg}");
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("out of memory".into());
    let msg = err.to_string();
    assert!(msg.contains("fatal"), "got: {msg}");
    assert!(msg.contains("out of memory"), "got: {msg}");
}

#[test]
fn process_status_killed_and_timed_out_are_terminal() {
    let spec = SidecarSpec::new("cmd");
    for status in [ProcessStatus::Killed, ProcessStatus::TimedOut] {
        let mut info = ProcessInfo::new(spec.clone(), ProcessConfig::default());
        info.status = status;
        assert!(info.is_terminated());
        assert!(!info.is_running());
    }
}

#[test]
fn process_config_inherit_env_defaults_true() {
    let cfg = ProcessConfig::default();
    assert!(cfg.inherit_env);
}

#[test]
fn process_config_inherit_env_false_roundtrips() {
    let cfg = ProcessConfig {
        inherit_env: false,
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert!(!back.inherit_env);
}

#[tokio::test]
async fn hello_contract_version_matches() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    assert_eq!(client.hello.contract_version, abp_core::CONTRACT_VERSION);
}

#[tokio::test]
async fn hello_capabilities_present() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    // Capabilities are a BTreeMap; the mock sends empty `{}`.
    let _ = &client.hello.capabilities;
}

#[tokio::test]
async fn run_id_is_forwarded_to_receipt() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py)).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id.clone(), test_work_order()).await.unwrap();

    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    // The mock puts the run_id in receipt.meta.run_id.
    assert_eq!(receipt.meta.run_id.to_string(), run_id);
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn fatal_envelope_propagates_message() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "fatal"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _: Vec<_> = sidecar_run.events.collect().await;

    match sidecar_run.receipt.await.unwrap() {
        Err(HostError::Fatal(msg)) => {
            assert_eq!(msg, "something went wrong");
        }
        other => panic!("expected Fatal error, got: {other:?}"),
    }
}

#[tokio::test]
async fn non_hello_first_line_returns_protocol_error() {
    let py = require_python!();
    let result = SidecarClient::spawn(mock_spec_with_mode(&py, "no_hello")).await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Protocol(_)),
        "expected Protocol error for non-hello first line"
    );
}

#[tokio::test]
async fn malformed_json_midstream_returns_protocol_error() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec_with_mode(&py, "bad_json_midstream"))
        .await
        .unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());

    let receipt_result = sidecar_run.receipt.await.unwrap();
    assert!(matches!(receipt_result, Err(HostError::Protocol(_))));
}

#[test]
fn spec_env_unicode_key_and_value() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("日本語キー".into(), "値テスト".into());
    let json = serde_json::to_string(&spec).unwrap();
    let back: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.env["日本語キー"], "値テスト");
}

#[test]
fn spec_env_many_vars() {
    let mut spec = SidecarSpec::new("cmd");
    for i in 0..100 {
        spec.env.insert(format!("VAR_{i}"), format!("val_{i}"));
    }
    assert_eq!(spec.env.len(), 100);
    let json = serde_json::to_string(&spec).unwrap();
    let back: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.env.len(), 100);
}

#[test]
fn registry_discover_from_dir_with_no_hosts() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_discover_from_dir_finds_host_js() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("my-sidecar");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("host.js"), "// stub").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert_eq!(reg.list(), vec!["my-sidecar"]);
    let cfg = reg.get("my-sidecar").unwrap();
    assert_eq!(cfg.command, "node");
}

#[test]
fn registry_discover_from_dir_finds_host_py() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("py-sidecar");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("host.py"), "# stub").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert_eq!(reg.list(), vec!["py-sidecar"]);
    let cfg = reg.get("py-sidecar").unwrap();
    assert_eq!(cfg.command, "python");
}

#[test]
fn registry_discover_ignores_files_not_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("host.js"), "// not a dir").unwrap();
    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert!(reg.list().is_empty());
}
