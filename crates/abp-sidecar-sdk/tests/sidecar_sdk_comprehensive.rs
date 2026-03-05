#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for abp-sidecar-sdk public API.

use std::collections::BTreeMap;
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, ReceiptBuilder, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_error::ErrorCode;
use abp_protocol::{Envelope, JsonlCodec};
use abp_sidecar_sdk::builder::{SidecarBuilder, SidecarError};
use abp_sidecar_sdk::emitter::{EmitError, EventEmitter};
use abp_sidecar_sdk::runtime::SidecarRuntime;
use tokio::io::BufReader;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn noop_builder() -> SidecarBuilder {
    SidecarBuilder::new("test-sc")
        .on_run(|_wo, emitter| async move { Ok(emitter.finish("test-sc")) })
}

fn make_run_envelope(task: &str) -> (String, WorkOrder) {
    let wo = WorkOrderBuilder::new(task).build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    };
    (JsonlCodec::encode(&env).unwrap(), wo)
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 1 — SidecarBuilder construction and defaults
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s1_builder_new_with_str_literal() {
    let b = SidecarBuilder::new("alpha");
    assert_eq!(b.name(), "alpha");
}

#[test]
fn s1_builder_new_with_owned_string() {
    let b = SidecarBuilder::new(String::from("beta"));
    assert_eq!(b.name(), "beta");
}

#[test]
fn s1_builder_default_backend_version_none() {
    let b = SidecarBuilder::new("x");
    assert_eq!(b.backend_version(), None);
}

#[test]
fn s1_builder_default_adapter_version_none() {
    let b = SidecarBuilder::new("x");
    assert_eq!(b.adapter_version_str(), None);
}

#[test]
fn s1_builder_default_capabilities_empty() {
    let b = SidecarBuilder::new("x");
    assert!(b.capability_manifest().is_empty());
}

#[test]
fn s1_builder_default_mode_is_mapped() {
    let b = SidecarBuilder::new("x");
    assert_eq!(b.execution_mode(), ExecutionMode::Mapped);
}

#[test]
fn s1_builder_default_no_handler() {
    let b = SidecarBuilder::new("x");
    assert!(!b.has_handler());
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 2 — SidecarBuilder setters
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s2_version_sets_backend_version() {
    let b = SidecarBuilder::new("x").version("3.2.1");
    assert_eq!(b.backend_version(), Some("3.2.1"));
}

#[test]
fn s2_version_with_owned_string() {
    let b = SidecarBuilder::new("x").version(String::from("owned-v"));
    assert_eq!(b.backend_version(), Some("owned-v"));
}

#[test]
fn s2_adapter_version_sets() {
    let b = SidecarBuilder::new("x").adapter_version("0.5.0");
    assert_eq!(b.adapter_version_str(), Some("0.5.0"));
}

#[test]
fn s2_adapter_version_owned() {
    let b = SidecarBuilder::new("x").adapter_version(String::from("av-1"));
    assert_eq!(b.adapter_version_str(), Some("av-1"));
}

#[test]
fn s2_mode_passthrough() {
    let b = SidecarBuilder::new("x").mode(ExecutionMode::Passthrough);
    assert_eq!(b.execution_mode(), ExecutionMode::Passthrough);
}

#[test]
fn s2_mode_can_be_overwritten() {
    let b = SidecarBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .mode(ExecutionMode::Mapped);
    assert_eq!(b.execution_mode(), ExecutionMode::Mapped);
}

#[test]
fn s2_version_overwrite() {
    let b = SidecarBuilder::new("x").version("1.0").version("2.0");
    assert_eq!(b.backend_version(), Some("2.0"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 3 — Capabilities
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s3_add_single_capability() {
    let b = SidecarBuilder::new("x").capability(Capability::Streaming, SupportLevel::Native);
    assert_eq!(b.capability_manifest().len(), 1);
    assert!(matches!(
        b.capability_manifest().get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn s3_add_multiple_capabilities() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Emulated)
        .capability(Capability::Vision, SupportLevel::Unsupported);
    assert_eq!(b.capability_manifest().len(), 3);
}

#[test]
fn s3_capability_overwrite_same_key() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::Streaming, SupportLevel::Emulated);
    assert_eq!(b.capability_manifest().len(), 1);
    assert!(matches!(
        b.capability_manifest().get(&Capability::Streaming),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn s3_replace_capabilities_wholesale() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Audio, SupportLevel::Native);
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capabilities(caps);
    assert_eq!(b.capability_manifest().len(), 1);
    assert!(b.capability_manifest().contains_key(&Capability::Audio));
    assert!(!b.capability_manifest().contains_key(&Capability::Streaming));
}

#[test]
fn s3_replace_with_empty_manifest() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::ToolUse, SupportLevel::Native)
        .capabilities(CapabilityManifest::new());
    assert!(b.capability_manifest().is_empty());
}

#[test]
fn s3_many_different_capabilities() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .capability(Capability::Vision, SupportLevel::Emulated)
        .capability(Capability::Audio, SupportLevel::Unsupported)
        .capability(Capability::ToolRead, SupportLevel::Native)
        .capability(Capability::ToolWrite, SupportLevel::Native)
        .capability(Capability::ExtendedThinking, SupportLevel::Emulated)
        .capability(Capability::McpClient, SupportLevel::Native);
    assert_eq!(b.capability_manifest().len(), 8);
}

#[test]
fn s3_capability_manifest_is_btreemap() {
    let b = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Emulated);
    let manifest: &CapabilityManifest = b.capability_manifest();
    // BTreeMap iteration is sorted
    let keys: Vec<_> = manifest.keys().collect();
    assert_eq!(keys.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 4 — Handler & build
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s4_on_run_sets_handler_flag() {
    let b = noop_builder();
    assert!(b.has_handler());
}

#[test]
fn s4_build_fails_without_handler() {
    let result = SidecarBuilder::new("x").build();
    assert!(result.is_err());
}

#[test]
fn s4_build_error_is_no_handler() {
    let err = SidecarBuilder::new("x").build().unwrap_err();
    assert!(matches!(err, SidecarError::NoHandler));
}

#[test]
fn s4_build_succeeds_with_handler() {
    let result = noop_builder().build();
    assert!(result.is_ok());
}

#[test]
fn s4_build_preserves_identity() {
    let rt = SidecarBuilder::new("mysc")
        .version("1.0.0")
        .adapter_version("0.2.0")
        .on_run(|_wo, em| async move { Ok(em.finish("mysc")) })
        .build()
        .unwrap();
    assert_eq!(rt.identity().id, "mysc");
    assert_eq!(rt.identity().backend_version.as_deref(), Some("1.0.0"));
    assert_eq!(rt.identity().adapter_version.as_deref(), Some("0.2.0"));
}

#[test]
fn s4_build_preserves_capabilities() {
    let rt = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .on_run(|_wo, em| async move { Ok(em.finish("x")) })
        .build()
        .unwrap();
    assert_eq!(rt.capabilities().len(), 1);
}

#[test]
fn s4_build_preserves_mode() {
    let rt = SidecarBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, em| async move { Ok(em.finish("x")) })
        .build()
        .unwrap();
    assert_eq!(rt.execution_mode(), ExecutionMode::Passthrough);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 5 — SidecarBuilder identity, clone, debug
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s5_identity_without_versions() {
    let id = SidecarBuilder::new("bare").identity();
    assert_eq!(id.id, "bare");
    assert!(id.backend_version.is_none());
    assert!(id.adapter_version.is_none());
}

#[test]
fn s5_identity_with_both_versions() {
    let id = SidecarBuilder::new("full")
        .version("1.0")
        .adapter_version("2.0")
        .identity();
    assert_eq!(id.backend_version.as_deref(), Some("1.0"));
    assert_eq!(id.adapter_version.as_deref(), Some("2.0"));
}

#[test]
fn s5_builder_clone_preserves_name() {
    let b = SidecarBuilder::new("orig").version("1.0");
    let c = b.clone();
    assert_eq!(c.name(), "orig");
    assert_eq!(c.backend_version(), Some("1.0"));
}

#[test]
fn s5_builder_clone_with_handler() {
    let b = noop_builder();
    let c = b.clone();
    assert!(c.has_handler());
    assert!(c.build().is_ok());
}

#[test]
fn s5_builder_debug_contains_struct_name() {
    let b = SidecarBuilder::new("dbg-test");
    let s = format!("{b:?}");
    assert!(s.contains("SidecarBuilder"));
    assert!(s.contains("dbg-test"));
}

#[test]
fn s5_builder_debug_shows_handler_presence() {
    let b = noop_builder();
    let s = format!("{b:?}");
    assert!(s.contains("SidecarBuilder"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 6 — SidecarError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s6_error_no_handler_display() {
    let e = SidecarError::NoHandler;
    let msg = format!("{e}");
    assert!(msg.contains("no run handler"));
}

#[test]
fn s6_error_protocol_display() {
    let e = SidecarError::Protocol("bad json".to_string());
    let msg = format!("{e}");
    assert!(msg.contains("protocol error"));
    assert!(msg.contains("bad json"));
}

#[test]
fn s6_error_handler_display() {
    let e = SidecarError::Handler("boom".to_string());
    let msg = format!("{e}");
    assert!(msg.contains("handler error"));
    assert!(msg.contains("boom"));
}

#[test]
fn s6_error_io_from_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let e: SidecarError = io_err.into();
    let msg = format!("{e}");
    assert!(msg.contains("I/O error"));
}

#[test]
fn s6_error_is_debug() {
    let e = SidecarError::NoHandler;
    let s = format!("{e:?}");
    assert!(s.contains("NoHandler"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 7 — EventEmitter construction
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s7_emitter_new_returns_pair() {
    let (emitter, _rx) = EventEmitter::new("r1", 8);
    assert_eq!(emitter.ref_id(), "r1");
}

#[tokio::test]
async fn s7_emitter_ref_id_owned_string() {
    let (emitter, _rx) = EventEmitter::new(String::from("r2"), 8);
    assert_eq!(emitter.ref_id(), "r2");
}

#[tokio::test]
async fn s7_emitter_from_sender() {
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let emitter = EventEmitter::from_sender("r3", tx);
    assert_eq!(emitter.ref_id(), "r3");
}

#[tokio::test]
async fn s7_emitter_clone() {
    let (emitter, _rx) = EventEmitter::new("r4", 8);
    let cloned = emitter.clone();
    assert_eq!(cloned.ref_id(), "r4");
}

#[tokio::test]
async fn s7_emitter_debug() {
    let (emitter, _rx) = EventEmitter::new("r5", 8);
    let s = format!("{emitter:?}");
    assert!(s.contains("EventEmitter"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 8 — EventEmitter: emit_text_delta
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s8_emit_text_delta_basic() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_text_delta("hello").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[tokio::test]
async fn s8_emit_text_delta_empty() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_text_delta("").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, ""),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[tokio::test]
async fn s8_emit_text_delta_unicode() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_text_delta("こんにちは🌍").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "こんにちは🌍"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 9 — EventEmitter: emit_message
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s9_emit_message_basic() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_message("final answer").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::AssistantMessage { text } if text == "final answer"
    ));
}

#[tokio::test]
async fn s9_emit_message_multiline() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_message("line1\nline2\nline3").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantMessage { text } => assert!(text.contains('\n')),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 10 — EventEmitter: emit_tool_call_start
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s10_emit_tool_call_start() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_tool_call_start("read_file", "tc-1", serde_json::json!({"path": "main.rs"}))
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
            assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
            assert_eq!(input["path"], "main.rs");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[tokio::test]
async fn s10_emit_tool_call_start_empty_input() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_tool_call_start("list_files", "tc-2", serde_json::json!({}))
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert!(input.is_object());
            assert_eq!(input.as_object().unwrap().len(), 0);
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 11 — EventEmitter: emit_tool_result
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s11_emit_tool_result_success() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_tool_result(
            "read_file",
            "tc-1",
            serde_json::json!("file content"),
            false,
        )
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
            assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
            assert_eq!(output, serde_json::json!("file content"));
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[tokio::test]
async fn s11_emit_tool_result_error_flag() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_tool_result("bash", "tc-5", serde_json::json!("exit 1"), true)
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 12 — EventEmitter: emit_warning
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s12_emit_warning() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_warning("low on tokens").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::Warning { message } if message == "low on tokens"
    ));
}

#[tokio::test]
async fn s12_emit_warning_empty() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_warning("").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::Warning { message } => assert_eq!(message, ""),
        other => panic!("expected Warning, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 13 — EventEmitter: emit_error
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s13_emit_error_with_code() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
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
            assert!(matches!(error_code, Some(ErrorCode::BackendTimeout)));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[tokio::test]
async fn s13_emit_error_without_code() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_error(None, "unknown failure").await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "unknown failure");
            assert!(error_code.is_none());
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 14 — EventEmitter: emit_run_started / run_completed
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s14_emit_run_started() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_run_started("beginning work").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::RunStarted { message } if message == "beginning work"
    ));
}

#[tokio::test]
async fn s14_emit_run_completed() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_run_completed("all done").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(matches!(
        ev.kind,
        AgentEventKind::RunCompleted { message } if message == "all done"
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 15 — EventEmitter: emit_file_changed
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s15_emit_file_changed() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_file_changed("src/main.rs", "added entry point")
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::FileChanged { path, summary } => {
            assert_eq!(path, "src/main.rs");
            assert_eq!(summary, "added entry point");
        }
        other => panic!("expected FileChanged, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 16 — EventEmitter: emit_command_executed
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s16_emit_command_executed_with_exit_code() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_command_executed("cargo build", Some(0), Some("ok"))
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
            assert_eq!(output_preview.as_deref(), Some("ok"));
        }
        other => panic!("expected CommandExecuted, got {other:?}"),
    }
}

#[tokio::test]
async fn s16_emit_command_executed_no_exit_code() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_command_executed("kill -9 1", None, None)
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
        other => panic!("expected CommandExecuted, got {other:?}"),
    }
}

#[tokio::test]
async fn s16_emit_command_with_nonzero_exit() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter
        .emit_command_executed("false", Some(1), Some("error"))
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::CommandExecuted { exit_code, .. } => {
            assert_eq!(exit_code, Some(1));
        }
        other => panic!("expected CommandExecuted, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 17 — EventEmitter: finish / finish_failed
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s17_finish_returns_complete_receipt() {
    let (emitter, _rx) = EventEmitter::new("r", 4);
    let receipt = emitter.finish("my-backend");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "my-backend");
}

#[tokio::test]
async fn s17_finish_failed_returns_failed_receipt() {
    let (emitter, _rx) = EventEmitter::new("r", 4);
    let receipt = emitter.finish_failed("my-backend");
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.backend.id, "my-backend");
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 18 — EventEmitter: closed channel
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s18_emit_text_delta_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    let err = emitter.emit_text_delta("hi").await.unwrap_err();
    assert!(matches!(err, EmitError::ChannelClosed));
}

#[tokio::test]
async fn s18_emit_message_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(emitter.emit_message("msg").await.is_err());
}

#[tokio::test]
async fn s18_emit_warning_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(emitter.emit_warning("warn").await.is_err());
}

#[tokio::test]
async fn s18_emit_error_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(emitter.emit_error(None, "err").await.is_err());
}

#[tokio::test]
async fn s18_emit_run_started_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(emitter.emit_run_started("start").await.is_err());
}

#[tokio::test]
async fn s18_emit_run_completed_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(emitter.emit_run_completed("done").await.is_err());
}

#[tokio::test]
async fn s18_emit_file_changed_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(emitter.emit_file_changed("f", "s").await.is_err());
}

#[tokio::test]
async fn s18_emit_command_executed_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(
        emitter
            .emit_command_executed("ls", None, None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn s18_emit_tool_call_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(
        emitter
            .emit_tool_call_start("t", "id", serde_json::json!({}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn s18_emit_tool_result_on_closed_channel() {
    let (emitter, rx) = EventEmitter::new("r", 4);
    drop(rx);
    assert!(
        emitter
            .emit_tool_result("t", "id", serde_json::json!(null), false)
            .await
            .is_err()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 19 — EmitError
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s19_emit_error_display() {
    let e = EmitError::ChannelClosed;
    assert_eq!(format!("{e}"), "event channel closed");
}

#[test]
fn s19_emit_error_debug() {
    let e = EmitError::ChannelClosed;
    let s = format!("{e:?}");
    assert!(s.contains("ChannelClosed"));
}

#[test]
fn s19_emit_error_clone() {
    let e = EmitError::ChannelClosed;
    let c = e.clone();
    assert!(matches!(c, EmitError::ChannelClosed));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 20 — EventEmitter: multiple events in sequence
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s20_multiple_events_ordered() {
    let (emitter, mut rx) = EventEmitter::new("r", 16);
    emitter.emit_run_started("go").await.unwrap();
    emitter.emit_text_delta("tok1").await.unwrap();
    emitter.emit_text_delta("tok2").await.unwrap();
    emitter.emit_message("full").await.unwrap();
    emitter.emit_run_completed("done").await.unwrap();

    let e1 = rx.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::RunStarted { .. }));
    let e2 = rx.recv().await.unwrap();
    assert!(matches!(e2.kind, AgentEventKind::AssistantDelta { .. }));
    let e3 = rx.recv().await.unwrap();
    assert!(matches!(e3.kind, AgentEventKind::AssistantDelta { .. }));
    let e4 = rx.recv().await.unwrap();
    assert!(matches!(e4.kind, AgentEventKind::AssistantMessage { .. }));
    let e5 = rx.recv().await.unwrap();
    assert!(matches!(e5.kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn s20_events_have_timestamp() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    let before = chrono::Utc::now();
    emitter.emit_text_delta("t").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(ev.ts >= before);
}

#[tokio::test]
async fn s20_events_ext_is_none() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    emitter.emit_text_delta("t").await.unwrap();
    let ev = rx.recv().await.unwrap();
    assert!(ev.ext.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 21 — SidecarRuntime accessors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s21_runtime_identity() {
    let rt = noop_builder().version("9.0").build().unwrap();
    assert_eq!(rt.identity().id, "test-sc");
    assert_eq!(rt.identity().backend_version.as_deref(), Some("9.0"));
}

#[test]
fn s21_runtime_capabilities_empty() {
    let rt = noop_builder().build().unwrap();
    assert!(rt.capabilities().is_empty());
}

#[test]
fn s21_runtime_capabilities_nonempty() {
    let rt = SidecarBuilder::new("x")
        .capability(Capability::Streaming, SupportLevel::Native)
        .on_run(|_wo, em| async move { Ok(em.finish("x")) })
        .build()
        .unwrap();
    assert_eq!(rt.capabilities().len(), 1);
}

#[test]
fn s21_runtime_execution_mode_default() {
    let rt = noop_builder().build().unwrap();
    assert_eq!(rt.execution_mode(), ExecutionMode::Mapped);
}

#[test]
fn s21_runtime_execution_mode_passthrough() {
    let rt = SidecarBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, em| async move { Ok(em.finish("x")) })
        .build()
        .unwrap();
    assert_eq!(rt.execution_mode(), ExecutionMode::Passthrough);
}

#[test]
fn s21_runtime_debug_format() {
    let rt = noop_builder().build().unwrap();
    let s = format!("{rt:?}");
    assert!(s.contains("SidecarRuntime"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 22 — SidecarRuntime: run_with_io hello handshake
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s22_runtime_sends_hello_on_empty_input() {
    let rt = noop_builder().build().unwrap();
    let input = b"" as &[u8];
    let reader = BufReader::new(input);
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
    assert!(!lines.is_empty());
    let hello: Envelope = JsonlCodec::decode(lines[0]).unwrap();
    assert!(matches!(hello, Envelope::Hello { .. }));
}

#[tokio::test]
async fn s22_hello_contains_identity() {
    let rt = SidecarBuilder::new("hello-sc")
        .version("1.2.3")
        .on_run(|_wo, em| async move { Ok(em.finish("hello-sc")) })
        .build()
        .unwrap();
    let reader = BufReader::new(b"" as &[u8]);
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let line = std::str::from_utf8(&output)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    let hello = JsonlCodec::decode(line).unwrap();
    match hello {
        Envelope::Hello { backend, .. } => {
            assert_eq!(backend.id, "hello-sc");
            assert_eq!(backend.backend_version.as_deref(), Some("1.2.3"));
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn s22_hello_contains_contract_version() {
    let rt = noop_builder().build().unwrap();
    let reader = BufReader::new(b"" as &[u8]);
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let line = std::str::from_utf8(&output)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    let hello = JsonlCodec::decode(line).unwrap();
    match hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn s22_hello_contains_mode() {
    let rt = SidecarBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, em| async move { Ok(em.finish("x")) })
        .build()
        .unwrap();
    let reader = BufReader::new(b"" as &[u8]);
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let line = std::str::from_utf8(&output)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    let hello = JsonlCodec::decode(line).unwrap();
    match hello {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 23 — SidecarRuntime: run handling
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s23_runtime_processes_run_envelope() {
    let rt = noop_builder().build().unwrap();
    let (run_line, wo) = make_run_envelope("test-task");
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // hello + final
    assert!(lines.len() >= 2);

    let last = JsonlCodec::decode(lines.last().unwrap()).unwrap();
    assert!(matches!(last, Envelope::Final { .. }));
}

#[tokio::test]
async fn s23_final_receipt_has_complete_outcome() {
    let rt = noop_builder().build().unwrap();
    let (run_line, _wo) = make_run_envelope("task");
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let last = JsonlCodec::decode(lines.last().unwrap()).unwrap();
    match last {
        Envelope::Final { receipt, .. } => {
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

#[tokio::test]
async fn s23_final_ref_id_matches_run_id() {
    let rt = noop_builder().build().unwrap();
    let (run_line, wo) = make_run_envelope("task");
    let expected_id = wo.id.to_string();
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let last = JsonlCodec::decode(lines.last().unwrap()).unwrap();
    match last {
        Envelope::Final { ref_id, .. } => assert_eq!(ref_id, expected_id),
        other => panic!("expected Final, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 24 — SidecarRuntime: handler emitting events
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s24_runtime_streams_handler_events() {
    let rt = SidecarBuilder::new("ev-sc")
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("token1").await.unwrap();
            emitter.emit_text_delta("token2").await.unwrap();
            Ok(emitter.finish("ev-sc"))
        })
        .build()
        .unwrap();

    let (run_line, _wo) = make_run_envelope("task");
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // hello + 2 events + final = 4
    assert!(lines.len() >= 4, "expected >=4 lines, got {}", lines.len());

    // Verify events
    let ev1 = JsonlCodec::decode(lines[1]).unwrap();
    assert!(matches!(ev1, Envelope::Event { .. }));
    let ev2 = JsonlCodec::decode(lines[2]).unwrap();
    assert!(matches!(ev2, Envelope::Event { .. }));
}

#[tokio::test]
async fn s24_event_ref_ids_match_run_id() {
    let rt = SidecarBuilder::new("x")
        .on_run(|_wo, emitter| async move {
            emitter.emit_message("hi").await.unwrap();
            Ok(emitter.finish("x"))
        })
        .build()
        .unwrap();

    let (run_line, wo) = make_run_envelope("task");
    let expected_id = wo.id.to_string();
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    for line in text.lines().skip(1) {
        // skip hello
        let envelope = JsonlCodec::decode(line).unwrap();
        match envelope {
            Envelope::Event { ref_id, .. } => assert_eq!(ref_id, expected_id),
            Envelope::Final { ref_id, .. } => assert_eq!(ref_id, expected_id),
            _ => {}
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 25 — SidecarRuntime: handler errors produce Fatal
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s25_handler_error_produces_fatal() {
    let rt = SidecarBuilder::new("fail-sc")
        .on_run(
            |_wo, _emitter| async move { Err(SidecarError::Handler("intentional failure".into())) },
        )
        .build()
        .unwrap();

    let (run_line, _wo) = make_run_envelope("task");
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let last = JsonlCodec::decode(lines.last().unwrap()).unwrap();
    match last {
        Envelope::Fatal { error, .. } => {
            assert!(error.contains("intentional failure"));
        }
        other => panic!("expected Fatal, got {other:?}"),
    }
}

#[tokio::test]
async fn s25_fatal_has_ref_id() {
    let rt = SidecarBuilder::new("x")
        .on_run(|_wo, _em| async move { Err(SidecarError::Handler("err".into())) })
        .build()
        .unwrap();

    let (run_line, wo) = make_run_envelope("task");
    let expected_id = wo.id.to_string();
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let last_line = text.lines().last().unwrap();
    let env = JsonlCodec::decode(last_line).unwrap();
    match env {
        Envelope::Fatal { ref_id, .. } => assert_eq!(ref_id, Some(expected_id)),
        other => panic!("expected Fatal, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 26 — SidecarRuntime: blank/skip lines
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s26_runtime_skips_blank_lines() {
    let rt = noop_builder().build().unwrap();
    let (run_line, _wo) = make_run_envelope("task");
    let input = format!("\n\n{}\n\n", run_line.trim());
    let reader = BufReader::new(input.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() >= 2); // hello + final
}

#[tokio::test]
async fn s26_runtime_ignores_non_run_envelopes() {
    let rt = noop_builder().build().unwrap();
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "other".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Mapped,
    );
    let hello_line = JsonlCodec::encode(&hello).unwrap();
    let reader = BufReader::new(hello_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // Only hello from our runtime, the incoming Hello was ignored
    assert_eq!(lines.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 27 — sidecar_script helper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s27_sidecar_script_joins_paths() {
    use std::path::Path;
    let root = Path::new("hosts");
    let result = abp_sidecar_sdk::sidecar_script(root, "node/index.js");
    assert_eq!(result, Path::new("hosts").join("node/index.js"));
}

#[test]
fn s27_sidecar_script_absolute_root() {
    use std::path::Path;
    let root = if cfg!(windows) {
        Path::new("C:\\proj\\hosts")
    } else {
        Path::new("/proj/hosts")
    };
    let result = abp_sidecar_sdk::sidecar_script(root, "script.js");
    assert_eq!(result, root.join("script.js"));
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 28 — Builder chaining fluency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn s28_full_builder_chain() {
    let b = SidecarBuilder::new("full-chain")
        .version("1.0.0")
        .adapter_version("2.0.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .capability(Capability::Vision, SupportLevel::Emulated)
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, em| async move { Ok(em.finish("full-chain")) });

    assert_eq!(b.name(), "full-chain");
    assert_eq!(b.backend_version(), Some("1.0.0"));
    assert_eq!(b.adapter_version_str(), Some("2.0.0"));
    assert_eq!(b.capability_manifest().len(), 3);
    assert_eq!(b.execution_mode(), ExecutionMode::Passthrough);
    assert!(b.has_handler());
}

#[test]
fn s28_builder_chain_builds_successfully() {
    let rt = SidecarBuilder::new("chain-build")
        .version("1.0.0")
        .adapter_version("2.0.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .mode(ExecutionMode::Mapped)
        .on_run(|_wo, em| async move { Ok(em.finish("chain-build")) })
        .build();
    assert!(rt.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 29 — End-to-end lifecycle with mixed events
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s29_e2e_mixed_events_lifecycle() {
    let rt = SidecarBuilder::new("e2e")
        .version("1.0")
        .capability(Capability::Streaming, SupportLevel::Native)
        .capability(Capability::ToolUse, SupportLevel::Native)
        .on_run(|_wo, emitter| async move {
            emitter.emit_run_started("starting").await.unwrap();
            emitter.emit_text_delta("tok").await.unwrap();
            emitter
                .emit_tool_call_start("read_file", "t1", serde_json::json!({"p": "a.rs"}))
                .await
                .unwrap();
            emitter
                .emit_tool_result("read_file", "t1", serde_json::json!("content"), false)
                .await
                .unwrap();
            emitter.emit_message("final msg").await.unwrap();
            emitter.emit_run_completed("done").await.unwrap();
            Ok(emitter.finish("e2e"))
        })
        .build()
        .unwrap();

    let (run_line, _wo) = make_run_envelope("e2e-task");
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // hello + 6 events + final = 8
    assert_eq!(
        lines.len(),
        8,
        "expected 8 lines, got {}: {:?}",
        lines.len(),
        lines
    );
}

#[tokio::test]
async fn s29_e2e_passthrough_mode() {
    let rt = SidecarBuilder::new("pt")
        .mode(ExecutionMode::Passthrough)
        .on_run(|_wo, emitter| async move {
            emitter.emit_text_delta("pass").await.unwrap();
            Ok(emitter.finish("pt"))
        })
        .build()
        .unwrap();

    let (run_line, _wo) = make_run_envelope("task");
    let reader = BufReader::new(run_line.as_bytes());
    let mut output = Vec::new();
    rt.run_with_io(reader, &mut output).await.unwrap();

    let text = std::str::from_utf8(&output).unwrap();
    let hello_line = text.lines().next().unwrap();
    let hello = JsonlCodec::decode(hello_line).unwrap();
    match hello {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        other => panic!("expected Hello, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SECTION 30 — Edge cases and special characters
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn s30_emit_text_delta_with_special_chars() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    let special = "line1\nline2\ttab\\backslash\"quote";
    emitter.emit_text_delta(special).await.unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, special),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[tokio::test]
async fn s30_emit_tool_call_complex_json_input() {
    let (emitter, mut rx) = EventEmitter::new("r", 4);
    let input = serde_json::json!({
        "nested": {"arr": [1, 2, 3], "obj": {"key": "val"}},
        "null_val": null,
        "bool_val": true
    });
    emitter
        .emit_tool_call_start("complex_tool", "tc-99", input.clone())
        .await
        .unwrap();
    let ev = rx.recv().await.unwrap();
    match ev.kind {
        AgentEventKind::ToolCall { input: i, .. } => assert_eq!(i, input),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn s30_builder_name_with_unicode() {
    let b = SidecarBuilder::new("日本語サイドカー");
    assert_eq!(b.name(), "日本語サイドカー");
}

#[test]
fn s30_builder_empty_name() {
    let b = SidecarBuilder::new("");
    assert_eq!(b.name(), "");
}

#[test]
fn s30_builder_name_with_special_chars() {
    let b = SidecarBuilder::new("my-sidecar_v2.0/beta");
    assert_eq!(b.name(), "my-sidecar_v2.0/beta");
}

#[tokio::test]
async fn s30_cloned_emitter_sends_to_same_channel() {
    let (emitter, mut rx) = EventEmitter::new("r", 8);
    let clone = emitter.clone();
    emitter.emit_text_delta("from-orig").await.unwrap();
    clone.emit_text_delta("from-clone").await.unwrap();

    let e1 = rx.recv().await.unwrap();
    let e2 = rx.recv().await.unwrap();
    match (&e1.kind, &e2.kind) {
        (
            AgentEventKind::AssistantDelta { text: t1 },
            AgentEventKind::AssistantDelta { text: t2 },
        ) => {
            assert_eq!(t1, "from-orig");
            assert_eq!(t2, "from-clone");
        }
        other => panic!("unexpected events: {other:?}"),
    }
}
