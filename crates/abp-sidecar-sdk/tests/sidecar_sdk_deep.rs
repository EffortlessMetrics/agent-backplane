#![allow(clippy::all)]
#![allow(clippy::useless_vec)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for abp-sidecar-sdk: builder, emitter, runtime, registration, and edge cases.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, ExecutionMode, Outcome,
    SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_error::ErrorCode;
use abp_protocol::{Envelope, JsonlCodec};
use abp_sidecar_sdk::builder::{SidecarBuilder, SidecarError};
use abp_sidecar_sdk::emitter::{EmitError, EventEmitter};
use tokio::io::BufReader;

// ═══════════════════════════════════════════════════════════════════════════
// Helper: a trivial run handler that completes immediately.
// ═══════════════════════════════════════════════════════════════════════════

fn noop_handler() -> SidecarBuilder {
    SidecarBuilder::new("test").on_run(|_wo, emitter| async move {
        Ok(emitter.finish("test"))
    })
}

fn make_run_input(task: &str) -> (String, WorkOrder) {
    let wo = WorkOrderBuilder::new(task).build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    };
    (JsonlCodec::encode(&env).unwrap(), wo)
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 1 — SidecarBuilder: construction, setters, accessors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn builder_new_sets_name_string() {
    let b = SidecarBuilder::new(String::from("owned-name"));
    assert_eq!(b.name(), "owned-name");
}

#[test]
fn builder_new_sets_name_str() {
    let b = SidecarBuilder::new("borrowed-name");
    assert_eq!(b.name(), "borrowed-name");
}

#[test]
fn builder_defaults_no_version() {
    let b = SidecarBuilder::new("x");
    assert_eq!(b.backend_version(), None);
}

#[test]
fn builder_defaults_no_adapter_version() {
    let b = SidecarBuilder::new("x");
    assert_eq!(b.adapter_version_str(), None);
}

#[test]
fn builder_defaults_empty_capabilities() {
    let b = SidecarBuilder::new("x");
    assert!(b.capability_manifest().is_empty());
}

#[test]
fn builder_defaults_mapped_mode() {
    let b = SidecarBuilder::new("x");
    assert_eq!(b.execution_mode(), ExecutionMode::Mapped);
}

#[test]
fn builder_defaults_no_handler() {
    let b = SidecarBuilder::new("x");
    assert!(!b.has_handler());
}

#[test]
fn builder_version_sets_backend_version() {
    let b = SidecarBuilder::new("x").version("2.3.4");
    assert_eq!(b.backend_version(), Some("2.3.4"));
}

#[test]
fn builder_version_owned_string() {
    let b = SidecarBuilder::new("x").version(String::from("owned"));
    assert_eq!(b.backend_version(), Some("owned"));
}

#[test]
fn builder_adapter_version_sets() {
    let b = SidecarBuilder::new("x").adapter_version("0.9.1");
    assert_eq!(b.adapter_version_str(), Some("0.9.1"));
}

#[test]
fn builder_adapter_version_owned_string() {
    let b = SidecarBuilder::new("x").adapter_version(String::from("av"));
    assert_eq!(b.adapter_version_str(), Some("av"));
}

#[test]
fn builder_set_passthrough_mode() {
    let b = SidecarBuilder::new("x").mode(ExecutionMode::Passthrough);
    assert_eq!(b.execution_mode(), ExecutionMode::Passthrough);
}

#[test]
fn builder_set_mapped_mode_explicit() {
    let b = SidecarBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .mode(ExecutionMode::Mapped);
    assert_eq!(b.execution_mode(), ExecutionMode::Mapped);
}

// ── Capability manipulation ────────────────────────────────────────────

#[test]
fn builder_add_single_capability() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native);
    let manifest = b.capability_manifest();
    assert_eq!(manifest.len(), 1);
    assert!(matches!(
        manifest.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn builder_add_multiple_capabilities_chained() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Emulated)
        .capability(Capability::Vision, SupportLevel::Unsupported);
    assert_eq!(b.capability_manifest().len(), 3);
}

#[test]
fn builder_overwrite_capability_same_key() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::Streaming, SupportLevel::Emulated);
    assert!(matches!(
        b.capability_manifest().get(&Capability::Streaming),
        Some(SupportLevel::Emulated)
    ));
    assert_eq!(b.capability_manifest().len(), 1);
}

#[test]
fn builder_replace_capabilities_clears_previous() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Audio, SupportLevel::Native);

    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .capabilities(caps);

    assert_eq!(b.capability_manifest().len(), 1);
    assert!(b.capability_manifest().contains_key(&Capability::Audio));
    assert!(!b.capability_manifest().contains_key(&Capability::Streaming));
}

#[test]
fn builder_replace_with_empty_capabilities() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capabilities(CapabilityManifest::new());
    assert!(b.capability_manifest().is_empty());
}

#[test]
fn builder_many_capabilities() {
    let caps = vec![
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::Vision, SupportLevel::Emulated),
        (Capability::Audio, SupportLevel::Unsupported),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolEdit, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Emulated),
        (Capability::ExtendedThinking, SupportLevel::Native),
        (Capability::McpClient, SupportLevel::Emulated),
    ];
    let mut b = SidecarBuilder::new("x");
    for (cap, level) in &caps {
        b = b.capability(cap.clone(), level.clone());
    }
    assert_eq!(b.capability_manifest().len(), caps.len());
}

// ── Handler registration ───────────────────────────────────────────────

#[test]
fn builder_on_run_sets_handler() {
    let b = noop_handler();
    assert!(b.has_handler());
}

#[test]
fn builder_on_run_replaces_handler() {
    let b = SidecarBuilder::new("x")
        .on_run(|_wo, emitter| async move { Ok(emitter.finish("first")) })
        .on_run(|_wo, emitter| async move { Ok(emitter.finish("second")) });
    assert!(b.has_handler());
}

// ── Chaining ──────────────────────────────────────────────────────────

#[test]
fn builder_full_chain() {
    let b = SidecarBuilder::new("full")
        .version("1.0.0")
        .adapter_version("0.5.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, emitter| async move { Ok(emitter.finish("full")) });

    assert_eq!(b.name(), "full");
    assert_eq!(b.backend_version(), Some("1.0.0"));
    assert_eq!(b.adapter_version_str(), Some("0.5.0"));
    assert_eq!(b.capability_manifest().len(), 2);
    assert_eq!(b.execution_mode(), ExecutionMode::Passthrough);
    assert!(b.has_handler());
}

#[test]
fn builder_chain_order_independent() {
    // version → adapter_version → capability → mode → handler
    let b1 = SidecarBuilder::new("x")
        .version("1.0")
        .adapter_version("0.1")
        .capability(Capability::Streaming, SupportLevel::Native)
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, e| async move { Ok(e.finish("x")) });

    // handler → mode → capability → adapter_version → version
    let b2 = SidecarBuilder::new("x")
        .on_run(|_wo, e| async move { Ok(e.finish("x")) })
        .mode(ExecutionMode::Passthrough)
        .capability(Capability::Streaming, SupportLevel::Native)
        .adapter_version("0.1")
        .version("1.0");

    assert_eq!(b1.name(), b2.name());
    assert_eq!(b1.backend_version(), b2.backend_version());
    assert_eq!(b1.adapter_version_str(), b2.adapter_version_str());
    assert_eq!(b1.execution_mode(), b2.execution_mode());
    assert_eq!(b1.capability_manifest().len(), b2.capability_manifest().len());
    assert!(b1.has_handler());
    assert!(b2.has_handler());
}

// ── Identity ───────────────────────────────────────────────────────────

#[test]
fn builder_identity_full() {
    let b = SidecarBuilder::new("my-backend")
        .version("3.0.0")
        .adapter_version("1.2.0");
    let id = b.identity();
    assert_eq!(id.id, "my-backend");
    assert_eq!(id.backend_version.as_deref(), Some("3.0.0"));
    assert_eq!(id.adapter_version.as_deref(), Some("1.2.0"));
}

#[test]
fn builder_identity_minimal() {
    let id = SidecarBuilder::new("min").identity();
    assert_eq!(id.id, "min");
    assert_eq!(id.backend_version, None);
    assert_eq!(id.adapter_version, None);
}

#[test]
fn builder_identity_matches_build_runtime_identity() {
    let b = SidecarBuilder::new("match-test")
        .version("1.0")
        .adapter_version("0.2")
        .on_run(|_wo, e| async move { Ok(e.finish("match-test")) });
    let id_before = b.identity();
    let rt = b.build().unwrap();
    let id_after = rt.identity();
    assert_eq!(id_before.id, id_after.id);
    assert_eq!(id_before.backend_version, id_after.backend_version);
    assert_eq!(id_before.adapter_version, id_after.adapter_version);
}

// ── Clone ──────────────────────────────────────────────────────────────

#[test]
fn builder_clone_preserves_all_fields() {
    let b = SidecarBuilder::new("clonable")
        .version("1.0")
        .adapter_version("0.1")
        .capability(Capability::Streaming, SupportLevel::Native)
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, e| async move { Ok(e.finish("clonable")) });

    let b2 = b.clone();
    assert_eq!(b2.name(), "clonable");
    assert_eq!(b2.backend_version(), Some("1.0"));
    assert_eq!(b2.adapter_version_str(), Some("0.1"));
    assert_eq!(b2.capability_manifest().len(), 1);
    assert_eq!(b2.execution_mode(), ExecutionMode::Passthrough);
    assert!(b2.has_handler());
}

#[test]
fn builder_clone_is_independent() {
    let b = SidecarBuilder::new("original").version("1.0");
    let b2 = b.clone().version("2.0");
    assert_eq!(b.backend_version(), Some("1.0"));
    assert_eq!(b2.backend_version(), Some("2.0"));
}

// ── Debug ──────────────────────────────────────────────────────────────

#[test]
fn builder_debug_contains_name() {
    let b = SidecarBuilder::new("debug-name");
    let d = format!("{b:?}");
    assert!(d.contains("debug-name"));
}

#[test]
fn builder_debug_contains_struct_name() {
    let d = format!("{:?}", SidecarBuilder::new("x"));
    assert!(d.contains("SidecarBuilder"));
}

#[test]
fn builder_debug_with_handler_shows_placeholder() {
    let b = noop_handler();
    let d = format!("{b:?}");
    assert!(d.contains("..."));
}

#[test]
fn builder_debug_without_handler_shows_none() {
    let b = SidecarBuilder::new("x");
    let d = format!("{b:?}");
    assert!(d.contains("None"));
}

// ── Build validation ──────────────────────────────────────────────────

#[test]
fn builder_build_fails_without_handler() {
    let err = SidecarBuilder::new("x").build().unwrap_err();
    assert!(matches!(err, SidecarError::NoHandler));
}

#[test]
fn builder_build_error_display() {
    let err = SidecarBuilder::new("x").build().unwrap_err();
    assert_eq!(err.to_string(), "no run handler configured");
}

#[test]
fn builder_build_succeeds_minimal() {
    let rt = noop_handler().build();
    assert!(rt.is_ok());
}

#[test]
fn builder_build_succeeds_full() {
    let rt = SidecarBuilder::new("full")
        .version("1.0")
        .adapter_version("0.5")
        .capability(Capability::Streaming, SupportLevel::Native)
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, e| async move { Ok(e.finish("full")) })
        .build();
    assert!(rt.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 2 — SidecarError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_error_no_handler_display() {
    let e = SidecarError::NoHandler;
    assert_eq!(e.to_string(), "no run handler configured");
}

#[test]
fn sidecar_error_io_display() {
    let e = SidecarError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken"));
    assert!(e.to_string().contains("pipe broken"));
}

#[test]
fn sidecar_error_protocol_display() {
    let e = SidecarError::Protocol("bad json".into());
    assert_eq!(e.to_string(), "protocol error: bad json");
}

#[test]
fn sidecar_error_handler_display() {
    let e = SidecarError::Handler("task failed".into());
    assert_eq!(e.to_string(), "handler error: task failed");
}

#[test]
fn sidecar_error_debug_no_handler() {
    let e = SidecarError::NoHandler;
    let d = format!("{e:?}");
    assert!(d.contains("NoHandler"));
}

#[test]
fn sidecar_error_io_from_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let se: SidecarError = io_err.into();
    assert!(matches!(se, SidecarError::Io(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 3 — EventEmitter: construction and event types
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn emitter_new_ref_id() {
    let (em, _rx) = EventEmitter::new("ref-42", 8);
    assert_eq!(em.ref_id(), "ref-42");
}

#[tokio::test]
async fn emitter_new_with_owned_string() {
    let (em, _rx) = EventEmitter::new(String::from("owned-ref"), 8);
    assert_eq!(em.ref_id(), "owned-ref");
}

#[tokio::test]
async fn emitter_from_sender_ref_id() {
    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    let em = EventEmitter::from_sender("sender-ref", tx);
    assert_eq!(em.ref_id(), "sender-ref");
}

#[tokio::test]
async fn emitter_from_sender_can_emit() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let em = EventEmitter::from_sender("s", tx);
    em.emit_text_delta("hi").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { text } if text == "hi"));
}

#[tokio::test]
async fn emitter_text_delta_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_text_delta("chunk").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_text_delta_empty_string() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_text_delta("").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, ""),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_text_delta_unicode() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_text_delta("こんにちは 🌍").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "こんにちは 🌍"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_message_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_message("Hello, world!").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { text } if text == "Hello, world!"));
}

#[tokio::test]
async fn emitter_tool_call_start_fields() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    let input = serde_json::json!({"path": "/tmp/file.rs", "content": "fn main() {}"});
    em.emit_tool_call_start("write_file", "tc-99", input.clone())
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input: got_input,
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(tool_use_id, Some("tc-99".into()));
            assert!(parent_tool_use_id.is_none());
            assert_eq!(got_input, input);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_tool_result_success() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_tool_result("read_file", "tc-1", serde_json::json!("data"), false)
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolResult {
            tool_name,
            tool_use_id,
            output,
            is_error,
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id, Some("tc-1".into()));
            assert_eq!(output, serde_json::json!("data"));
            assert!(!is_error);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_tool_result_error() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_tool_result("bash", "tc-2", serde_json::json!({"error": "boom"}), true)
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_warning_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_warning("rate limit approaching").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::Warning { message } if message == "rate limit approaching"
    ));
}

#[tokio::test]
async fn emitter_error_with_code_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_error(Some(ErrorCode::BackendCrashed), "backend died")
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "backend died");
            assert!(error_code.is_some());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_error_without_code_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_error(None, "generic error").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::Error { error_code, .. } => assert!(error_code.is_none()),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_run_started_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_run_started("initializing").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunStarted { message } if message == "initializing"));
}

#[tokio::test]
async fn emitter_run_completed_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_run_completed("finished").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::RunCompleted { message } if message == "finished"));
}

#[tokio::test]
async fn emitter_file_changed_content() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_file_changed("lib.rs", "refactored module")
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::FileChanged { path, summary } => {
            assert_eq!(path, "lib.rs");
            assert_eq!(summary, "refactored module");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_command_executed_full() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_command_executed("cargo build", Some(0), Some("Compiling..."))
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } => {
            assert_eq!(command, "cargo build");
            assert_eq!(exit_code, Some(0));
            assert_eq!(output_preview, Some("Compiling...".into()));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_command_executed_no_exit_code() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_command_executed("kill -9", None, None)
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::CommandExecuted {
            exit_code,
            output_preview,
            ..
        } => {
            assert!(exit_code.is_none());
            assert!(output_preview.is_none());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// ── Emitter: event timestamp ───────────────────────────────────────────

#[tokio::test]
async fn emitter_events_have_timestamps() {
    let (em, mut rx) = EventEmitter::new("r", 4);
    let before = chrono::Utc::now();
    em.emit_text_delta("t").await.unwrap();
    let after = chrono::Utc::now();
    let ev = rx.recv().await.unwrap();
    assert!(ev.ts >= before);
    assert!(ev.ts <= after);
}

// ── Emitter: finish / finish_failed ────────────────────────────────────

#[tokio::test]
async fn emitter_finish_creates_complete_receipt() {
    let (em, _rx) = EventEmitter::new("r", 4);
    let receipt = em.finish("backend-x");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "backend-x");
}

#[tokio::test]
async fn emitter_finish_failed_creates_failed_receipt() {
    let (em, _rx) = EventEmitter::new("r", 4);
    let receipt = em.finish_failed("backend-y");
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.backend.id, "backend-y");
}

// ── Emitter: closed channel ────────────────────────────────────────────

#[tokio::test]
async fn emitter_closed_channel_text_delta() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(em.emit_text_delta("x").await, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_closed_channel_message() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(em.emit_message("x").await, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_closed_channel_warning() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(em.emit_warning("x").await, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_closed_channel_error() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(em.emit_error(None, "x").await, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_closed_channel_run_started() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(em.emit_run_started("x").await, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_closed_channel_run_completed() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(em.emit_run_completed("x").await, Err(EmitError::ChannelClosed)));
}

#[tokio::test]
async fn emitter_closed_channel_file_changed() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(
        em.emit_file_changed("f", "s").await,
        Err(EmitError::ChannelClosed)
    ));
}

#[tokio::test]
async fn emitter_closed_channel_command_executed() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(
        em.emit_command_executed("c", None, None).await,
        Err(EmitError::ChannelClosed)
    ));
}

#[tokio::test]
async fn emitter_closed_channel_tool_call_start() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(
        em.emit_tool_call_start("t", "id", serde_json::json!(null)).await,
        Err(EmitError::ChannelClosed)
    ));
}

#[tokio::test]
async fn emitter_closed_channel_tool_result() {
    let (em, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(matches!(
        em.emit_tool_result("t", "id", serde_json::json!(null), false).await,
        Err(EmitError::ChannelClosed)
    ));
}

// ── Emitter: clone shares channel ──────────────────────────────────────

#[tokio::test]
async fn emitter_clone_shares_ref_id() {
    let (em, _rx) = EventEmitter::new("shared-ref", 4);
    let em2 = em.clone();
    assert_eq!(em.ref_id(), em2.ref_id());
}

#[tokio::test]
async fn emitter_clone_both_send_to_same_channel() {
    let (em, mut rx) = EventEmitter::new("r", 8);
    let em2 = em.clone();
    em.emit_text_delta("first").await.unwrap();
    em2.emit_text_delta("second").await.unwrap();

    let ev1 = rx.recv().await.unwrap();
    let ev2 = rx.recv().await.unwrap();
    assert!(matches!(ev1.kind, AgentEventKind::AssistantDelta { text } if text == "first"));
    assert!(matches!(ev2.kind, AgentEventKind::AssistantDelta { text } if text == "second"));
}

// ── EmitError ──────────────────────────────────────────────────────────

#[test]
fn emit_error_display() {
    let e = EmitError::ChannelClosed;
    assert_eq!(e.to_string(), "event channel closed");
}

#[test]
fn emit_error_debug() {
    let e = EmitError::ChannelClosed;
    let d = format!("{e:?}");
    assert!(d.contains("ChannelClosed"));
}

#[test]
fn emit_error_clone() {
    let e = EmitError::ChannelClosed;
    let e2 = e.clone();
    assert_eq!(e.to_string(), e2.to_string());
}

// ── Emitter: multiple events in sequence ───────────────────────────────

#[tokio::test]
async fn emitter_multiple_events_ordering() {
    let (em, mut rx) = EventEmitter::new("r", 16);
    em.emit_run_started("start").await.unwrap();
    em.emit_text_delta("a").await.unwrap();
    em.emit_text_delta("b").await.unwrap();
    em.emit_message("ab").await.unwrap();
    em.emit_run_completed("done").await.unwrap();

    let events: Vec<AgentEvent> = {
        let mut v = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            v.push(ev);
        }
        v
    };

    assert_eq!(events.len(), 5);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(events[1].kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(events[2].kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(events[3].kind, AgentEventKind::AssistantMessage { .. }));
    assert!(matches!(events[4].kind, AgentEventKind::RunCompleted { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 4 — SidecarRuntime: accessors and protocol lifecycle
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn runtime_identity_from_builder() {
    let rt = SidecarBuilder::new("id-test")
        .version("3.0")
        .adapter_version("0.7")
        .on_run(|_wo, e| async move { Ok(e.finish("id-test")) })
        .build()
        .unwrap();
    assert_eq!(rt.identity().id, "id-test");
    assert_eq!(rt.identity().backend_version.as_deref(), Some("3.0"));
    assert_eq!(rt.identity().adapter_version.as_deref(), Some("0.7"));
}

#[tokio::test]
async fn runtime_capabilities_from_builder() {
    let rt = SidecarBuilder::new("cap-rt")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Emulated)
        .on_run(|_wo, e| async move { Ok(e.finish("cap-rt")) })
        .build()
        .unwrap();
    assert_eq!(rt.capabilities().len(), 2);
    assert!(rt.capabilities().contains_key(&Capability::Streaming));
    assert!(rt.capabilities().contains_key(&Capability::ToolUse));
}

#[tokio::test]
async fn runtime_mode_from_builder() {
    let rt = SidecarBuilder::new("m")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, e| async move { Ok(e.finish("m")) })
        .build()
        .unwrap();
    assert_eq!(rt.execution_mode(), ExecutionMode::Passthrough);
}

#[tokio::test]
async fn runtime_debug_impl() {
    let rt = noop_handler().build().unwrap();
    let d = format!("{rt:?}");
    assert!(d.contains("SidecarRuntime"));
    assert!(d.contains("test"));
    assert!(d.contains("<RunHandler>"));
}

// ── Protocol: hello handshake ──────────────────────────────────────────

#[tokio::test]
async fn runtime_sends_hello_on_empty_input() {
    let rt = SidecarBuilder::new("hello-x")
        .version("1.0")
        .on_run(|_wo, e| async move { Ok(e.finish("hello-x")) })
        .build()
        .unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"hello\""));
    assert!(output.contains("hello-x"));
}

#[tokio::test]
async fn runtime_hello_contains_capabilities() {
    let rt = SidecarBuilder::new("hc")
        .capability(Capability::Streaming, SupportLevel::Native)
        .on_run(|_wo, e| async move { Ok(e.finish("hc")) })
        .build()
        .unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("streaming"));
}

#[tokio::test]
async fn runtime_hello_contains_mode() {
    let rt = SidecarBuilder::new("hm")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, e| async move { Ok(e.finish("hm")) })
        .build()
        .unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("passthrough"));
}

#[tokio::test]
async fn runtime_hello_is_valid_json() {
    let rt = noop_handler().build().unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    for line in output.lines() {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(parsed.is_object());
    }
}

// ── Protocol: run → events → final ────────────────────────────────────

#[tokio::test]
async fn runtime_processes_run_envelope() {
    let rt = SidecarBuilder::new("run-proc")
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("processed").await.unwrap();
            Ok(emitter.finish("run-proc"))
        })
        .build()
        .unwrap();

    let (input, _wo) = make_run_input("process this");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"hello\""));
    assert!(output.contains("\"t\":\"event\""));
    assert!(output.contains("processed"));
    assert!(output.contains("\"t\":\"final\""));
}

#[tokio::test]
async fn runtime_final_envelope_contains_receipt() {
    let rt = SidecarBuilder::new("receipt-test")
        .on_run(|_wo, emitter| async move { Ok(emitter.finish("receipt-test")) })
        .build()
        .unwrap();

    let (input, _wo) = make_run_input("receipt task");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let final_line = output
        .lines()
        .find(|l| l.contains("\"t\":\"final\""))
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(final_line).unwrap();
    assert!(parsed["receipt"].is_object());
    assert_eq!(parsed["receipt"]["outcome"], "complete");
}

// ── Protocol: handler error → fatal ────────────────────────────────────

#[tokio::test]
async fn runtime_handler_error_produces_fatal() {
    let rt = SidecarBuilder::new("fatal-x")
        .on_run(|_wo, _em| async move {
            Err(SidecarError::Handler("something broke".into()))
        })
        .build()
        .unwrap();

    let (input, _wo) = make_run_input("fail task");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"fatal\""));
    assert!(output.contains("something broke"));
}

#[tokio::test]
async fn runtime_fatal_envelope_is_valid_json() {
    let rt = SidecarBuilder::new("fatal-json")
        .on_run(|_wo, _em| async move {
            Err(SidecarError::Handler("fail".into()))
        })
        .build()
        .unwrap();

    let (input, _wo) = make_run_input("fail");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    for line in output.lines() {
        let _: serde_json::Value = serde_json::from_str(line).unwrap();
    }
}

// ── Protocol: ignoring non-run envelopes ───────────────────────────────

#[tokio::test]
async fn runtime_ignores_fatal_envelope() {
    let rt = noop_handler().build().unwrap();
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"nope\"}\n";
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"hello\""));
    assert!(!output.contains("\"t\":\"final\""));
}

#[tokio::test]
async fn runtime_ignores_blank_lines() {
    let rt = noop_handler().build().unwrap();
    let input = "\n\n\n";
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1); // only hello
}

#[tokio::test]
async fn runtime_rejects_malformed_json() {
    let rt = noop_handler().build().unwrap();
    let input = "this is not json\n";
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    let result = rt.run_with_io(stdin, &mut stdout).await;
    assert!(result.is_err());
}

// ── Protocol: multiple run envelopes ───────────────────────────────────

#[tokio::test]
async fn runtime_handles_sequential_runs() {
    let rt = SidecarBuilder::new("seq")
        .on_run(|wo, emitter| async move {
            emitter.emit_text_delta(&wo.task).await.unwrap();
            Ok(emitter.finish("seq"))
        })
        .build()
        .unwrap();

    let (input1, _) = make_run_input("first");
    let (input2, _) = make_run_input("second");
    let combined = format!("{}{}", input1, input2);

    let stdin = BufReader::new(combined.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let final_count = output
        .lines()
        .filter(|l| l.contains("\"t\":\"final\""))
        .count();
    assert_eq!(final_count, 2);
}

// ── Protocol: event ordering ───────────────────────────────────────────

#[tokio::test]
async fn runtime_events_before_final() {
    let rt = SidecarBuilder::new("order")
        .on_run(|_wo, emitter| async move {
            emitter.emit_run_started("go").await.unwrap();
            emitter.emit_text_delta("token").await.unwrap();
            emitter.emit_run_completed("done").await.unwrap();
            Ok(emitter.finish("order"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("order test");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    // hello first
    assert!(lines[0].contains("\"t\":\"hello\""));
    // final last
    assert!(lines.last().unwrap().contains("\"t\":\"final\""));
    // events in between
    let event_count = lines
        .iter()
        .filter(|l| l.contains("\"t\":\"event\""))
        .count();
    assert_eq!(event_count, 3);
}

#[tokio::test]
async fn runtime_many_events_streamed() {
    let rt = SidecarBuilder::new("many")
        .on_run(|_wo, emitter| async move {
            for i in 0..20 {
                emitter
                    .emit_text_delta(&format!("token-{i}"))
                    .await
                    .unwrap();
            }
            Ok(emitter.finish("many"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("many events");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let event_count = output
        .lines()
        .filter(|l| l.contains("\"t\":\"event\""))
        .count();
    assert_eq!(event_count, 20);
}

// ── Protocol: run_with_io output is all valid JSONL ────────────────────

#[tokio::test]
async fn runtime_all_output_is_valid_jsonl() {
    let rt = SidecarBuilder::new("jsonl-valid")
        .on_run(|_wo, emitter| async move {
            emitter.emit_run_started("go").await.unwrap();
            emitter.emit_text_delta("data").await.unwrap();
            emitter.emit_warning("careful").await.unwrap();
            emitter
                .emit_tool_call_start("read", "tc-1", serde_json::json!({}))
                .await
                .unwrap();
            emitter
                .emit_tool_result("read", "tc-1", serde_json::json!("ok"), false)
                .await
                .unwrap();
            emitter.emit_run_completed("done").await.unwrap();
            Ok(emitter.finish("jsonl-valid"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("jsonl check");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    for (i, line) in output.lines().enumerate() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "line {i} is not valid JSON: {line}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 5 — Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn builder_empty_name() {
    let b = SidecarBuilder::new("");
    assert_eq!(b.name(), "");
}

#[test]
fn builder_whitespace_name() {
    let b = SidecarBuilder::new("  spaces  ");
    assert_eq!(b.name(), "  spaces  ");
}

#[test]
fn builder_unicode_name() {
    let b = SidecarBuilder::new("日本語-sidecar-🚀");
    assert_eq!(b.name(), "日本語-sidecar-🚀");
}

#[test]
fn builder_empty_version() {
    let b = SidecarBuilder::new("x").version("");
    assert_eq!(b.backend_version(), Some(""));
}

#[test]
fn builder_version_overwrite() {
    let b = SidecarBuilder::new("x").version("1.0").version("2.0");
    assert_eq!(b.backend_version(), Some("2.0"));
}

#[test]
fn builder_adapter_version_overwrite() {
    let b = SidecarBuilder::new("x")
        .adapter_version("0.1")
        .adapter_version("0.2");
    assert_eq!(b.adapter_version_str(), Some("0.2"));
}

#[tokio::test]
async fn emitter_buffer_size_one() {
    let (em, mut rx) = EventEmitter::new("r", 1);
    em.emit_text_delta("only one").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { text } if text == "only one"));
}

#[tokio::test]
async fn emitter_large_text_delta() {
    let large = "x".repeat(100_000);
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_text_delta(&large).await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text.len(), 100_000),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_special_characters_in_text() {
    let special = "line1\nline2\ttab\"quoted\"\\backslash\0null";
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_text_delta(special).await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, special),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn emitter_tool_call_with_complex_json_input() {
    let input = serde_json::json!({
        "nested": {
            "array": [1, 2, {"deep": true}],
            "null_val": null,
            "float": 3.14
        }
    });
    let (em, mut rx) = EventEmitter::new("r", 4);
    em.emit_tool_call_start("complex", "tc-1", input.clone())
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolCall { input: got, .. } => assert_eq!(got, input),
        other => panic!("unexpected: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 6 — Serialization round-trips (SidecarError, Envelope)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_error_std_error_trait() {
    let e: Box<dyn std::error::Error> = Box::new(SidecarError::NoHandler);
    assert!(!e.to_string().is_empty());
}

#[test]
fn sidecar_error_io_source() {
    let io = std::io::Error::new(std::io::ErrorKind::Other, "inner");
    let se = SidecarError::Io(io);
    // The source should propagate
    let source = std::error::Error::source(&se);
    assert!(source.is_some());
}

#[tokio::test]
async fn runtime_hello_envelope_round_trip() {
    let rt = SidecarBuilder::new("rt-test")
        .version("1.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .on_run(|_wo, e| async move { Ok(e.finish("rt-test")) })
        .build()
        .unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let hello_line = output.lines().next().unwrap();
    // Decode the hello envelope
    let decoded = JsonlCodec::decode(hello_line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[tokio::test]
async fn runtime_final_envelope_round_trip() {
    let rt = SidecarBuilder::new("rt-final")
        .on_run(|_wo, e| async move { Ok(e.finish("rt-final")) })
        .build()
        .unwrap();

    let (input, _) = make_run_input("round trip");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let final_line = output
        .lines()
        .find(|l| l.contains("\"t\":\"final\""))
        .unwrap();
    let decoded = JsonlCodec::decode(final_line.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        other => panic!("expected Final, got: {other:?}"),
    }
}

#[tokio::test]
async fn runtime_event_envelope_round_trip() {
    let rt = SidecarBuilder::new("rt-event")
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("roundtrip").await.unwrap();
            Ok(emitter.finish("rt-event"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("event rt");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let event_line = output
        .lines()
        .find(|l| l.contains("\"t\":\"event\""))
        .unwrap();
    let decoded = JsonlCodec::decode(event_line.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantDelta { text } if text == "roundtrip"
            ));
        }
        other => panic!("expected Event, got: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 7 — lib.rs: sidecar_script and registration helpers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_script_joins_path() {
    use std::path::Path;
    let result = abp_sidecar_sdk::sidecar_script(Path::new("/opt/hosts"), "node/index.js");
    assert_eq!(result, Path::new("/opt/hosts/node/index.js"));
}

#[test]
fn sidecar_script_relative_root() {
    use std::path::Path;
    let result = abp_sidecar_sdk::sidecar_script(Path::new("hosts"), "python/main.py");
    assert_eq!(result, Path::new("hosts/python/main.py"));
}

#[test]
fn sidecar_script_with_trailing_separator() {
    use std::path::Path;
    let result = abp_sidecar_sdk::sidecar_script(Path::new("hosts/"), "script.js");
    // Path::join handles trailing separators correctly
    assert!(result.ends_with("script.js"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  SECTION 8 — End-to-end: builder → runtime → full protocol flow
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn e2e_full_lifecycle_with_all_event_types() {
    let rt = SidecarBuilder::new("e2e-all")
        .version("2.0.0")
        .adapter_version("1.0.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .mode(ExecutionMode::Mapped)
        .on_run(|wo, emitter| async move {
            emitter
                .emit_run_started(&format!("handling: {}", wo.task))
                .await
                .unwrap();
            emitter.emit_text_delta("partial ").await.unwrap();
            emitter.emit_text_delta("response").await.unwrap();
            emitter
                .emit_tool_call_start("read_file", "tc-1", serde_json::json!({"path": "f.rs"}))
                .await
                .unwrap();
            emitter
                .emit_tool_result("read_file", "tc-1", serde_json::json!("fn main(){}"), false)
                .await
                .unwrap();
            emitter
                .emit_file_changed("f.rs", "updated")
                .await
                .unwrap();
            emitter
                .emit_command_executed("cargo test", Some(0), Some("pass"))
                .await
                .unwrap();
            emitter.emit_warning("deprecation").await.unwrap();
            emitter.emit_message("Final answer").await.unwrap();
            emitter.emit_run_completed("done").await.unwrap();
            Ok(emitter.finish("e2e-all"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("comprehensive test");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    // First line: hello
    assert!(lines[0].contains("\"t\":\"hello\""));
    // Last line: final
    assert!(lines.last().unwrap().contains("\"t\":\"final\""));
    // 10 events in between
    let event_count = lines
        .iter()
        .filter(|l| l.contains("\"t\":\"event\""))
        .count();
    assert_eq!(event_count, 10);

    // Verify all output is valid JSONL
    for line in &lines {
        let _: serde_json::Value = serde_json::from_str(line).unwrap();
    }
}

#[tokio::test]
async fn e2e_handler_uses_work_order_task() {
    let rt = SidecarBuilder::new("wo-task")
        .on_run(|wo, emitter| async move {
            emitter.emit_message(&wo.task).await.unwrap();
            Ok(emitter.finish("wo-task"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("echo this back");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("echo this back"));
}

#[tokio::test]
async fn e2e_handler_returns_failed_receipt() {
    let rt = SidecarBuilder::new("fail-receipt")
        .on_run(|_wo, emitter| async move {
            emitter.emit_warning("about to fail").await.unwrap();
            Ok(emitter.finish_failed("fail-receipt"))
        })
        .build()
        .unwrap();

    let (input, _) = make_run_input("fail gracefully");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let final_line = output
        .lines()
        .find(|l| l.contains("\"t\":\"final\""))
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(final_line).unwrap();
    assert_eq!(parsed["receipt"]["outcome"], "failed");
}

#[tokio::test]
async fn e2e_no_events_just_receipt() {
    let rt = SidecarBuilder::new("silent")
        .on_run(|_wo, emitter| async move { Ok(emitter.finish("silent")) })
        .build()
        .unwrap();

    let (input, _) = make_run_input("silent run");
    let stdin = BufReader::new(input.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    // hello + final only, no events
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"t\":\"hello\""));
    assert!(lines[1].contains("\"t\":\"final\""));
}

#[tokio::test]
async fn e2e_mixed_run_and_ignored_envelopes() {
    let rt = SidecarBuilder::new("mixed")
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("ok").await.unwrap();
            Ok(emitter.finish("mixed"))
        })
        .build()
        .unwrap();

    // Send a fatal (ignored), then a run
    let (run_input, _) = make_run_input("after ignored");
    let combined = format!(
        "{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"skip me\"}}\n{}",
        run_input
    );

    let stdin = BufReader::new(combined.as_bytes());
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("\"t\":\"hello\""));
    assert!(output.contains("\"t\":\"event\""));
    assert!(output.contains("\"t\":\"final\""));
}

#[tokio::test]
async fn e2e_passthrough_mode_in_hello() {
    let rt = SidecarBuilder::new("pt")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, e| async move { Ok(e.finish("pt")) })
        .build()
        .unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let hello_line = output.lines().next().unwrap();
    let decoded = JsonlCodec::decode(hello_line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got: {other:?}"),
    }
}

#[tokio::test]
async fn e2e_backend_identity_in_hello() {
    let rt = SidecarBuilder::new("bi")
        .version("5.0")
        .adapter_version("2.0")
        .on_run(|_wo, e| async move { Ok(e.finish("bi")) })
        .build()
        .unwrap();

    let stdin = BufReader::new(&b""[..]);
    let mut stdout = Vec::new();
    rt.run_with_io(stdin, &mut stdout).await.unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let hello_line = output.lines().next().unwrap();
    let decoded = JsonlCodec::decode(hello_line.trim()).unwrap();
    match decoded {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "bi");
            assert_eq!(backend.backend_version.as_deref(), Some("5.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("2.0"));
        }
        other => panic!("expected Hello, got: {other:?}"),
    }
}
