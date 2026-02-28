//! Integration tests for abp-host.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal WorkOrder suitable for testing.
fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "test task".into(),
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

/// Returns the path to the mock sidecar Python script.
fn mock_script_path() -> String {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests")
        .join("mock_sidecar.py")
        .to_string_lossy()
        .into_owned()
}

/// Check whether `python3` (or `python` on Windows) is available.
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

// ---------------------------------------------------------------------------
// Unit tests – SidecarSpec
// ---------------------------------------------------------------------------

#[test]
fn sidecar_spec_new_sets_command() {
    let spec = SidecarSpec::new("my-sidecar");
    assert_eq!(spec.command, "my-sidecar");
}

#[test]
fn sidecar_spec_defaults() {
    let spec = SidecarSpec::new("cmd");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_fields_are_settable() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["index.js".into()];
    spec.env.insert("FOO".into(), "bar".into());
    spec.cwd = Some("/tmp".into());

    assert_eq!(spec.args, vec!["index.js"]);
    assert_eq!(spec.env["FOO"], "bar");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp"));
}

// ---------------------------------------------------------------------------
// Unit tests – HostError Display
// ---------------------------------------------------------------------------

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "not found"));
    let msg = err.to_string();
    assert!(msg.contains("spawn"), "got: {msg}");
    assert!(msg.contains("not found"), "got: {msg}");
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(err.to_string().contains("stdout"), "{}", err);
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(err.to_string().contains("stdin"), "{}", err);
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("bad hello".into());
    let msg = err.to_string();
    assert!(msg.contains("violation"), "got: {msg}");
    assert!(msg.contains("bad hello"), "got: {msg}");
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("boom".into());
    let msg = err.to_string();
    assert!(msg.contains("fatal"), "got: {msg}");
    assert!(msg.contains("boom"), "got: {msg}");
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    let msg = err.to_string();
    assert!(msg.contains("exited"), "got: {msg}");
    assert!(msg.contains("1"), "got: {msg}");
}

#[test]
fn host_error_exited_no_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("exited"), "got: {msg}");
    assert!(msg.contains("None"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// Integration tests – require Python
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spawn_receives_hello() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path()];

    let client = SidecarClient::spawn(spec)
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(
        client.hello.backend.backend_version.as_deref(),
        Some("0.1")
    );
}

#[tokio::test]
async fn run_receives_events_and_receipt() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path()];

    let client = SidecarClient::spawn(spec)
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let wo = test_work_order();

    let sidecar_run = client
        .run(run_id.clone(), wo)
        .await
        .expect("run should succeed");

    // Collect all events.
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty(), "should receive at least one event");

    // Receive receipt.
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

    // Wait for the background task to finish.
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn spawn_invalid_command_returns_error() {
    let spec = SidecarSpec::new("nonexistent-binary-xyz-123");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Spawn(_)),
        "expected Spawn error, got: {err}"
    );
}
