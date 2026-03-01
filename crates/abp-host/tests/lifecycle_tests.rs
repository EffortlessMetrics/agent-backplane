// SPDX-License-Identifier: MIT OR Apache-2.0
//! Process lifecycle tests for abp-host.
//!
//! Validates sidecar spawn/run/exit behaviour, environment propagation,
//! working-directory handling, spec validation, sequential reuse, and
//! registry-to-spawn round-trips.

use abp_core::{
    AgentEventKind, CapabilityRequirements, ContextPacket, ExecutionLane, Outcome, PolicyProfile,
    RuntimeConfig, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::{HostError, SidecarClient, SidecarSpec};
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

// ---------------------------------------------------------------------------
// 1. Normal lifecycle — spawn, run, verify clean exit
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

    // Background task exits cleanly.
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2. Process environment — env vars are forwarded to sidecar
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
// 3. Working directory — cwd is set correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_cwd_is_set() {
    let py = require_python!();
    let tmp = std::env::temp_dir();
    let canonical = tmp
        .canonicalize()
        .expect("temp dir should be canonicalizable");

    let mut spec = mock_spec_with_mode(&py, "echo_cwd");
    spec.cwd = Some(canonical.to_string_lossy().into_owned());

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

    // Extract the reported cwd and canonicalize for cross-platform comparison.
    let reported_cwd = msg
        .strip_prefix("cwd=")
        .expect("message should start with 'cwd='");
    let reported = std::path::Path::new(reported_cwd)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(reported_cwd));

    assert_eq!(
        reported, canonical,
        "sidecar cwd mismatch: got {reported:?}, expected {canonical:?}"
    );

    let _ = sidecar_run.receipt.await;
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 4. SidecarSpec validation — empty command and missing executable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_empty_command_fails() {
    let spec = SidecarSpec::new("");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err(), "empty command should fail to spawn");

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Spawn(_)),
        "expected Spawn error, got: {err}"
    );
}

#[tokio::test]
async fn lifecycle_missing_executable_fails_with_helpful_error() {
    let spec = SidecarSpec::new("nonexistent-binary-abp-test-xyz");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err(), "missing binary should fail");

    let err = result.unwrap_err();
    assert!(
        matches!(&err, HostError::Spawn(_)),
        "expected Spawn error, got: {err}"
    );

    let msg = err.to_string();
    assert!(
        msg.contains("spawn"),
        "error should mention spawning: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 5. Multiple sequential spawns — can spawn, complete, then spawn again
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
// 6. Registry + spawn — register, retrieve spec, spawn it
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
// 7. Registry discovers sidecars from hosts/ directory
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

    // Specs reference the correct interpreter.
    let node_spec = registry.get("node").unwrap();
    assert_eq!(node_spec.command, "node");
    let py_spec = registry.get("python").unwrap();
    assert_eq!(py_spec.command, "python");
}

// ---------------------------------------------------------------------------
// 8. Registry list returns sorted names
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_registry_list_sorted() {
    let mut registry = SidecarRegistry::default();
    // Insert in reverse-alphabetical order.
    registry.register(SidecarConfig::new("zeta", "z")).unwrap();
    registry.register(SidecarConfig::new("alpha", "a")).unwrap();
    registry.register(SidecarConfig::new("mu", "m")).unwrap();

    let names = registry.list();
    assert_eq!(names, vec!["alpha", "mu", "zeta"]);
}
