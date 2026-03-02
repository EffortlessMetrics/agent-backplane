// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the Node.js sidecar (`hosts/node/host.js`).
//!
//! These tests spawn the real Node.js sidecar process and exercise the full
//! JSONL protocol: hello handshake → run → event streaming → final receipt.
//!
//! # Requirements
//!
//! - `node` must be installed and on PATH.
//!
//! # Running
//!
//! These tests are `#[ignore]` by default so they don't run in regular CI.
//! Run them manually with:
//!
//! ```sh
//! cargo test --test sidecar_e2e -- --ignored
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

fn node_host_path() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("hosts")
        .join("node")
        .join("host.js")
        .to_string_lossy()
        .into_owned()
}

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

macro_rules! require_node {
    () => {
        if !node_available() {
            eprintln!("SKIP: node not found on PATH");
            return;
        }
    };
}

fn node_spec() -> SidecarSpec {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec![node_host_path()];
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
async fn node_e2e_hello_handshake() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
    assert_eq!(client.hello.backend.id, "example_node_sidecar");
    assert_eq!(client.hello.backend.adapter_version.as_deref(), Some("0.1"));
    // backend_version is process.version (e.g. "v20.11.0")
    assert!(
        client.hello.backend.backend_version.is_some(),
        "backend_version should be set to node version"
    );
}

// ---------------------------------------------------------------------------
// 2. Hello reports capabilities
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn node_e2e_hello_capabilities() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
        .await
        .expect("spawn should succeed");

    let caps = &client.hello.capabilities;
    // The node sidecar advertises streaming: native.
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
async fn node_e2e_run_produces_events() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
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
async fn node_e2e_event_ordering() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
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

    // Middle events should be AssistantMessage.
    for ev in &events[1..events.len() - 1] {
        assert!(
            matches!(&ev.kind, AgentEventKind::AssistantMessage { .. }),
            "middle events should be AssistantMessage, got: {:?}",
            ev.kind
        );
    }

    let _ = sidecar_run.receipt.await;
    let _ = sidecar_run.wait.await;
}

// ---------------------------------------------------------------------------
// 5. Final envelope contains a valid receipt
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn node_e2e_final_receipt() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    // Drain events so background task reaches the final envelope.
    let _events: Vec<_> = sidecar_run.events.collect().await;

    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel should not be dropped")
        .expect("receipt should be Ok");

    assert_eq!(receipt.backend.id, "example_node_sidecar");
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
async fn node_e2e_responds_within_timeout() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    // The node sidecar is synchronous — everything should complete well within 10s.
    let events = tokio::time::timeout(
        Duration::from_secs(10),
        sidecar_run.events.collect::<Vec<_>>(),
    )
    .await
    .expect("events should arrive within 10s timeout");

    assert!(!events.is_empty());

    let receipt = tokio::time::timeout(Duration::from_secs(5), sidecar_run.receipt)
        .await
        .expect("receipt should arrive within 5s")
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
async fn node_e2e_malformed_json_fatal() {
    require_node!();

    // Spawn node host.js directly via a raw process so we can write bad JSON.
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new("node")
        .arg(node_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn node sidecar");

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
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut fatal_line)).await;
    assert!(
        result.is_ok(),
        "sidecar should respond to malformed JSON within 5s"
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
async fn node_e2e_graceful_shutdown() {
    require_node!();

    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new("node")
        .arg(node_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn node sidecar");

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
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("sidecar should exit within 5s after stdin close")
        .expect("wait should succeed");

    assert!(
        status.success(),
        "sidecar should exit cleanly, got: {status}"
    );
}

// ---------------------------------------------------------------------------
// 9. Stderr capture (the node sidecar doesn't write to stderr by default,
//    but verify the mechanism works — stderr is captured without blocking)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn node_e2e_stderr_capture() {
    require_node!();

    // Use SidecarClient which captures stderr via a background task.
    let client = SidecarClient::spawn(node_spec())
        .await
        .expect("spawn should succeed");

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id.clone(), test_work_order())
        .await
        .expect("run should succeed");

    // Drain events and receipt — if stderr capture blocked, we'd hang here.
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
async fn node_e2e_receipt_metadata() {
    require_node!();
    let client = SidecarClient::spawn(node_spec())
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

    // The node sidecar echoes back the run_id and work_order_id.
    assert_eq!(receipt.meta.run_id.to_string(), run_id);
    assert_eq!(receipt.meta.work_order_id, wo_id);

    sidecar_run.wait.await.unwrap().unwrap();
}
