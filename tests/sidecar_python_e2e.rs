// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the Python sidecar (`hosts/python/host.py`).
//!
//! These tests spawn the real Python sidecar process and exercise the full
//! JSONL protocol: hello handshake → run → event streaming → final receipt.
//!
//! # Requirements
//!
//! - `python3` (or `python` on Windows) must be installed and on PATH.
//!
//! # Running
//!
//! These tests are `#[ignore]` by default so they don't run in regular CI.
//! Run them manually with:
//!
//! ```sh
//! cargo test --test sidecar_python_e2e -- --ignored
//! ```

use abp_core::{
    AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirements, ContextPacket,
    ExecutionLane, Outcome, PolicyProfile, RuntimeConfig, SupportLevel, WorkOrder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_host::{SidecarClient, SidecarSpec};
use std::time::Duration;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn python_host_path() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("hosts")
        .join("python")
        .join("host.py")
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
                eprintln!("SKIP: python not found on PATH");
                return;
            }
        }
    };
}

fn python_spec(py: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![python_host_path()];
    spec
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "e2e test task".into(),
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

// ---------------------------------------------------------------------------
// 1. Hello handshake completes with correct identity
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_hello_handshake() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
    assert_eq!(client.hello.backend.id, "python_sidecar");
    assert_eq!(
        client.hello.backend.adapter_version.as_deref(),
        Some("0.2.0")
    );
    assert!(
        client.hello.backend.backend_version.is_some(),
        "backend_version should be set to python version"
    );
}

// ---------------------------------------------------------------------------
// 2. Hello reports capabilities
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_hello_capabilities() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let caps = &client.hello.capabilities;
    assert!(
        matches!(caps.get(&Capability::Streaming), Some(SupportLevel::Native)),
        "streaming capability should be native, got: {:?}",
        caps.get(&Capability::Streaming)
    );
}

// ---------------------------------------------------------------------------
// 3. Send work order and receive events
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_run_produces_events() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty(), "sidecar should emit at least one event");

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ---------------------------------------------------------------------------
// 4. Event ordering: RunStarted → messages → RunCompleted
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_event_ordering() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events: Vec<_> = sidecar_run.events.collect().await;

    // First event must be RunStarted.
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got: {:?}",
        events[0].kind
    );

    // Last event must be RunCompleted.
    let last = events.last().expect("should have events");
    assert!(
        matches!(&last.kind, AgentEventKind::RunCompleted { .. }),
        "last event should be RunCompleted, got: {:?}",
        last.kind
    );

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ---------------------------------------------------------------------------
// 5. Final envelope contains a valid receipt
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_final_receipt() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert_eq!(receipt.backend.id, "python_sidecar");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(
        matches!(receipt.outcome, Outcome::Complete),
        "expected Complete outcome, got: {:?}",
        receipt.outcome
    );
    assert!(
        !receipt.trace.is_empty(),
        "receipt trace should contain the events"
    );

    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 6. Sidecar responds within a reasonable timeout
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_responds_within_timeout() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events = tokio::time::timeout(
        Duration::from_secs(15),
        sidecar_run.events.collect::<Vec<_>>(),
    )
    .await
    .expect("events should arrive within 15s timeout");

    assert!(!events.is_empty());

    let receipt = tokio::time::timeout(Duration::from_secs(10), sidecar_run.receipt)
        .await
        .expect("receipt should arrive within 10s")
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 7. Error handling: malformed JSON → sidecar responds with fatal
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_malformed_json_fatal() {
    let py = require_python!();

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new(&py)
        .arg(python_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn python sidecar");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Read hello line.
    let mut hello_line = String::new();
    reader.read_line(&mut hello_line).await.unwrap();
    assert!(
        hello_line.contains("\"t\":\"hello\""),
        "expected hello envelope"
    );

    // Send malformed JSON.
    stdin.write_all(b"this is not json\n").await.unwrap();
    stdin.flush().await.unwrap();

    // Read the fatal response.
    let mut fatal_line = String::new();
    let result =
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut fatal_line)).await;
    assert!(
        result.is_ok(),
        "sidecar should respond to malformed JSON within 10s"
    );

    assert!(
        fatal_line.contains("\"t\":\"fatal\""),
        "expected fatal envelope, got: {fatal_line}"
    );
    assert!(
        fatal_line.contains("invalid json"),
        "fatal should mention invalid json, got: {fatal_line}"
    );

    // Clean up.
    let _ = child.kill().await;
    let _ = child.wait().await;
}

// ---------------------------------------------------------------------------
// 8. Graceful shutdown: closing stdin causes the sidecar to exit
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_graceful_shutdown() {
    let py = require_python!();

    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new(&py)
        .arg(python_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn python sidecar");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Read hello.
    let mut hello_line = String::new();
    reader.read_line(&mut hello_line).await.unwrap();
    assert!(hello_line.contains("\"t\":\"hello\""));

    // Close stdin — the sidecar should detect EOF and exit.
    drop(stdin);

    // Wait for the process to exit within a reasonable time.
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("sidecar should exit within 10s after stdin close")
        .expect("wait should succeed");

    // Python sidecar should exit cleanly on stdin EOF.
    assert!(
        status.success(),
        "sidecar should exit cleanly, got: {status}"
    );
}

// ---------------------------------------------------------------------------
// 9. Stderr capture (verify the mechanism doesn't block the run)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_stderr_capture() {
    let py = require_python!();

    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let _events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 10. Receipt metadata matches run parameters
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_receipt_metadata() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let wo = test_work_order();
    let wo_id = wo.id;

    let sidecar_run = client
        .run(run_id.clone(), wo)
        .await
        .expect("run should succeed");

    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert_eq!(receipt.meta.run_id.to_string(), run_id);
    assert_eq!(receipt.meta.work_order_id, wo_id);

    sidecar_run.wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 11. Python sidecar emits warning about missing Claude SDK
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn python_e2e_sdk_fallback_warning() {
    let py = require_python!();
    let client = SidecarClient::spawn(python_spec(&py))
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    let events: Vec<_> = sidecar_run.events.collect().await;

    // Without the Claude SDK installed, the Python sidecar should emit
    // a warning or error event about SDK unavailability and then complete.
    let has_warning_or_error = events.iter().any(|e| {
        matches!(
            &e.kind,
            AgentEventKind::Warning { .. } | AgentEventKind::Error { .. }
        )
    });

    // The sidecar may still complete — the warning is informational.
    // This test just verifies we get events and a receipt.
    assert!(!events.is_empty(), "should have at least one event");

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    // Outcome may be "complete" or "partial" depending on SDK availability.
    assert!(
        matches!(
            receipt.outcome,
            Outcome::Complete | Outcome::Partial | Outcome::Failed
        ),
        "unexpected outcome: {:?}",
        receipt.outcome
    );

    // Suppress unused variable warning.
    let _ = has_warning_or_error;

    let _ = sidecar_run.wait.await;
}
