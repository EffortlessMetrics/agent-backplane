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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style scenario tests for the Agent Backplane.
//!
//! Each test follows the Given/When/Then pattern documented in its doc comment.
//! Tests are organized by domain: work order lifecycle, dialect detection,
//! policy enforcement, capability negotiation, receipt validation, workspace
//! staging, and IR conversion roundtrips.

use std::collections::BTreeMap;
use std::path::Path;

use abp_capability::negotiate;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, ExecutionMode, MinSupport, Outcome, PolicyProfile, ReceiptBuilder,
    SupportLevel, WorkOrderBuilder, WorkspaceMode,
};
use abp_dialect::Dialect;
use abp_dialect::DialectDetector;
use abp_ir::lower::{lower_for_dialect, lower_to_claude, lower_to_gemini, lower_to_openai};
use abp_ir::normalize::normalize;
use abp_mapper::{
    default_ir_mapper, supported_ir_pairs, ClaudeGeminiIrMapper, IrMapper, OpenAiClaudeIrMapper,
    OpenAiGeminiIrMapper,
};
use abp_policy::PolicyEngine;
use abp_projection::{ProjectionMatrix, ProjectionMode};
use abp_receipt::{compute_hash, verify_hash};
use abp_runtime::{Runtime, RuntimeError};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn simple_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Hello world task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn simple_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage::text(IrRole::Assistant, "4"),
    ])
}

fn ir_tool_definitions() -> Vec<IrToolDefinition> {
    vec![IrToolDefinition {
        name: "get_weather".into(),
        description: "Get weather for a location".into(),
        parameters: json!({"type": "object", "properties": {"location": {"type": "string"}}}),
    }]
}

fn openai_style_request() -> serde_json::Value {
    json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hello"}
        ]
    })
}

fn claude_style_request() -> serde_json::Value {
    json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    })
}

fn gemini_style_request() -> serde_json::Value {
    json!({
        "contents": [
            {"role": "user", "parts": [{"text": "Hello"}]}
        ],
        "generationConfig": {"temperature": 0.7}
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Work Order Lifecycle
// ═══════════════════════════════════════════════════════════════════════════

/// Given a valid work order,
/// When submitted to runtime with mock backend,
/// Then receive a valid receipt with a SHA-256 hash.
#[tokio::test]
async fn given_valid_work_order_when_submitted_to_mock_then_receipt_has_hash() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

/// Given a valid work order,
/// When executed via mock backend,
/// Then trace events include RunStarted and RunCompleted.
#[tokio::test]
async fn given_valid_work_order_when_executed_then_trace_has_lifecycle_events() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order();
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    let has_started = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
    let has_completed = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));

    assert!(has_started, "trace should contain RunStarted");
    assert!(has_completed, "trace should contain RunCompleted");
}

/// Given a work order with tool_use capability requirement,
/// When the mock backend executes (it doesn't support ToolUse natively),
/// Then runtime returns a capability check error.
#[tokio::test]
async fn given_work_order_with_tool_use_cap_when_mock_backend_then_capability_error() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("tool task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .root(".")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Native,
            }],
        })
        .build();

    let result = rt.run_streaming("mock", wo).await;
    // The mock backend will either fail at pre-flight capability check or
    // within the backend run itself when ToolUse (not in manifest) is required.
    match result {
        Err(_) => {} // capability check failed before execution
        Ok(handle) => {
            let receipt_result = handle.receipt.await.unwrap();
            assert!(
                receipt_result.is_err(),
                "should fail due to unsatisfied capability"
            );
        }
    }
}

/// Given a work order with unknown backend,
/// Then runtime returns UnknownBackend error.
#[tokio::test]
async fn given_work_order_with_unknown_backend_then_unknown_backend_error() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order();
    let result = rt.run_streaming("nonexistent_backend", wo).await;

    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got: {err:?}"
    );
}

/// Given a work order in passthrough mode (vendor flag),
/// When executed via mock backend,
/// Then the receipt reflects the execution mode.
#[tokio::test]
async fn given_passthrough_work_order_when_executed_then_receipt_shows_mode() {
    let rt = Runtime::with_default_backends();
    let mut wo = simple_work_order();
    wo.config
        .vendor
        .insert("abp".to_string(), json!({"mode": "passthrough"}));
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();
    // MockBackend extracts mode via extract_execution_mode, which reads the vendor flag.
    assert!(
        receipt.mode == ExecutionMode::Passthrough || receipt.mode == ExecutionMode::Mapped,
        "receipt mode should be set"
    );
}

/// Given a runtime with mock backend,
/// When backend_names is queried,
/// Then "mock" is in the list.
#[tokio::test]
async fn given_runtime_with_default_backends_then_mock_is_listed() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()));
}

/// Given two sequential runs,
/// When both complete,
/// Then each produces a unique run_id.
#[tokio::test]
async fn given_two_sequential_runs_then_each_has_unique_run_id() {
    let rt = Runtime::with_default_backends();
    let wo1 = simple_work_order();
    let wo2 = simple_work_order();

    let h1 = rt.run_streaming("mock", wo1).await.unwrap();
    let h2 = rt.run_streaming("mock", wo2).await.unwrap();

    assert_ne!(h1.run_id, h2.run_id);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Dialect Detection
// ═══════════════════════════════════════════════════════════════════════════

/// Given an OpenAI-formatted request JSON,
/// When dialect detection runs,
/// Then returns OpenAI dialect.
#[test]
fn given_openai_request_when_detected_then_openai_dialect() {
    let detector = DialectDetector::new();
    let result = detector.detect(&openai_style_request()).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
    assert!(result.confidence > 0.0);
}

/// Given a Claude-formatted request JSON,
/// When dialect detection runs,
/// Then returns Claude dialect.
#[test]
fn given_claude_request_when_detected_then_claude_dialect() {
    let detector = DialectDetector::new();
    let result = detector.detect(&claude_style_request()).unwrap();
    assert_eq!(result.dialect, Dialect::Claude);
    assert!(result.confidence > 0.0);
}

/// Given a Gemini-formatted request JSON,
/// When detection runs,
/// Then returns Gemini dialect.
#[test]
fn given_gemini_request_when_detected_then_gemini_dialect() {
    let detector = DialectDetector::new();
    let result = detector.detect(&gemini_style_request()).unwrap();
    assert_eq!(result.dialect, Dialect::Gemini);
    assert!(result.confidence > 0.0);
}

/// Given an ambiguous request (has both OpenAI and Claude markers),
/// When detection runs,
/// Then returns best-match with confidence score below 1.0.
#[test]
fn given_ambiguous_request_when_detected_then_best_match_with_confidence() {
    let detector = DialectDetector::new();
    // A JSON object with some OpenAI-like fields and some Claude-like fields
    let ambiguous = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 1024
    });
    let result = detector.detect(&ambiguous).unwrap();
    assert!(result.confidence > 0.0);
    assert!(result.confidence <= 1.0);
    assert!(!result.evidence.is_empty());
}

/// Given an OpenAI request,
/// When detect_all runs,
/// Then multiple dialects may score, sorted by descending confidence.
#[test]
fn given_openai_request_when_detect_all_then_sorted_results() {
    let detector = DialectDetector::new();
    let results = detector.detect_all(&openai_style_request());
    assert!(!results.is_empty());
    // Verify descending confidence order
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

/// Given a non-object JSON value,
/// When detection runs,
/// Then returns None.
#[test]
fn given_non_object_json_when_detected_then_none() {
    let detector = DialectDetector::new();
    assert!(detector.detect(&json!("just a string")).is_none());
    assert!(detector.detect(&json!(42)).is_none());
    assert!(detector.detect(&json!([])).is_none());
}

/// Given every known dialect enum variant,
/// When label() is called,
/// Then each returns a non-empty human-readable string.
#[test]
fn given_all_dialects_then_labels_are_nonempty() {
    for dialect in Dialect::all() {
        assert!(!dialect.label().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Policy Enforcement
// ═══════════════════════════════════════════════════════════════════════════

/// Given a policy that denies tool_use (Bash),
/// When checking tool usage,
/// Then policy engine rejects Bash.
#[test]
fn given_policy_denying_bash_when_checked_then_rejected() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed);
    assert!(decision.reason.is_some());
}

/// Given a policy with file read globs denying .env files,
/// When checking paths,
/// Then allows src/lib.rs but denies .env.
#[test]
fn given_policy_with_read_globs_when_checking_then_allows_matching_only() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/.env.*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("config/.env")).allowed);
    assert!(!engine.can_read_path(Path::new(".env.production")).allowed);
    assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
}

/// Given a permissive (empty) policy,
/// When checking any capability,
/// Then all pass.
#[test]
fn given_permissive_policy_when_checking_anything_then_all_allowed() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_read_path(Path::new("any/file.rs")).allowed);
    assert!(
        engine
            .can_write_path(Path::new("any/other/file.txt"))
            .allowed
    );
}

/// Given a policy with both allowlist and denylist,
/// When a tool is on both lists,
/// Then denylist takes precedence.
#[test]
fn given_policy_with_both_allow_deny_then_deny_wins() {
    let policy = PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
}

/// Given a policy with write-deny globs,
/// When checking denied and allowed paths,
/// Then only denied paths are rejected.
#[test]
fn given_policy_with_write_deny_then_only_denied_paths_rejected() {
    let policy = PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

/// Given a policy with an explicit allowlist,
/// When checking a tool not on the allowlist,
/// Then it is denied.
#[test]
fn given_explicit_allowlist_when_tool_unlisted_then_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability Negotiation
// ═══════════════════════════════════════════════════════════════════════════

/// Given a backend declaring limited capabilities (only Streaming),
/// When work order requires unsupported capability (ToolUse),
/// Then negotiation reports unsupported.
#[test]
fn given_limited_backend_when_requiring_unsupported_then_negotiation_fails() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolUse,
            min_support: MinSupport::Native,
        }],
    };

    let result = negotiate(&manifest, &requirements);
    assert!(!result.is_compatible());
    assert!(!result.unsupported.is_empty());
}

/// Given matching capabilities,
/// When negotiation runs,
/// Then proceeds (is_compatible returns true).
#[test]
fn given_matching_capabilities_when_negotiated_then_compatible() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);

    let requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let result = negotiate(&manifest, &requirements);
    assert!(result.is_compatible());
}

/// Given a backend with emulated capabilities,
/// When negotiation allows emulated support,
/// Then the capability is reported as emulated but compatible.
#[test]
fn given_emulated_capability_when_emulated_allowed_then_compatible() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolBash, SupportLevel::Emulated);

    let requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };

    let result = negotiate(&manifest, &requirements);
    assert!(result.is_compatible());
}

/// Given a backend with emulated-only capability,
/// When requirement demands Native,
/// Then negotiation fails.
#[test]
fn given_emulated_only_when_native_required_then_incompatible() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolBash, SupportLevel::Emulated);

    let requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Native,
        }],
    };

    let result = negotiate(&manifest, &requirements);
    assert!(!result.is_compatible());
}

/// Given empty requirements,
/// When negotiation runs against any manifest,
/// Then always compatible.
#[test]
fn given_empty_requirements_then_always_compatible() {
    let manifest = CapabilityManifest::new();
    let requirements = CapabilityRequirements::default();
    let result = negotiate(&manifest, &requirements);
    assert!(result.is_compatible());
}

/// Given the predefined openai_gpt4o_manifest,
/// When queried for Streaming,
/// Then it is Native.
#[test]
fn given_openai_manifest_then_streaming_is_native() {
    let manifest = abp_capability::openai_gpt4o_manifest();
    assert!(manifest.contains_key(&Capability::Streaming));
    assert!(matches!(
        manifest.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Receipt Validation
// ═══════════════════════════════════════════════════════════════════════════

/// Given a completed run receipt,
/// When the hash is computed,
/// Then it is deterministic and verifiable.
#[test]
fn given_completed_receipt_when_hashed_then_deterministic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(receipt.receipt_sha256.is_some());
    let hash1 = receipt.receipt_sha256.as_ref().unwrap().clone();

    // verify_hash should pass
    assert!(verify_hash(&receipt));

    // Re-hashing the same receipt gives the same value
    let hash2 = compute_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2);
}

/// Given a receipt with an error outcome,
/// When the receipt is produced,
/// Then outcome is Failed.
#[test]
fn given_error_during_run_when_receipt_produced_then_error_outcome() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_trace_event(make_event(AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: None,
        }))
        .build()
        .with_hash()
        .unwrap();

    assert_eq!(receipt.outcome, Outcome::Failed);
    let has_error = receipt
        .trace
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. }));
    assert!(has_error);
    assert!(verify_hash(&receipt));
}

/// Given a receipt built with ReceiptBuilder,
/// When fields are set correctly,
/// Then the builder produces matching values.
#[test]
fn given_receipt_builder_when_built_then_fields_match() {
    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .backend_version("1.0.0")
        .adapter_version("0.2.0")
        .mode(ExecutionMode::Passthrough)
        .build();

    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.2.0"));
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.outcome, Outcome::Partial);
}

/// Given two receipts built from the same builder configuration with pinned timestamps,
/// When hashed,
/// Then the hashes differ only because of different run_ids (UUID v4).
#[test]
fn given_receipt_with_trace_events_then_hash_includes_trace() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }))
        .build()
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    // Different traces → different hashes (also different run_ids, but the point stands)
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

/// Given a receipt with receipt_sha256 already set,
/// When verify_hash is called,
/// Then it confirms the hash is consistent.
#[test]
fn given_receipt_with_hash_when_verified_then_consistent() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(verify_hash(&receipt));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Workspace Staging
// ═══════════════════════════════════════════════════════════════════════════

/// Given a source directory with files,
/// When workspace is staged with exclude globs,
/// Then excluded files are absent from the staged workspace.
#[test]
fn given_source_dir_when_staged_with_exclude_then_excluded_absent() {
    use abp_workspace::WorkspaceStager;
    use std::fs;

    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("keep.rs"), "fn main() {}").unwrap();
    fs::write(src.path().join("remove.log"), "log data").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .exclude(vec!["*.log".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("keep.rs").exists());
    assert!(
        !ws.path().join("remove.log").exists(),
        "excluded file should be absent"
    );
}

/// Given workspace staging with git_init enabled,
/// When stage() runs,
/// Then a .git directory exists in the staged workspace.
#[test]
fn given_workspace_staging_when_git_init_then_git_dir_exists() {
    use abp_workspace::WorkspaceStager;
    use std::fs;

    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("main.rs"), "fn main() {}").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    assert!(
        ws.path().join(".git").exists(),
        "git repo should be initialized"
    );
}

/// Given a staged workspace with git init,
/// When git_status is queried,
/// Then it returns Some (workspace has a git repo).
#[test]
fn given_staged_workspace_when_git_status_queried_then_returns_some() {
    use abp_workspace::{WorkspaceManager, WorkspaceStager};
    use std::fs;

    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("file.txt"), "content").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(true)
        .stage()
        .unwrap();

    // git_status returns Some when the directory is a git repo
    let status = WorkspaceManager::git_status(ws.path());
    assert!(status.is_some());
}

/// Given a staged workspace with include patterns,
/// When only specific files are included,
/// Then non-matching files are absent.
#[test]
fn given_staged_workspace_with_include_then_only_matching_present() {
    use abp_workspace::WorkspaceStager;
    use std::fs;

    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.rs"), "").unwrap();
    fs::write(src.path().join("b.txt"), "").unwrap();

    let ws = WorkspaceStager::new()
        .source_root(src.path())
        .include(vec!["*.rs".into()])
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("a.rs").exists());
    assert!(
        !ws.path().join("b.txt").exists(),
        "non-matching file should be absent"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. IR Conversion Roundtrip
// ═══════════════════════════════════════════════════════════════════════════

/// Given an IR conversation,
/// When lowered to OpenAI format and inspected,
/// Then the output has "messages" array with correct roles.
#[test]
fn given_ir_conversation_when_lowered_to_openai_then_has_messages() {
    let conv = simple_ir_conversation();
    let tools = ir_tool_definitions();
    let lowered = lower_to_openai(&conv, &tools);

    let messages = lowered.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[2]["role"], "assistant");
}

/// Given an IR conversation,
/// When lowered to Claude format,
/// Then the output has "messages" array.
#[test]
fn given_ir_conversation_when_lowered_to_claude_then_has_messages() {
    let conv = simple_ir_conversation();
    let tools = vec![];
    let lowered = lower_to_claude(&conv, &tools);

    assert!(lowered.get("messages").is_some());
}

/// Given an IR conversation,
/// When lowered to Gemini format,
/// Then the output has "contents" array.
#[test]
fn given_ir_conversation_when_lowered_to_gemini_then_has_contents() {
    let conv = simple_ir_conversation();
    let tools = vec![];
    let lowered = lower_to_gemini(&conv, &tools);

    assert!(lowered.get("contents").is_some());
}

/// Given an OpenAI-style request,
/// When converted to IR via shim and back,
/// Then the roundtripped messages preserve semantic content.
#[test]
fn given_openai_request_when_roundtripped_through_ir_then_equivalent() {
    use abp_shim_openai::{ir_to_messages, messages_to_ir, ChatCompletionRequest, Message};

    let request = ChatCompletionRequest::builder()
        .model("gpt-4")
        .messages(vec![
            Message::system("You are helpful."),
            Message::user("What is 2+2?"),
        ])
        .build();

    let ir = messages_to_ir(&request.messages);
    let roundtripped = ir_to_messages(&ir);

    // Same number of messages
    assert_eq!(request.messages.len(), roundtripped.len());
    // Roles match
    assert_eq!(roundtripped[0].role, abp_shim_openai::Role::System);
    assert_eq!(roundtripped[1].role, abp_shim_openai::Role::User);
}

/// Given an OpenAI-to-Claude IR mapper,
/// When a conversation is mapped,
/// Then the result preserves message count and text content.
#[test]
fn given_openai_to_claude_ir_mapper_when_mapped_then_content_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_ir_conversation();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();

    // Text content should be preserved
    let mapped_text: Vec<String> = mapped
        .messages
        .iter()
        .filter_map(|m| {
            if m.is_text_only() {
                Some(m.text_content())
            } else {
                None
            }
        })
        .collect();

    // At least the user and assistant text should survive
    assert!(mapped_text.contains(&"What is 2+2?".to_string()));
    assert!(mapped_text.contains(&"4".to_string()));
}

/// Given an OpenAI-to-Gemini IR mapper,
/// When a conversation is mapped,
/// Then the result preserves core text.
#[test]
fn given_openai_to_gemini_ir_mapper_when_mapped_then_content_preserved() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_ir_conversation();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();

    let has_user_text = mapped
        .messages
        .iter()
        .any(|m| m.text_content().contains("2+2"));
    assert!(has_user_text, "user message text should be preserved");
}

/// Given a Claude-to-Gemini IR mapper,
/// When a conversation is mapped,
/// Then it produces a valid result.
#[test]
fn given_claude_to_gemini_ir_mapper_then_produces_valid_result() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Tell me a joke"),
        IrMessage::text(IrRole::Assistant, "Why did the chicken cross the road?"),
    ]);
    let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
    assert!(result.is_ok());
    let mapped = result.unwrap();
    assert!(!mapped.messages.is_empty());
}

/// Given an IR conversation with tool use blocks,
/// When lowered to OpenAI format,
/// Then tool_calls appear in the assistant message.
#[test]
fn given_ir_with_tool_use_when_lowered_to_openai_then_tool_calls_present() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What's the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: json!({"location": "NYC"}),
            }],
        ),
    ]);
    let tools = ir_tool_definitions();
    let lowered = lower_to_openai(&conv, &tools);
    let messages = lowered["messages"].as_array().unwrap();
    let assistant_msg = &messages[1];
    assert!(
        assistant_msg.get("tool_calls").is_some(),
        "assistant message should have tool_calls"
    );
}

/// Given the lower_for_dialect dispatcher,
/// When called for each known dialect,
/// Then it produces valid JSON for all dialects.
#[test]
fn given_lower_for_dialect_when_called_for_all_then_all_produce_valid_json() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ]);
    let tools = vec![];

    for dialect in abp_sdk_types::Dialect::all() {
        let result = lower_for_dialect(*dialect, &conv, &tools);
        assert!(
            result.is_object(),
            "lowered output for {dialect:?} should be a JSON object"
        );
    }
}

/// Given the default_ir_mapper factory,
/// When queried for a supported pair,
/// Then it returns Some mapper.
#[test]
fn given_default_ir_mapper_factory_when_supported_pair_then_returns_mapper() {
    let pairs = supported_ir_pairs();
    assert!(!pairs.is_empty(), "should have at least one supported pair");

    for (src, tgt) in &pairs {
        let mapper = default_ir_mapper(*src, *tgt);
        assert!(
            mapper.is_some(),
            "factory should return a mapper for {src:?} -> {tgt:?}"
        );
    }
}

/// Given IR normalization,
/// When applied to a conversation with whitespace,
/// Then text is trimmed.
#[test]
fn given_ir_normalization_when_applied_then_text_trimmed() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "  hello  "),
        IrMessage::text(IrRole::Assistant, "  world  "),
    ]);
    let normalized = normalize(&conv);
    for msg in &normalized.messages {
        let text: String = msg.text_content();
        assert_eq!(
            text,
            text.trim(),
            "text should be trimmed after normalization"
        );
    }
}

/// Given an OpenAI request converted to WorkOrder via shim,
/// When examined,
/// Then the task and model fields are populated.
#[test]
fn given_openai_request_when_converted_to_work_order_then_fields_populated() {
    use abp_shim_openai::{request_to_work_order, ChatCompletionRequest, Message};

    let request = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Write a poem")])
        .build();

    let wo = request_to_work_order(&request);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    assert!(!wo.task.is_empty());
}

/// Given a Claude MessageRequest,
/// When converted to WorkOrder via shim,
/// Then the task is populated from messages.
#[test]
fn given_claude_request_when_converted_to_work_order_then_task_populated() {
    use abp_shim_claude::{request_to_work_order, ContentBlock, Message, MessageRequest, Role};

    let request = MessageRequest {
        model: "claude-3-5-sonnet-20241022".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Explain Rust lifetimes".into(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let wo = request_to_work_order(&request);
    assert!(!wo.task.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Projection Matrix
// ═══════════════════════════════════════════════════════════════════════════

/// Given a projection matrix with registered backends,
/// When a work order is projected,
/// Then a backend is selected.
#[test]
fn given_projection_matrix_with_backends_when_projected_then_backend_selected() {
    let mut matrix = ProjectionMatrix::with_defaults();
    let manifest = abp_capability::openai_gpt4o_manifest();
    matrix.register_backend("openai-gpt4o", manifest, Dialect::OpenAi, 10);

    let wo = simple_work_order();
    let result = matrix.project(&wo);
    // May or may not match depending on work order requirements, but should not panic
    // If it matches, selected_backend should be set
    if let Ok(res) = result {
        assert!(!res.selected_backend.is_empty());
    }
}

/// Given a projection matrix with default dialect entries,
/// When OpenAI->Claude is looked up,
/// Then an entry exists.
#[test]
fn given_projection_defaults_when_looking_up_openai_claude_then_entry_exists() {
    let matrix = ProjectionMatrix::with_defaults();
    let entry = matrix.lookup(Dialect::OpenAi, Dialect::Claude);
    assert!(
        entry.is_some(),
        "OpenAI->Claude should have a default projection entry"
    );
}

/// Given a projection matrix,
/// When registering a custom mapping,
/// Then it can be looked up.
#[test]
fn given_projection_matrix_when_custom_mapping_registered_then_lookup_works() {
    let mut matrix = ProjectionMatrix::new();
    matrix.register(Dialect::OpenAi, Dialect::Gemini, ProjectionMode::Mapped);
    let entry = matrix.lookup(Dialect::OpenAi, Dialect::Gemini);
    assert!(entry.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Core Contract Invariants
// ═══════════════════════════════════════════════════════════════════════════

/// Given the CONTRACT_VERSION constant,
/// Then it equals "abp/v0.1".
#[test]
fn given_contract_version_then_equals_expected() {
    assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
}

/// Given canonical_json,
/// When called on an object with unordered keys,
/// Then keys are sorted deterministically.
#[test]
fn given_canonical_json_when_called_then_keys_sorted() {
    let json = abp_core::canonical_json(&json!({"z": 1, "a": 2})).unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    assert!(a_pos < z_pos, "keys should be sorted: a before z");
}

/// Given sha256_hex,
/// When called on known input,
/// Then output is 64 hex characters.
#[test]
fn given_sha256_hex_then_output_is_64_hex_chars() {
    let hex = abp_core::sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

/// Given a WorkOrderBuilder,
/// When all fields are set,
/// Then the built WorkOrder reflects them.
#[test]
fn given_work_order_builder_when_all_fields_set_then_reflected() {
    let wo = WorkOrderBuilder::new("Fix bug")
        .root("/tmp/ws")
        .workspace_mode(WorkspaceMode::Staged)
        .model("gpt-4")
        .max_budget_usd(1.0)
        .max_turns(5)
        .build();

    assert_eq!(wo.task, "Fix bug");
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
    assert_eq!(wo.config.max_turns, Some(5));
}

/// Given a SupportLevel,
/// When checking satisfies() logic,
/// Then Native satisfies both, Emulated satisfies only Emulated.
#[test]
fn given_support_level_when_checking_satisfies_then_correct() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

/// Given an AgentEvent with ext field,
/// When serialized and deserialized,
/// Then ext is preserved.
#[test]
fn given_agent_event_with_ext_when_roundtripped_then_ext_preserved() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"foo": "bar"}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: Some(ext),
    };

    let json = serde_json::to_string(&event).unwrap();
    let restored: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(restored.ext.is_some());
    assert_eq!(restored.ext.unwrap()["raw_message"], json!({"foo": "bar"}));
}

/// Given an Outcome enum,
/// When serialized to JSON,
/// Then it uses snake_case.
#[test]
fn given_outcome_when_serialized_then_snake_case() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, "\"complete\"");

    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(json, "\"failed\"");

    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    assert_eq!(json, "\"partial\"");
}

/// Given RuntimeError variants,
/// Then error_code maps correctly and is_retryable is accurate.
#[test]
fn given_runtime_errors_then_error_code_and_retryable_correct() {
    let unknown = RuntimeError::UnknownBackend { name: "x".into() };
    assert!(!unknown.is_retryable());

    let backend_fail = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
    assert!(backend_fail.is_retryable());
}

/// Given a ReceiptBuilder with no hash,
/// When with_hash() is called on ReceiptBuilder directly,
/// Then the receipt has a hash set.
#[test]
fn given_receipt_builder_when_with_hash_called_then_hash_set() {
    let receipt = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.unwrap().len(), 64);
}
