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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive end-to-end tests for sidecar process spawning with the
//! Node.js mock sidecar (`hosts/node/host.js`).
//!
//! Exercises the full lifecycle: process spawn → hello handshake → run →
//! event streaming → final receipt, including error paths, concurrency,
//! timeout behavior, receipt hashing, event ordering, policy enforcement,
//! and workspace staging integration.
//!
//! # Requirements
//!
//! - `node` must be installed and on PATH.
//!
//! # Running
//!
//! All tests are `#[ignore = "requires node"]` so they don't run in regular CI.
//!
//! ```sh
//! cargo test --test sidecar_spawn_e2e_tests -- --ignored
//! ```

use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, MinSupport, Outcome,
    PolicyProfile, RuntimeConfig, SupportLevel, WorkOrder, WorkspaceMode, WorkspaceSpec,
    receipt_hash,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use abp_policy::PolicyEngine;
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

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: task.into(),
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

/// Run a complete sidecar lifecycle and return (events, receipt).
async fn run_sidecar(wo: WorkOrder) -> (Vec<AgentEvent>, abp_core::Receipt) {
    let client = SidecarClient::spawn(node_spec())
        .await
        .expect("spawn should succeed");
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, wo).await.expect("run should succeed");
    let events: Vec<_> = sidecar_run.events.collect().await;
    let receipt = sidecar_run
        .receipt
        .await
        .expect("receipt channel ok")
        .expect("receipt ok");
    let _ = sidecar_run.wait.await;
    (events, receipt)
}

// ===========================================================================
// 1. Spawn & hello handshake
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_contract_version() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_backend_id() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert_eq!(client.hello.backend.id, "example_node_sidecar");
}

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_backend_version_present() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert!(
        client.hello.backend.backend_version.is_some(),
        "backend_version should report node version"
    );
}

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_adapter_version() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert_eq!(client.hello.backend.adapter_version.as_deref(), Some("0.1"));
}

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_streaming_native() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert!(matches!(
        client.hello.capabilities.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_tool_read_emulated() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert!(matches!(
        client.hello.capabilities.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
#[ignore = "requires node"]
async fn spawn_hello_tool_write_emulated() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    assert!(matches!(
        client.hello.capabilities.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

// ===========================================================================
// 2. Send work order, receive events and final receipt
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn run_produces_events_and_receipt() {
    require_node!();
    let (events, receipt) = run_sidecar(make_work_order("test task")).await;
    assert!(!events.is_empty(), "should produce at least one event");
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn run_receipt_contains_backend_identity() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("identity check")).await;
    assert_eq!(receipt.backend.id, "example_node_sidecar");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
#[ignore = "requires node"]
async fn run_receipt_work_order_id_matches() {
    require_node!();
    let wo = make_work_order("wo id check");
    let wo_id = wo.id;
    let (_, receipt) = run_sidecar(wo).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
#[ignore = "requires node"]
async fn run_receipt_trace_not_empty() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("trace check")).await;
    assert!(
        !receipt.trace.is_empty(),
        "receipt trace should mirror emitted events"
    );
}

// ===========================================================================
// 3. Various work order configurations
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_workspace_first_lane() {
    require_node!();
    let mut wo = make_work_order("workspace-first test");
    wo.lane = ExecutionLane::WorkspaceFirst;
    let (events, receipt) = run_sidecar(wo).await;
    assert!(!events.is_empty());
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_context_snippets() {
    require_node!();
    let mut wo = make_work_order("context snippet test");
    wo.context = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "Use async/await".into(),
        }],
    };
    let (events, receipt) = run_sidecar(wo).await;
    assert!(!events.is_empty());
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_model_config() {
    require_node!();
    let mut wo = make_work_order("model config test");
    wo.config.model = Some("gpt-4".into());
    wo.config.max_turns = Some(5);
    wo.config.max_budget_usd = Some(1.0);
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_include_exclude_globs() {
    require_node!();
    let mut wo = make_work_order("glob filter test");
    wo.workspace.include = vec!["**/*.rs".into()];
    wo.workspace.exclude = vec!["target/**".into()];
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_vendor_flags() {
    require_node!();
    let mut wo = make_work_order("vendor flags test");
    wo.config
        .vendor
        .insert("custom_key".into(), serde_json::json!("custom_value"));
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_capability_requirements() {
    require_node!();
    let mut wo = make_work_order("cap requirements test");
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ===========================================================================
// 4. Error handling
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn error_malformed_json_produces_fatal() {
    require_node!();
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new("node")
        .arg(node_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Read hello.
    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();
    assert!(hello.contains("\"t\":\"hello\""));

    // Send garbage.
    stdin.write_all(b"NOT_JSON\n").await.unwrap();
    stdin.flush().await.unwrap();

    let mut resp = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut resp))
        .await
        .expect("should respond within 5s")
        .unwrap();

    assert!(
        resp.contains("\"t\":\"fatal\""),
        "expected fatal, got: {resp}"
    );
    assert!(resp.contains("invalid json"));

    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[tokio::test]
#[ignore = "requires node"]
async fn error_spawn_nonexistent_binary() {
    let spec = SidecarSpec::new("nonexistent_binary_that_does_not_exist_12345");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Spawn(_)));
}

#[tokio::test]
#[ignore = "requires node"]
async fn error_empty_json_object() {
    require_node!();
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new("node")
        .arg(node_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();
    assert!(hello.contains("\"t\":\"hello\""));

    // Send valid JSON but not a valid run envelope — the sidecar ignores
    // unknown `t` values, so it just doesn't respond. Verify no crash.
    stdin.write_all(b"{\"t\":\"unknown\"}\n").await.unwrap();
    stdin.flush().await.unwrap();

    // Give it a moment; the sidecar should remain alive.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Close stdin to trigger graceful exit.
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("should exit")
        .unwrap();
    assert!(status.success());
}

// ===========================================================================
// 5. Concurrent sidecar spawns
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn concurrent_spawn_two_sidecars() {
    require_node!();
    let (r1, r2) = tokio::join!(
        SidecarClient::spawn(node_spec()),
        SidecarClient::spawn(node_spec()),
    );
    let c1 = r1.expect("first spawn ok");
    let c2 = r2.expect("second spawn ok");
    assert_eq!(c1.hello.backend.id, "example_node_sidecar");
    assert_eq!(c2.hello.backend.id, "example_node_sidecar");
}

#[tokio::test]
#[ignore = "requires node"]
async fn concurrent_run_three_sidecars() {
    require_node!();
    let futs = (0..3).map(|i| {
        let wo = make_work_order(&format!("concurrent task {i}"));
        async move { run_sidecar(wo).await }
    });
    let results: Vec<_> = futures::future::join_all(futs).await;
    for (events, receipt) in &results {
        assert!(!events.is_empty());
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
#[ignore = "requires node"]
async fn concurrent_five_sidecars_all_complete() {
    require_node!();
    let futs = (0..5).map(|i| {
        let wo = make_work_order(&format!("parallel-{i}"));
        async move { run_sidecar(wo).await }
    });
    let results: Vec<_> = futures::future::join_all(futs).await;
    assert_eq!(results.len(), 5);
    for (_, receipt) in &results {
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

// ===========================================================================
// 6. Timeout / kill behavior
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn timeout_events_arrive_within_deadline() {
    require_node!();
    let client = SidecarClient::spawn(node_spec()).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client
        .run(run_id, make_work_order("timeout test"))
        .await
        .unwrap();

    let events = tokio::time::timeout(
        Duration::from_secs(10),
        sidecar_run.events.collect::<Vec<_>>(),
    )
    .await
    .expect("events within 10s");
    assert!(!events.is_empty());

    let receipt = tokio::time::timeout(Duration::from_secs(5), sidecar_run.receipt)
        .await
        .expect("receipt within 5s")
        .unwrap()
        .unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
#[ignore = "requires node"]
async fn graceful_shutdown_on_stdin_close() {
    require_node!();
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new("node")
        .arg(node_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let mut hello = String::new();
    reader.read_line(&mut hello).await.unwrap();
    assert!(hello.contains("\"t\":\"hello\""));

    drop(stdin);

    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("should exit within 5s")
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
#[ignore = "requires node"]
async fn kill_sidecar_process() {
    require_node!();
    use tokio::process::Command;

    let mut child = Command::new("node")
        .arg(node_host_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    // Force kill without reading.
    child.kill().await.expect("kill should succeed");
    let status = child.wait().await.unwrap();
    assert!(
        !status.success(),
        "killed process should have non-zero exit"
    );
}

// ===========================================================================
// 7. Receipt hash verification
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_hash_is_deterministic() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("hash test")).await;

    let hash1 = receipt_hash(&receipt).unwrap();
    let hash2 = receipt_hash(&receipt).unwrap();
    assert_eq!(
        hash1, hash2,
        "hashing the same receipt must be deterministic"
    );
    assert_eq!(hash1.len(), 64, "SHA-256 hex digest is 64 chars");
}

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_with_hash_populates_field() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("with_hash test")).await;

    // The node sidecar sets receipt_sha256 to null.
    assert!(
        receipt.receipt_sha256.is_none(),
        "sidecar should leave receipt_sha256 null"
    );

    let hashed = receipt.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_hash_self_referential_prevention() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("self-ref check")).await;

    let hash_before = receipt_hash(&receipt).unwrap();
    let hashed = receipt.with_hash().unwrap();
    let hash_after = receipt_hash(&hashed).unwrap();

    assert_eq!(
        hash_before, hash_after,
        "hash must be stable regardless of receipt_sha256 field value"
    );
}

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_hash_differs_for_different_runs() {
    require_node!();
    let (_, r1) = run_sidecar(make_work_order("run A")).await;
    let (_, r2) = run_sidecar(make_work_order("run B")).await;

    // Different run IDs (UUID v4) ⇒ different hashes.
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    assert_ne!(h1, h2, "different runs should produce different hashes");
}

// ===========================================================================
// 8. Event streaming order
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn event_first_is_run_started() {
    require_node!();
    let (events, _) = run_sidecar(make_work_order("order test")).await;
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event must be RunStarted, got: {:?}",
        events[0].kind
    );
}

#[tokio::test]
#[ignore = "requires node"]
async fn event_last_is_run_completed() {
    require_node!();
    let (events, _) = run_sidecar(make_work_order("order test")).await;
    let last = events.last().unwrap();
    assert!(
        matches!(&last.kind, AgentEventKind::RunCompleted { .. }),
        "last event must be RunCompleted, got: {:?}",
        last.kind
    );
}

#[tokio::test]
#[ignore = "requires node"]
async fn event_middle_are_assistant_messages() {
    require_node!();
    let (events, _) = run_sidecar(make_work_order("middle events")).await;
    assert!(
        events.len() >= 3,
        "need at least 3 events (start, msg, end)"
    );
    for ev in &events[1..events.len() - 1] {
        assert!(
            matches!(&ev.kind, AgentEventKind::AssistantMessage { .. }),
            "middle events should be AssistantMessage, got: {:?}",
            ev.kind
        );
    }
}

#[tokio::test]
#[ignore = "requires node"]
async fn event_timestamps_are_monotonic() {
    require_node!();
    let (events, _) = run_sidecar(make_work_order("ts monotonic")).await;
    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "timestamps must be non-decreasing: {:?} < {:?}",
            window[1].ts,
            window[0].ts
        );
    }
}

#[tokio::test]
#[ignore = "requires node"]
async fn event_count_matches_trace() {
    require_node!();
    let (events, receipt) = run_sidecar(make_work_order("count check")).await;
    assert_eq!(
        events.len(),
        receipt.trace.len(),
        "streamed events should match receipt trace length"
    );
}

#[tokio::test]
#[ignore = "requires node"]
async fn event_run_started_message_contains_task() {
    require_node!();
    let task = "unique task for message check";
    let (events, _) = run_sidecar(make_work_order(task)).await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(
            message.contains(task),
            "RunStarted message should contain task, got: {message}"
        );
    } else {
        panic!("first event not RunStarted");
    }
}

#[tokio::test]
#[ignore = "requires node"]
async fn event_assistant_message_mentions_workspace() {
    require_node!();
    let (events, _) = run_sidecar(make_work_order("workspace echo")).await;
    let texts: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("workspace root")),
        "at least one message should mention workspace root"
    );
}

// ===========================================================================
// 9. Policy enforcement through the full pipeline
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn policy_engine_from_work_order_policy() {
    require_node!();
    let mut wo = make_work_order("policy test");
    wo.policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };

    // Verify the policy compiles.
    let engine = PolicyEngine::new(&wo.policy).unwrap();
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_read_path(std::path::Path::new(".env")).allowed);
    assert!(
        !engine
            .can_write_path(std::path::Path::new(".git/config"))
            .allowed
    );

    // The sidecar itself doesn't enforce policy, but the work order round-trips.
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn policy_deny_write_glob() {
    require_node!();
    let policy = PolicyProfile {
        deny_write: vec!["**/secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(
        !engine
            .can_write_path(std::path::Path::new("secrets/key.pem"))
            .allowed
    );
    assert!(
        engine
            .can_write_path(std::path::Path::new("src/main.rs"))
            .allowed
    );

    let mut wo = make_work_order("deny-write policy");
    wo.policy = policy;
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn policy_empty_permits_everything() {
    require_node!();
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("AnyTool").allowed);
    assert!(
        engine
            .can_read_path(std::path::Path::new("any/path"))
            .allowed
    );
    assert!(
        engine
            .can_write_path(std::path::Path::new("any/path"))
            .allowed
    );

    let (_, receipt) = run_sidecar(make_work_order("empty policy")).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ===========================================================================
// 10. Workspace staging integration
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn workspace_passthrough_mode_works() {
    require_node!();
    let mut wo = make_work_order("passthrough ws");
    wo.workspace.mode = WorkspaceMode::PassThrough;
    wo.workspace.root = ".".into();
    let (events, receipt) = run_sidecar(wo).await;
    assert!(!events.is_empty());
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn workspace_staged_mode_round_trips() {
    require_node!();
    let mut wo = make_work_order("staged ws");
    wo.workspace.mode = WorkspaceMode::Staged;
    wo.workspace.root = ".".into();
    wo.workspace.include = vec!["**/*.toml".into()];
    wo.workspace.exclude = vec!["target/**".into()];
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn workspace_manager_prepare_passthrough() {
    require_node!();
    use abp_workspace::WorkspaceManager;

    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let prepared = WorkspaceManager::prepare(&spec).unwrap();
    assert!(prepared.path().exists());

    // Run sidecar with the prepared workspace path.
    let mut wo = make_work_order("prepared ws");
    wo.workspace.root = prepared.path().to_string_lossy().into_owned();
    let (_, receipt) = run_sidecar(wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ===========================================================================
// Additional edge-case and integration tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_outcome_is_complete() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("outcome check")).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_verification_harness_ok() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("harness ok")).await;
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_artifacts_empty_for_mock() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("artifacts check")).await;
    assert!(receipt.artifacts.is_empty());
}

#[tokio::test]
#[ignore = "requires node"]
async fn receipt_usage_raw_contains_note() {
    require_node!();
    let (_, receipt) = run_sidecar(make_work_order("usage raw")).await;
    assert_eq!(
        receipt.usage_raw.get("note").and_then(|v| v.as_str()),
        Some("example_node_sidecar")
    );
}

#[tokio::test]
#[ignore = "requires node"]
async fn sidecar_spec_with_env_vars() {
    require_node!();
    let mut spec = node_spec();
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
#[ignore = "requires node"]
async fn sidecar_spec_with_cwd() {
    require_node!();
    let mut spec = node_spec();
    spec.cwd = Some(".".into());
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "example_node_sidecar");
}

#[tokio::test]
#[ignore = "requires node"]
async fn multiple_sequential_runs_different_sidecars() {
    require_node!();
    for i in 0..3 {
        let (events, receipt) = run_sidecar(make_work_order(&format!("sequential run {i}"))).await;
        assert!(!events.is_empty());
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
#[ignore = "requires node"]
async fn long_task_description() {
    require_node!();
    let long_task = "a".repeat(10_000);
    let (events, receipt) = run_sidecar(make_work_order(&long_task)).await;
    assert!(!events.is_empty());
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn unicode_task_description() {
    require_node!();
    let (events, receipt) = run_sidecar(make_work_order("こんにちは世界 🌍")).await;
    assert!(!events.is_empty());
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
#[ignore = "requires node"]
async fn work_order_with_nil_uuid() {
    require_node!();
    let mut wo = make_work_order("nil uuid");
    wo.id = Uuid::nil();
    let (_, receipt) = run_sidecar(wo).await;
    assert_eq!(receipt.meta.work_order_id, Uuid::nil());
}
