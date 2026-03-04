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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the sidecar SDK builder, emitter, and runtime.

use abp_core::{
    AgentEventKind, Capability, CapabilityManifest, ExecutionMode, Outcome, ReceiptBuilder,
    SupportLevel, WorkOrderBuilder,
};
use abp_error::ErrorCode;
use abp_protocol::{Envelope, JsonlCodec};
use abp_sidecar_sdk::builder::{SidecarBuilder, SidecarError};
use abp_sidecar_sdk::emitter::{EmitError, EventEmitter};
use tokio::io::BufReader;

// ── SidecarBuilder tests ────────────────────────────────────────────────

#[test]
fn builder_new_sets_name() {
    let b = SidecarBuilder::new("my-sidecar");
    assert_eq!(b.name(), "my-sidecar");
}

#[test]
fn builder_version() {
    let b = SidecarBuilder::new("s").version("1.0.0");
    assert_eq!(b.backend_version(), Some("1.0.0"));
}

#[test]
fn builder_adapter_version() {
    let b = SidecarBuilder::new("s").adapter_version("0.2.0");
    assert_eq!(b.adapter_version_str(), Some("0.2.0"));
}

#[test]
fn builder_default_mode_is_mapped() {
    let b = SidecarBuilder::new("s");
    assert_eq!(b.execution_mode(), ExecutionMode::Mapped);
}

#[test]
fn builder_set_mode_passthrough() {
    let b = SidecarBuilder::new("s").mode(ExecutionMode::Passthrough);
    assert_eq!(b.execution_mode(), ExecutionMode::Passthrough);
}

#[test]
fn builder_single_capability() {
    let b = SidecarBuilder::new("s").capability(Capability::Streaming, SupportLevel::Native);
    assert!(matches!(
        b.capability_manifest().get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn builder_multiple_capabilities() {
    let b = SidecarBuilder::new("s")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Emulated);
    assert_eq!(b.capability_manifest().len(), 2);
}

#[test]
fn builder_replace_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Vision, SupportLevel::Native);
    let b = SidecarBuilder::new("s")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capabilities(caps);
    assert_eq!(b.capability_manifest().len(), 1);
    assert!(b.capability_manifest().contains_key(&Capability::Vision));
}

#[test]
fn builder_has_handler_false_by_default() {
    let b = SidecarBuilder::new("s");
    assert!(!b.has_handler());
}

#[test]
fn builder_has_handler_after_on_run() {
    let b = SidecarBuilder::new("s").on_run(|_wo, _em| async {
        Ok(ReceiptBuilder::new("s").outcome(Outcome::Complete).build())
    });
    assert!(b.has_handler());
}

#[test]
fn builder_build_fails_without_handler() {
    let result = SidecarBuilder::new("s").build();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), SidecarError::NoHandler));
}

#[test]
fn builder_build_succeeds_with_handler() {
    let result = SidecarBuilder::new("s")
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("s").outcome(Outcome::Complete).build())
        })
        .build();
    assert!(result.is_ok());
}

#[test]
fn builder_identity() {
    let b = SidecarBuilder::new("my-sc")
        .version("1.0.0")
        .adapter_version("0.3.0");
    let id = b.identity();
    assert_eq!(id.id, "my-sc");
    assert_eq!(id.backend_version.as_deref(), Some("1.0.0"));
    assert_eq!(id.adapter_version.as_deref(), Some("0.3.0"));
}

#[test]
fn builder_debug_format() {
    let b = SidecarBuilder::new("test");
    let debug = format!("{b:?}");
    assert!(debug.contains("SidecarBuilder"));
}

#[test]
fn builder_is_clone() {
    let b = SidecarBuilder::new("test")
        .version("1.0")
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("s").outcome(Outcome::Complete).build())
        });
    let b2 = b.clone();
    assert_eq!(b2.name(), "test");
    assert!(b2.has_handler());
}

// ── EventEmitter tests ─────────────────────────────────────────────────

#[tokio::test]
async fn emitter_ref_id() {
    let (emitter, _rx) = EventEmitter::new("run-42", 4);
    assert_eq!(emitter.ref_id(), "run-42");
}

#[tokio::test]
async fn emitter_text_delta() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter.emit_text_delta("hello").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
        other => panic!("unexpected event kind: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_message() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter.emit_message("full message").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { text } if text == "full message"));
}

#[tokio::test]
async fn emitter_tool_call_start() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter
        .emit_tool_call_start("read_file", "tc-1", serde_json::json!({"path": "foo.rs"}))
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id, Some("tc-1".into()));
            assert_eq!(input, serde_json::json!({"path": "foo.rs"}));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_tool_result() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter
        .emit_tool_result("read_file", "tc-1", serde_json::json!("contents"), false)
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolResult {
            tool_name,
            is_error,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert!(!is_error);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_warning() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter.emit_warning("be careful").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Warning { message } if message == "be careful"));
}

#[tokio::test]
async fn emitter_error_with_code() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter
        .emit_error(Some(ErrorCode::BackendTimeout), "timed out")
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "timed out");
            assert!(error_code.is_some());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_error_without_code() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter.emit_error(None, "unknown").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::Error { error_code, .. } => assert!(error_code.is_none()),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_run_started() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter.emit_run_started("beginning").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { message } if message == "beginning"));
}

#[tokio::test]
async fn emitter_run_completed() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter.emit_run_completed("all done").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { message } if message == "all done"));
}

#[tokio::test]
async fn emitter_file_changed() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter
        .emit_file_changed("src/main.rs", "added function")
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::FileChanged { path, summary }
            if path == "src/main.rs" && summary == "added function"
    ));
}

#[tokio::test]
async fn emitter_command_executed() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 4);
    emitter
        .emit_command_executed("cargo test", Some(0), Some("ok"))
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } => {
            assert_eq!(command, "cargo test");
            assert_eq!(exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("ok"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_closed_channel_error() {
    let (emitter, rx) = EventEmitter::new("run-1", 4);
    drop(rx);
    let result = emitter.emit_text_delta("hi").await;
    assert!(matches!(result, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_finish_receipt() {
    let (emitter, _rx) = EventEmitter::new("run-1", 4);
    let receipt = emitter.finish("my-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "my-backend");
}

#[tokio::test]
async fn emitter_finish_failed_receipt() {
    let (emitter, _rx) = EventEmitter::new("run-1", 4);
    let receipt = emitter.finish_failed("my-backend");
    assert_eq!(receipt.outcome, Outcome::Failed);
}

#[tokio::test]
async fn emitter_from_sender() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let emitter = EventEmitter::from_sender("run-x", tx);
    assert_eq!(emitter.ref_id(), "run-x");
    emitter.emit_text_delta("hi").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn emitter_clone_shares_channel() {
    let (emitter, mut rx) = EventEmitter::new("run-1", 8);
    let emitter2 = emitter.clone();
    emitter.emit_text_delta("a").await.unwrap();
    emitter2.emit_text_delta("b").await.unwrap();
    let _ev1 = rx.recv().await.unwrap();
    let _ev2 = rx.recv().await.unwrap();
}

// ── SidecarRuntime integration tests ────────────────────────────────────

#[tokio::test]
async fn runtime_hello_handshake() {
    let runtime = SidecarBuilder::new("hello-test")
        .version("1.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("hello-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();

    // Provide empty stdin (just EOF) — runtime should send hello then stop.
    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();

    runtime.run_with_io(stdin, &mut stdout).await.unwrap();

    // The output should contain a hello envelope.
    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"hello\""));
    assert!(output.contains("hello-test"));
}

#[tokio::test]
async fn runtime_run_handler_called() {
    let runtime = SidecarBuilder::new("run-test")
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("hello world").await.unwrap();
            Ok(ReceiptBuilder::new("run-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();

    // Create a run envelope as input.
    let wo = WorkOrderBuilder::new("test task").build();
    let run_env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let input_line = JsonlCodec::encode(&run_env).unwrap();

    let stdin = BufReader::new(input_line.as_bytes());
    let mut stdout = Vec::new();

    runtime.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    // Should contain hello, event, and final.
    assert!(output.contains("\"t\":\"hello\""));
    assert!(output.contains("\"t\":\"event\""));
    assert!(output.contains("hello world"));
    assert!(output.contains("\"t\":\"final\""));
}

#[tokio::test]
async fn runtime_handler_error_sends_fatal() {
    let runtime = SidecarBuilder::new("fatal-test")
        .on_run(|_wo, _em| async move { Err(SidecarError::Handler("intentional failure".into())) })
        .build()
        .unwrap();

    let wo = WorkOrderBuilder::new("fail").build();
    let run_env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let input_line = JsonlCodec::encode(&run_env).unwrap();

    let stdin = BufReader::new(input_line.as_bytes());
    let mut stdout = Vec::new();

    runtime.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"fatal\""));
    assert!(output.contains("intentional failure"));
}

#[tokio::test]
async fn runtime_identity_accessor() {
    let rt = SidecarBuilder::new("acc-test")
        .version("2.0")
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("acc-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();
    assert_eq!(rt.identity().id, "acc-test");
    assert_eq!(rt.identity().backend_version.as_deref(), Some("2.0"));
}

#[tokio::test]
async fn runtime_capabilities_accessor() {
    let rt = SidecarBuilder::new("cap-test")
        .capability(Capability::ToolUse, SupportLevel::Native)
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("cap-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();
    assert!(rt.capabilities().contains_key(&Capability::ToolUse));
}

#[tokio::test]
async fn runtime_mode_accessor() {
    let rt = SidecarBuilder::new("mode-test")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("mode-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();
    assert_eq!(rt.execution_mode(), ExecutionMode::Passthrough);
}

#[tokio::test]
async fn runtime_ignores_non_run_envelopes() {
    let rt = SidecarBuilder::new("ignore-test")
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("ignore-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();

    // Send a fatal envelope — should be ignored, no crash.
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"nope\"}\n";
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();

    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    // Should only have the hello (the fatal was ignored).
    assert!(output.contains("\"t\":\"hello\""));
    assert!(!output.contains("\"t\":\"final\""));
}

#[tokio::test]
async fn runtime_multiple_events_streamed() {
    let rt = SidecarBuilder::new("multi-test")
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("one").await.unwrap();
            emitter.emit_text_delta("two").await.unwrap();
            emitter.emit_text_delta("three").await.unwrap();
            Ok(ReceiptBuilder::new("multi-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();

    let wo = WorkOrderBuilder::new("multi").build();
    let run_env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let input_line = JsonlCodec::encode(&run_env).unwrap();
    let stdin = BufReader::new(input_line.as_bytes());
    let mut stdout = Vec::new();

    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    // Count event lines.
    let event_count = output
        .lines()
        .filter(|l| l.contains("\"t\":\"event\""))
        .count();
    assert_eq!(event_count, 3);
}

#[tokio::test]
async fn runtime_debug_impl() {
    let rt = SidecarBuilder::new("debug-test")
        .on_run(|_wo, _em| async {
            Ok(ReceiptBuilder::new("debug-test")
                .outcome(Outcome::Complete)
                .build())
        })
        .build()
        .unwrap();
    let debug = format!("{rt:?}");
    assert!(debug.contains("SidecarRuntime"));
    assert!(debug.contains("debug-test"));
}

// ── End-to-end: builder → runtime → protocol ───────────────────────────

#[tokio::test]
async fn end_to_end_sidecar_flow() {
    let rt = SidecarBuilder::new("e2e-sidecar")
        .version("0.1.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .on_run(|wo, emitter| async move {
            emitter
                .emit_run_started(&format!("starting: {}", wo.task))
                .await
                .unwrap();
            emitter.emit_text_delta("Hello, ").await.unwrap();
            emitter.emit_text_delta("world!").await.unwrap();
            emitter.emit_message("Hello, world!").await.unwrap();
            emitter.emit_run_completed("done").await.unwrap();
            Ok(emitter.finish("e2e-sidecar"))
        })
        .build()
        .unwrap();

    let wo = WorkOrderBuilder::new("greet the user").build();
    let run_env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let input_line = JsonlCodec::encode(&run_env).unwrap();
    let stdin = BufReader::new(input_line.as_bytes());
    let mut stdout = Vec::new();

    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    // Line 0: hello
    assert!(lines[0].contains("\"t\":\"hello\""));
    // Lines 1-4: events (run_started, 2x delta, message, run_completed)
    let event_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains("\"t\":\"event\""))
        .collect();
    assert_eq!(event_lines.len(), 5);
    // Last line: final
    assert!(lines.last().unwrap().contains("\"t\":\"final\""));
}
