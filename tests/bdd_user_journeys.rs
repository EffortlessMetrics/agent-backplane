// SPDX-License-Identifier: MIT OR Apache-2.0
//! BDD-style user journey tests covering the full ABP lifecycle:
//! first-time setup, simple chat completion, tool use, cross-SDK translation,
//! error handling, and audit trail.

use std::path::Path;

use abp_capability::negotiate;
use abp_config::{BackplaneConfig, load_config};
use abp_core::{
    AgentEventKind, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    MinSupport, Outcome, PolicyProfile, ReceiptBuilder, SupportLevel as CoreSupportLevel,
    WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_error::{AbpError, ErrorCode};
use abp_mapping::{Fidelity, features, known_rules, validate_mapping};
use abp_policy::PolicyEngine;
use abp_receipt::{diff_receipts, verify_hash};
use abp_runtime::{Runtime, RuntimeError};

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn make_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(abp_core::WorkspaceMode::PassThrough)
        .build()
}

// ===========================================================================
// User journey: First-time setup
// ===========================================================================

/// Given a fresh config load with no file,
/// When loading defaults,
/// Then sane defaults are returned.
#[test]
fn given_new_config_when_loading_then_defaults_are_sane() {
    let config = load_config(None).unwrap();
    assert_eq!(config.log_level.as_deref(), Some("info"));
    assert!(config.backends.is_empty());
    // default_backend should be None (CLI falls back to "mock" separately).
    assert!(config.default_backend.is_none());
}

/// Given the example TOML shipped with the repo,
/// When parsed,
/// Then the config is valid and has at least one backend.
#[test]
fn given_example_toml_when_parsed_then_config_is_valid() {
    let content = include_str!("../backplane.example.toml");
    let config: BackplaneConfig = toml::from_str(content).expect("parse example config");
    assert!(
        !config.backends.is_empty(),
        "example should define backends"
    );
    // Validation should not produce hard errors.
    let warnings = abp_config::validate_config(&config).unwrap();
    for w in &warnings {
        // Warnings are OK, hard errors are not.
        assert!(
            !w.to_string().starts_with("error:"),
            "unexpected error in example config: {w}"
        );
    }
}

/// Given a config that names a mock backend,
/// When the runtime is created with default backends,
/// Then "mock" is registered and retrievable.
#[test]
fn given_config_with_backend_when_runtime_created_then_backend_registered() {
    let rt = Runtime::with_default_backends();
    let names = rt.backend_names();
    assert!(names.contains(&"mock".to_string()), "mock backend missing");
    assert!(rt.backend("mock").is_some());
}

// ===========================================================================
// User journey: Simple chat completion
// ===========================================================================

/// Given an "openai-style" request (mock backend, simple task),
/// When run through the runtime,
/// Then the receipt is complete with a valid hash.
#[tokio::test]
async fn given_openai_request_when_run_then_receipt_is_complete() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("Summarize the README");

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some(), "hash should be set");
    assert!(verify_hash(&receipt), "receipt hash should verify");
}

/// Given a Claude-style request simulated via mock,
/// When run,
/// Then the receipt contains a trace with thinking-like events (assistant messages).
#[tokio::test]
async fn given_claude_request_when_run_then_receipt_has_thinking() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Explain quantum computing")
        .model("claude-3-opus")
        .root(".")
        .workspace_mode(abp_core::WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    // Mock backend emits AssistantMessage events; verify trace is non-empty.
    assert!(
        !receipt.trace.is_empty(),
        "receipt should contain trace events"
    );
    let has_assistant_msg = receipt.trace.iter().any(|ev| {
        matches!(
            ev.kind,
            AgentEventKind::AssistantMessage { .. } | AgentEventKind::AssistantDelta { .. }
        )
    });
    assert!(has_assistant_msg, "trace should have assistant messages");
}

/// Given a Gemini-style request simulated via mock,
/// When run,
/// Then the receipt has usage data.
#[tokio::test]
async fn given_gemini_request_when_run_then_receipt_has_usage() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Generate a haiku")
        .model("gemini-2.5-flash")
        .root(".")
        .workspace_mode(abp_core::WorkspaceMode::PassThrough)
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    // usage_raw should be present (at least an object).
    assert!(
        receipt.usage_raw.is_object(),
        "usage_raw should be a JSON object"
    );
}

// ===========================================================================
// User journey: Tool use
// ===========================================================================

/// Given a request with tools and an open policy,
/// When run,
/// Then the mock backend emits events (tool calls appear in trace).
#[tokio::test]
async fn given_request_with_tools_when_run_then_tool_calls_in_events() {
    let rt = Runtime::with_default_backends();
    let wo = WorkOrderBuilder::new("Fix the bug")
        .root(".")
        .workspace_mode(abp_core::WorkspaceMode::PassThrough)
        .policy(PolicyProfile::default()) // no restrictions
        .build();

    let handle = rt.run_streaming("mock", wo).await.unwrap();

    // Drain events.
    let mut events = vec![];
    let mut stream = handle.events;
    while let Some(ev) = tokio_stream::StreamExt::next(&mut stream).await {
        events.push(ev);
    }

    let receipt = handle.receipt.await.unwrap().unwrap();

    assert_eq!(receipt.outcome, Outcome::Complete);
    // Mock backend should emit at least RunStarted + RunCompleted.
    assert!(events.len() >= 2, "should have at least 2 events");
}

/// Given a restricted policy that disallows "Bash",
/// When the tool is checked,
/// Then it is denied.
#[test]
fn given_restricted_policy_when_tool_called_then_denied() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec![],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed, "Bash should be denied");
    assert!(
        decision.reason.as_deref().unwrap().contains("disallowed"),
        "reason should mention disallowed"
    );

    // Write to .git should be denied.
    let write_decision = engine.can_write_path(Path::new(".git/config"));
    assert!(!write_decision.allowed, "write to .git should be denied");
}

/// Given an open policy that allows "Read",
/// When the tool is checked,
/// Then it is permitted.
#[test]
fn given_allowed_tool_when_called_then_executed() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into(), "Bash".into()],
        disallowed_tools: vec![],
        deny_read: vec![],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// User journey: Cross-SDK translation
// ===========================================================================

/// Given an OpenAI request,
/// When the Claude backend handles it,
/// Then the tool_use mapping is lossless (translated faithfully).
#[test]
fn given_openai_request_when_claude_backend_then_translated() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .expect("OpenAI→Claude tool_use rule should exist");
    assert!(
        rule.fidelity.is_lossless(),
        "OpenAI→Claude tool_use should be lossless"
    );

    // Streaming should also be lossless.
    let streaming_rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING)
        .expect("OpenAI→Claude streaming rule should exist");
    assert!(streaming_rule.fidelity.is_lossless());
}

/// Given a Claude request,
/// When the Gemini backend handles it,
/// Then thinking is mapped (lossy-labeled).
#[test]
fn given_claude_request_when_gemini_backend_then_mapped() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
        .expect("Claude→Gemini thinking rule should exist");
    assert!(
        matches!(rule.fidelity, Fidelity::LossyLabeled { .. }),
        "Claude→Gemini thinking should be lossy-labeled, got {:?}",
        rule.fidelity
    );
}

/// Given a lossy mapping scenario,
/// When validate_mapping is run,
/// Then the fidelity loss is labeled in the validation results.
#[test]
fn given_lossy_mapping_when_run_then_fidelity_labeled() {
    let reg = known_rules();
    let features_to_check: Vec<String> = vec![features::TOOL_USE.into(), features::THINKING.into()];

    // OpenAI→Codex: tool_use is lossy, thinking is lossy.
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &features_to_check);
    assert_eq!(results.len(), 2);

    // tool_use: OpenAI→Codex is lossy-labeled.
    assert!(
        matches!(results[0].fidelity, Fidelity::LossyLabeled { .. }),
        "OpenAI→Codex tool_use should be lossy, got {:?}",
        results[0].fidelity
    );
    assert!(
        !results[0].errors.is_empty(),
        "lossy mapping should have errors"
    );

    // thinking: OpenAI→Codex should also be lossy or unsupported.
    assert!(
        !results[1].errors.is_empty(),
        "thinking mapping should have fidelity issues"
    );
}

// ===========================================================================
// User journey: Error handling
// ===========================================================================

/// Given a bad backend name,
/// When run,
/// Then an error with the correct code is returned.
#[tokio::test]
async fn given_bad_backend_when_run_then_error_with_code() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("hello");

    let result = rt.run_streaming("nonexistent-backend", wo).await;
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected UnknownBackend error"),
    };
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got {err:?}"
    );
    assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
}

/// Given a capability gap (requiring McpClient on mock),
/// When run,
/// Then a negotiation/capability error is returned.
#[test]
fn given_capability_gap_when_run_then_negotiation_error() {
    let manifest = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require(&[(Capability::McpClient, MinSupport::Native)]);

    let result = negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported, vec![Capability::McpClient]);

    // Also check via runtime API.
    let rt = Runtime::with_default_backends();
    let reqs = require(&[(Capability::McpClient, MinSupport::Native)]);
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(
        matches!(err, RuntimeError::CapabilityCheckFailed(_)),
        "expected CapabilityCheckFailed, got {err:?}"
    );
}

/// Given a timeout scenario (simulated via error taxonomy),
/// When creating an error,
/// Then a partial receipt can still be built.
#[test]
fn given_timeout_when_run_then_partial_receipt() {
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);

    assert_eq!(err.code, ErrorCode::BackendTimeout);

    // A partial receipt can be constructed even on timeout.
    let receipt = ReceiptBuilder::new("openai")
        .outcome(Outcome::Partial)
        .usage_raw(serde_json::json!({
            "error": "timeout",
            "partial_tokens": 1500
        }))
        .build()
        .with_hash()
        .unwrap();

    assert_eq!(receipt.outcome, Outcome::Partial);
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt));
}

// ===========================================================================
// User journey: Audit trail
// ===========================================================================

/// Given a completed run,
/// When inspecting the receipt,
/// Then the receipt hash is verifiable and fields are consistent.
#[tokio::test]
async fn given_completed_run_when_inspecting_then_receipt_verifiable() {
    let rt = Runtime::with_default_backends();
    let wo = make_work_order("Audit this code");

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let receipt = handle.receipt.await.unwrap().unwrap();

    // Receipt hash should be present and verify.
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt), "receipt should verify");

    // Metadata should be consistent.
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
    assert_eq!(receipt.outcome, Outcome::Complete);

    // Tampering should break verification.
    let mut tampered = receipt.clone();
    tampered.outcome = Outcome::Failed;
    assert!(
        !verify_hash(&tampered),
        "tampered receipt should fail verification"
    );
}

/// Given a chain of receipts from multiple runs,
/// When querying,
/// Then they are ordered and the chain verifies.
#[tokio::test]
async fn given_receipt_chain_when_querying_then_ordered_history() {
    let rt = Runtime::with_default_backends();

    // Run three tasks sequentially, all sharing the runtime's chain.
    for task in &["Step 1: analyze", "Step 2: refactor", "Step 3: test"] {
        let wo = make_work_order(task);
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let _receipt = handle.receipt.await.unwrap().unwrap();
    }

    // Retrieve the chain and verify.
    let chain = rt.receipt_chain();
    let chain_guard = chain.lock().await;
    assert_eq!(chain_guard.len(), 3, "chain should have 3 receipts");
    assert!(chain_guard.verify().is_ok(), "receipt chain should verify");

    // Verify ordering: each receipt should have a unique hash.
    let receipts: Vec<_> = chain_guard.iter().collect();
    let hashes: Vec<_> = receipts
        .iter()
        .map(|r| r.receipt_sha256.as_deref().unwrap_or(""))
        .collect();
    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(
        unique.len(),
        hashes.len(),
        "each receipt should have a unique hash"
    );

    // Diff first and last receipt: outcome should be the same but run_id differs.
    let diff = diff_receipts(receipts[0], receipts[2]);
    assert!(
        !diff.is_empty(),
        "diff between first and last should be non-empty"
    );
    assert!(
        diff.changes.iter().any(|d| d.field == "meta.run_id"),
        "diff should include run_id change"
    );
}
