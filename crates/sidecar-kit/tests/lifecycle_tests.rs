// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lifecycle tests for sidecar-kit.
//!
//! Validates the full sidecar lifecycle using `SidecarClient` and `RawRun`:
//! spawn, handshake, event streaming, receipt collection, error handling,
//! cancellation, and resource cleanup.

use serde_json::{Value, json};
use sidecar_kit::{ProcessSpec, SidecarClient, SidecarError};
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn mock_spec(py: &str, mode: &str) -> ProcessSpec {
    let mut spec = ProcessSpec::new(py);
    spec.args = vec![mock_script_path(), mode.to_string()];
    spec
}

// ---------------------------------------------------------------------------
// 1. Full lifecycle: init → process → complete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_full_init_process_complete() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "default"))
        .await
        .expect("spawn should succeed");

    assert_eq!(client.hello.contract_version, "abp/v0.1");
    assert_eq!(client.hello.backend["id"], "mock-lifecycle");

    let run = client
        .run_raw("run-1".into(), json!({"task": "test"}))
        .await
        .expect("run should succeed");

    let (events, result, wait, _cancel) = run.into_parts();
    let events: Vec<Value> = events.collect().await;
    assert_eq!(events.len(), 2, "expected 2 progress events");
    assert_eq!(events[0]["type"], "progress");
    assert_eq!(events[0]["step"], 1);
    assert_eq!(events[1]["step"], 2);

    let receipt = result
        .await
        .expect("receipt channel open")
        .expect("receipt Ok");
    assert_eq!(receipt["status"], "complete");

    wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 2. Error during processing → graceful shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_error_midstream_graceful_shutdown() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "error_midstream"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-err".into(), json!({"task": "will fail"}))
        .await
        .expect("run should start");

    let (events, result, wait, _cancel) = run.into_parts();
    let events: Vec<Value> = events.collect().await;
    assert!(!events.is_empty(), "should receive events before the error");

    let receipt_result = result.await.expect("receipt channel open");
    assert!(receipt_result.is_err(), "should get an error from fatal");
    let err = receipt_result.unwrap_err();
    assert!(
        matches!(err, SidecarError::Fatal(_)),
        "expected Fatal error, got: {err}"
    );
    assert!(err.to_string().contains("processing failed"));

    wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 3. Large event streams
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_large_event_stream() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "large_stream"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-large".into(), json!({"task": "large"}))
        .await
        .expect("run should succeed");

    let (events, result, wait, _cancel) = run.into_parts();
    let events: Vec<Value> = events.collect().await;
    assert_eq!(events.len(), 100, "expected 100 events");

    // Verify ordering.
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event["index"], i as u64, "event {i} has wrong index");
    }

    let receipt = result
        .await
        .expect("receipt channel open")
        .expect("receipt Ok");
    assert_eq!(receipt["status"], "complete");

    wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 4. Empty work order — no events, just receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_empty_work_order() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "empty_work_order"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-empty".into(), json!({}))
        .await
        .expect("run should succeed");

    let (events, result, wait, _cancel) = run.into_parts();
    let events: Vec<Value> = events.collect().await;
    assert!(
        events.is_empty(),
        "empty work order should produce no events"
    );

    let receipt = result
        .await
        .expect("receipt channel open")
        .expect("receipt Ok");
    assert_eq!(receipt["status"], "complete");

    wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 5. Tool call handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_tool_call_handling() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "tool_call"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-tool".into(), json!({"task": "use tools"}))
        .await
        .expect("run should succeed");

    let (events, result, wait, _cancel) = run.into_parts();
    let events: Vec<Value> = events.collect().await;
    assert_eq!(events.len(), 2, "expected tool_call + tool_result events");

    assert_eq!(events[0]["type"], "tool_call");
    assert_eq!(events[0]["tool"], "read_file");
    assert_eq!(events[0]["args"]["path"], "test.txt");

    assert_eq!(events[1]["type"], "tool_result");
    assert_eq!(events[1]["tool"], "read_file");
    assert_eq!(events[1]["result"], "file contents");

    let receipt = result
        .await
        .expect("receipt channel open")
        .expect("receipt Ok");
    assert_eq!(receipt["status"], "complete");

    wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 6. Multiple sequential work orders (reusing same process)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_multiple_sequential_work_orders() {
    let py = require_python!();

    // The multi_run mock handles 3 sequential runs in one process.
    // But SidecarClient consumes self on run_raw, so we test sequential spawns.
    for i in 0..3 {
        let client = SidecarClient::spawn(mock_spec(&py, "default"))
            .await
            .unwrap_or_else(|e| panic!("spawn #{i} failed: {e}"));

        assert_eq!(client.hello.backend["id"], "mock-lifecycle");

        let run = client
            .run_raw(format!("run-seq-{i}"), json!({"task": format!("task-{i}")}))
            .await
            .unwrap_or_else(|e| panic!("run #{i} failed: {e}"));

        let (events, result, wait, _cancel) = run.into_parts();
        let events: Vec<Value> = events.collect().await;
        assert!(!events.is_empty(), "run #{i} should produce events");

        let receipt = result
            .await
            .expect("receipt channel open")
            .expect("receipt Ok");
        assert_eq!(receipt["status"], "complete");

        wait.await.unwrap().unwrap();
    }
}

// ---------------------------------------------------------------------------
// 7. Timeout behavior — slow sidecar still completes within timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_timeout_slow_sidecar() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "slow"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-slow".into(), json!({"task": "slow"}))
        .await
        .expect("run should succeed");

    let (events, result, wait, _cancel) = run.into_parts();

    // Should complete within a reasonable timeout.
    let events: Vec<Value> = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        events.collect::<Vec<_>>(),
    )
    .await
    .expect("events should arrive within timeout");

    assert_eq!(events.len(), 3, "expected 3 slow events");
    assert_eq!(events[0]["step"], "start");
    assert_eq!(events[1]["step"], "middle");
    assert_eq!(events[2]["step"], "end");

    let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), result)
        .await
        .expect("receipt should arrive within timeout")
        .expect("receipt channel open")
        .expect("receipt Ok");
    assert_eq!(receipt["status"], "complete");

    wait.await.unwrap().unwrap();
}

// ---------------------------------------------------------------------------
// 8. Resource cleanup — crash mid-stream, verify no hang
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_resource_cleanup_on_crash() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "crash"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-crash".into(), json!({"task": "crash"}))
        .await
        .expect("run should start");

    let (events, result, wait, _cancel) = run.into_parts();

    // Events before the crash should still arrive.
    let events: Vec<Value> = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        events.collect::<Vec<_>>(),
    )
    .await
    .expect("events collection should not hang");
    assert!(
        !events.is_empty(),
        "should receive at least one event before crash"
    );

    // Receipt should be an error (sidecar exited without final).
    let receipt_result = tokio::time::timeout(std::time::Duration::from_secs(5), result)
        .await
        .expect("receipt should arrive within timeout")
        .expect("receipt channel open");
    assert!(
        receipt_result.is_err(),
        "should get error when sidecar crashes"
    );

    // Wait should complete without hanging.
    tokio::time::timeout(std::time::Duration::from_secs(5), wait)
        .await
        .expect("wait should complete within timeout")
        .unwrap()
        .unwrap();
}

// ---------------------------------------------------------------------------
// 9. Cancel token stops the run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_cancel_stops_run() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "slow"))
        .await
        .expect("spawn should succeed");

    let run = client
        .run_raw("run-cancel".into(), json!({"task": "cancel me"}))
        .await
        .expect("run should succeed");

    let (mut events, _result, wait, cancel) = run.into_parts();

    // Wait for first event, then cancel.
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), events.next())
        .await
        .expect("first event should arrive")
        .expect("should have at least one event");
    assert_eq!(first["step"], "start");

    cancel.cancel();

    // Wait handle should complete after cancellation.
    tokio::time::timeout(std::time::Duration::from_secs(10), wait)
        .await
        .expect("wait should complete after cancel")
        .unwrap()
        .unwrap();
}

// ---------------------------------------------------------------------------
// 10. Spawn failure does not prevent subsequent spawn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_spawn_failure_then_success() {
    let py = require_python!();

    let bad_spec = ProcessSpec::new("nonexistent-binary-lifecycle-test-xyz");
    let result = SidecarClient::spawn(bad_spec).await;
    assert!(result.is_err(), "bad spec should fail to spawn");

    let client = SidecarClient::spawn(mock_spec(&py, "default"))
        .await
        .expect("valid spec should succeed after prior failure");
    assert_eq!(client.hello.backend["id"], "mock-lifecycle");
}

// ---------------------------------------------------------------------------
// 11. HelloData typed extraction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_hello_data_typed_extraction() {
    let py = require_python!();
    let client = SidecarClient::spawn(mock_spec(&py, "default"))
        .await
        .expect("spawn should succeed");

    #[derive(Debug, serde::Deserialize)]
    struct Backend {
        id: String,
        version: String,
    }

    let backend: Backend = client
        .hello
        .backend_as()
        .expect("should deserialize backend");
    assert_eq!(backend.id, "mock-lifecycle");
    assert_eq!(backend.version, "0.1");
}
