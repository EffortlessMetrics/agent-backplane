// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive ABP conformance test suite.
//!
//! Categories:
//! 1. Passthrough parity tests
//! 2. Mapped-mode contract tests
//! 3. Receipt correctness tests
//! 4. Error taxonomy coverage
//! 5. Protocol conformance

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    Outcome, PolicyProfile, Receipt, ReceiptBuilder, SupportLevel,
    WorkOrder, WorkOrderBuilder, WorkspaceMode,
    chain::ReceiptChain, receipt_hash,
};
use abp_integrations::projection::{
    Dialect, ProjectionMatrix, ToolCall, ToolResult, supported_translations, translate,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use abp_protocol::validate::EnvelopeValidator;
use abp_runtime::{Runtime, RuntimeError};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use tokio_stream::StreamExt;
use std::path::Path;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

async fn drain_run(
    handle: abp_runtime::RunHandle,
) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("backend task panicked");
    (collected, receipt)
}

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "conformance-test".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::new_v4())
        .with_hash()
        .expect("receipt hash")
}

fn encode_stream(envelopes: &[Envelope]) -> Vec<u8> {
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, envelopes).unwrap();
    buf
}

fn decode_all(buf: &[u8]) -> Vec<Result<Envelope, ProtocolError>> {
    let reader = std::io::BufReader::new(buf);
    JsonlCodec::decode_stream(reader).collect()
}

#[allow(unused)]
fn _decode_all_ok(buf: &[u8]) -> Vec<Envelope> {
    decode_all(buf).into_iter().map(|r| r.unwrap()).collect()
}

// ═════════════════════════════════════════════════════════════════════════
// CATEGORY 1: Passthrough parity tests
// ═════════════════════════════════════════════════════════════════════════

/// When dialect == engine (identity), translate returns the original WO as JSON.
#[test]
fn passthrough_identity_abp_returns_original() {
    let wo = simple_work_order("identity test");
    let original = serde_json::to_value(&wo).unwrap();
    let translated = translate(Dialect::Abp, Dialect::Abp, &wo).unwrap();
    assert_eq!(original, translated);
}

#[test]
fn passthrough_identity_claude_returns_original() {
    let wo = simple_work_order("claude identity");
    let original = serde_json::to_value(&wo).unwrap();
    let translated = translate(Dialect::Claude, Dialect::Claude, &wo).unwrap();
    assert_eq!(original, translated);
}

#[test]
fn passthrough_identity_openai_returns_original() {
    let wo = simple_work_order("openai identity");
    let original = serde_json::to_value(&wo).unwrap();
    let translated = translate(Dialect::OpenAi, Dialect::OpenAi, &wo).unwrap();
    assert_eq!(original, translated);
}

#[test]
fn passthrough_identity_gemini_returns_original() {
    let wo = simple_work_order("gemini identity");
    let original = serde_json::to_value(&wo).unwrap();
    let translated = translate(Dialect::Gemini, Dialect::Gemini, &wo).unwrap();
    assert_eq!(original, translated);
}

#[test]
fn passthrough_identity_codex_returns_original() {
    let wo = simple_work_order("codex identity");
    let original = serde_json::to_value(&wo).unwrap();
    let translated = translate(Dialect::Codex, Dialect::Codex, &wo).unwrap();
    assert_eq!(original, translated);
}

#[test]
fn passthrough_identity_kimi_returns_original() {
    let wo = simple_work_order("kimi identity");
    let original = serde_json::to_value(&wo).unwrap();
    let translated = translate(Dialect::Kimi, Dialect::Kimi, &wo).unwrap();
    assert_eq!(original, translated);
}

/// Streaming events arrive in temporal order through the mock backend.
#[tokio::test]
async fn passthrough_events_arrive_in_order() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("event order test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _receipt) = drain_run(handle).await;

    for window in events.windows(2) {
        assert!(
            window[1].ts >= window[0].ts,
            "events must be temporally ordered"
        );
    }
}

/// The first streamed event is RunStarted.
#[tokio::test]
async fn passthrough_first_event_is_run_started() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("first event test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;

    assert!(!events.is_empty());
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first event should be RunStarted, got {:?}",
        events[0].kind
    );
}

/// The last streamed event is RunCompleted.
#[tokio::test]
async fn passthrough_last_event_is_run_completed() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("last event test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, _) = drain_run(handle).await;

    assert!(!events.is_empty());
    assert!(
        matches!(
            &events[events.len() - 1].kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );
}

/// Receipt payload content is faithfully preserved (no alteration by framing).
#[tokio::test]
async fn passthrough_receipt_preserves_payload() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("payload preservation");
    let wo_id = wo.id;
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (events, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    // Work order ID is faithfully preserved.
    assert_eq!(receipt.meta.work_order_id, wo_id);
    // Backend identity is preserved.
    assert_eq!(receipt.backend.id, "mock");
    // The trace events match what was streamed.
    assert_eq!(receipt.trace.len(), events.len());
}

/// Identity tool-call translation preserves tool name and input.
#[test]
fn passthrough_tool_call_identity_preserves_content() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    };
    let result = pm.translate_tool_call("abp", "abp", &call).unwrap();
    assert_eq!(result.tool_name, call.tool_name);
    assert_eq!(result.input, call.input);
    assert_eq!(result.tool_use_id, call.tool_use_id);
}

/// Identity tool-result translation preserves output and error flag.
#[test]
fn passthrough_tool_result_identity_preserves_content() {
    let pm = ProjectionMatrix::new();
    let tr = ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        output: json!({"content": "fn main() {}"}),
        is_error: false,
    };
    let result = pm.translate_tool_result("abp", "abp", &tr).unwrap();
    assert_eq!(result.tool_name, tr.tool_name);
    assert_eq!(result.output, tr.output);
    assert_eq!(result.is_error, tr.is_error);
}

// ═════════════════════════════════════════════════════════════════════════
// CATEGORY 2: Mapped-mode contract tests
// ═════════════════════════════════════════════════════════════════════════

/// ABP→Claude translation produces a JSON value.
#[test]
fn mapped_abp_to_claude_produces_json() {
    let wo = simple_work_order("translate to claude");
    let result = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    assert!(result.is_object(), "translation should produce a JSON object");
}

/// ABP→OpenAI translation produces a JSON value.
#[test]
fn mapped_abp_to_openai_produces_json() {
    let wo = simple_work_order("translate to openai");
    let result = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    assert!(result.is_object());
}

/// ABP→Gemini translation produces a JSON value.
#[test]
fn mapped_abp_to_gemini_produces_json() {
    let wo = simple_work_order("translate to gemini");
    let result = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    assert!(result.is_object());
}

/// ABP→Codex translation produces a JSON value.
#[test]
fn mapped_abp_to_codex_produces_json() {
    let wo = simple_work_order("translate to codex");
    let result = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    assert!(result.is_object());
}

/// ABP→Kimi translation produces a JSON value.
#[test]
fn mapped_abp_to_kimi_produces_json() {
    let wo = simple_work_order("translate to kimi");
    let result = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    assert!(result.is_object());
}

/// Vendor→vendor (non-ABP source) is unsupported and produces an error.
#[test]
fn mapped_claude_to_openai_is_unsupported() {
    let wo = simple_work_order("claude to openai");
    let err = translate(Dialect::Claude, Dialect::OpenAi, &wo);
    assert!(err.is_err(), "vendor-to-vendor should fail in v0.1");
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("unsupported"),
        "error should mention 'unsupported': {msg}"
    );
}

/// OpenAI→Claude is also unsupported in v0.1.
#[test]
fn mapped_openai_to_claude_is_unsupported() {
    let wo = simple_work_order("openai to claude");
    let err = translate(Dialect::OpenAi, Dialect::Claude, &wo);
    assert!(err.is_err());
}

/// Gemini→OpenAI is unsupported in v0.1.
#[test]
fn mapped_gemini_to_openai_is_unsupported() {
    let wo = simple_work_order("gemini to openai");
    let err = translate(Dialect::Gemini, Dialect::OpenAi, &wo);
    assert!(err.is_err());
}

/// Tool name translation ABP→OpenAI maps read_file to file_read.
#[test]
fn mapped_tool_call_abp_to_openai() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    };
    let result = pm.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(result.tool_name, "file_read");
    // Input payload is preserved.
    assert_eq!(result.input, call.input);
}

/// Tool name translation ABP→Anthropic maps read_file to Read.
#[test]
fn mapped_tool_call_abp_to_anthropic() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/lib.rs"}),
    };
    let result = pm.translate_tool_call("abp", "anthropic", &call).unwrap();
    assert_eq!(result.tool_name, "Read");
}

/// Tool name translation ABP→Gemini maps read_file to readFile.
#[test]
fn mapped_tool_call_abp_to_gemini() {
    let pm = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "Cargo.toml"}),
    };
    let result = pm.translate_tool_call("abp", "gemini", &call).unwrap();
    assert_eq!(result.tool_name, "readFile");
}

/// supported_translations includes all identity + ABP-to-vendor pairs.
#[test]
fn mapped_supported_translations_coverage() {
    let pairs = supported_translations();
    // At least 6 identity + 5 ABP→vendor = 11
    assert!(
        pairs.len() >= 11,
        "expected >= 11 supported translations, got {}",
        pairs.len()
    );
    // All identity pairs should be present.
    for d in &[
        Dialect::Abp,
        Dialect::Claude,
        Dialect::OpenAi,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
    ] {
        assert!(
            pairs.contains(&(*d, *d)),
            "identity pair ({d:?}, {d:?}) should be supported"
        );
    }
    // ABP→Claude should be present.
    assert!(pairs.contains(&(Dialect::Abp, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Abp, Dialect::OpenAi)));
}

/// Event translation preserves timestamp and kind structure.
#[test]
fn mapped_event_translation_preserves_structure() {
    let pm = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "x.rs"}),
        },
        ext: None,
    };
    let translated = pm.translate_event("abp", "openai", &event).unwrap();
    assert_eq!(translated.ts, event.ts);
    if let AgentEventKind::ToolCall { tool_name, .. } = &translated.kind {
        assert_eq!(tool_name, "file_read");
    } else {
        panic!("expected ToolCall after translation");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// CATEGORY 3: Receipt correctness tests
// ═════════════════════════════════════════════════════════════════════════

/// Receipt hash is deterministic for the same input.
#[test]
fn receipt_hash_is_deterministic() {
    let receipt = make_receipt("test-backend");
    let h1 = receipt_hash(&receipt).unwrap();
    let h2 = receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2, "same receipt should produce identical hashes");
}

/// Receipt hash changes when the outcome changes.
#[test]
fn receipt_hash_changes_with_outcome() {
    let r1 = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("test")
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();
    assert_ne!(
        r1.receipt_sha256, r2.receipt_sha256,
        "different outcomes should produce different hashes"
    );
}

/// Receipt hash changes when the backend ID changes.
#[test]
fn receipt_hash_changes_with_backend() {
    let r1 = ReceiptBuilder::new("backend-a")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("backend-b")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

/// Receipt hash is a 64-char hex string (SHA-256).
#[test]
fn receipt_hash_is_valid_sha256_hex() {
    let receipt = make_receipt("test");
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash should contain only hex digits"
    );
}

/// receipt_hash() nulls sha256 before computing (self-referential prevention).
#[test]
fn receipt_hash_nulls_sha256_before_computing() {
    let mut receipt = make_receipt("test");
    let original_hash = receipt.receipt_sha256.clone().unwrap();

    // Manually set a bogus hash and recompute.
    receipt.receipt_sha256 = Some("bogus".into());
    let recomputed = receipt_hash(&receipt).unwrap();

    // Despite the bogus hash, recomputed should match the original.
    assert_eq!(
        recomputed, original_hash,
        "receipt_hash should null sha256 before hashing"
    );
}

/// Receipt chain verification succeeds for properly linked receipts.
#[tokio::test]
async fn receipt_chain_verification_succeeds() {
    let rt = Runtime::with_default_backends();
    let mut chain = ReceiptChain::new();

    for i in 0..3 {
        let wo = simple_work_order(&format!("chain {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        chain.push(receipt.unwrap()).unwrap();
    }
    chain.verify().expect("chain should verify");
}

/// Each receipt in a chain has a unique run_id.
#[tokio::test]
async fn receipt_chain_unique_run_ids() {
    let rt = Runtime::with_default_backends();
    let mut chain = ReceiptChain::new();

    for i in 0..3 {
        let wo = simple_work_order(&format!("unique id {i}"));
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        chain.push(receipt.unwrap()).unwrap();
    }

    let ids: std::collections::HashSet<_> = chain.iter().map(|r| r.meta.run_id).collect();
    assert_eq!(ids.len(), 3, "each chain receipt should have unique run_id");
}

/// Receipt includes timing metadata with non-zero duration.
#[tokio::test]
async fn receipt_timing_metadata_present() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("timing test");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

/// Receipt includes contract version.
#[tokio::test]
async fn receipt_includes_contract_version() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("version check");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

/// Receipt hash can be recomputed and verified.
#[tokio::test]
async fn receipt_hash_recomputation_matches() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("hash recompute");
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let (_, receipt) = drain_run(handle).await;
    let receipt = receipt.unwrap();

    let stored = receipt.receipt_sha256.as_ref().unwrap();
    let recomputed = receipt_hash(&receipt).unwrap();
    assert_eq!(stored, &recomputed);
}

/// ReceiptBuilder produces receipt with correct outcome.
#[test]
fn receipt_builder_sets_outcome() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("test")
            .outcome(outcome.clone())
            .build();
        assert_eq!(r.outcome, outcome);
    }
}

/// Receipt with_hash is idempotent (hashing a hashed receipt gives same hash).
#[test]
fn receipt_with_hash_idempotent() {
    let r = ReceiptBuilder::new("idem-test")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "with_hash should be idempotent on recomputation");
}

// ═════════════════════════════════════════════════════════════════════════
// CATEGORY 4: Error taxonomy coverage
// ═════════════════════════════════════════════════════════════════════════

/// Unknown backend produces RuntimeError::UnknownBackend.
#[tokio::test]
async fn error_unknown_backend() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("unknown backend test");
    let err = match rt.run_streaming("nonexistent", wo).await {
        Ok(_) => panic!("expected error for unknown backend"),
        Err(e) => e,
    };
    assert!(
        matches!(err, RuntimeError::UnknownBackend { .. }),
        "expected UnknownBackend, got {err:?}"
    );
}

/// UnknownBackend error has stable display message containing backend name.
#[tokio::test]
async fn error_unknown_backend_display_message() {
    let rt = Runtime::with_default_backends();
    let wo = simple_work_order("display test");
    let err = match rt.run_streaming("no_such_backend", wo).await {
        Ok(_) => panic!("expected error for unknown backend"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("no_such_backend"),
        "error message should mention the backend name: {msg}"
    );
}

/// Capability check failure produces CapabilityCheckFailed.
#[tokio::test]
async fn error_capability_check_failed() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    assert!(
        matches!(err, RuntimeError::CapabilityCheckFailed(_)),
        "expected CapabilityCheckFailed, got {err:?}"
    );
}

/// CapabilityCheckFailed has a non-empty display message.
#[tokio::test]
async fn error_capability_check_display() {
    let rt = Runtime::with_default_backends();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    let msg = err.to_string();
    assert!(!msg.is_empty(), "error display should be non-empty");
}

/// RuntimeError implements std::error::Error.
#[test]
fn error_runtime_error_is_std_error() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<RuntimeError>();
}

/// RuntimeError is Send + Sync.
#[test]
fn error_runtime_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeError>();
}

/// Policy engine denies disallowed tools.
#[test]
fn error_policy_denies_disallowed_tool() {
    let engine = PolicyEngine::new(&PolicyProfile {
        allowed_tools: vec![],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    })
    .unwrap();
    let decision = engine.can_use_tool("Bash");
    assert!(!decision.allowed, "Bash should be denied");
}

/// Policy engine allows permitted tools.
#[test]
fn error_policy_allows_permitted_tool() {
    let engine = PolicyEngine::new(&PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec![],
        ..PolicyProfile::default()
    })
    .unwrap();
    let decision = engine.can_use_tool("Read");
    assert!(decision.allowed, "Read should be allowed");
}

/// Policy engine enforces read path restrictions.
#[test]
fn error_policy_denies_read_path() {
    let engine = PolicyEngine::new(&PolicyProfile {
        deny_read: vec!["**/.env".into()],
        ..PolicyProfile::default()
    })
    .unwrap();
    let decision = engine.can_read_path(Path::new(".env"));
    assert!(!decision.allowed, ".env should be denied for reading");
}

/// Policy engine enforces write path restrictions.
#[test]
fn error_policy_denies_write_path() {
    let engine = PolicyEngine::new(&PolicyProfile {
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    })
    .unwrap();
    let decision = engine.can_write_path(Path::new(".git/config"));
    assert!(!decision.allowed, ".git/config should be denied for writing");
}

/// Vendor-to-vendor translation error is not a panic.
#[test]
fn error_unmappable_translation_no_panic() {
    let wo = simple_work_order("no panic test");
    // Should return Err, not panic.
    let result = translate(Dialect::Claude, Dialect::Gemini, &wo);
    assert!(result.is_err());
}

/// All runtime error variants have non-empty Debug output.
#[tokio::test]
async fn error_all_variants_have_debug() {
    let rt = Runtime::with_default_backends();

    // UnknownBackend
    let wo = simple_work_order("debug test");
    let err = match rt.run_streaming("nope", wo).await {
        Ok(_) => panic!("expected error for unknown backend"),
        Err(e) => e,
    };
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());

    // CapabilityCheckFailed
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let err = rt.check_capabilities("mock", &reqs).unwrap_err();
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());
}

// ═════════════════════════════════════════════════════════════════════════
// CATEGORY 5: Protocol conformance
// ═════════════════════════════════════════════════════════════════════════

/// Envelope serialization uses `t` field as discriminator.
#[test]
fn protocol_envelope_uses_t_discriminator() {
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let json = serde_json::to_value(&hello).unwrap();
    assert!(json.get("t").is_some(), "envelope must use 't' field");
    assert!(
        json.get("type").is_none(),
        "envelope must NOT use 'type' field"
    );
    assert_eq!(json["t"].as_str().unwrap(), "hello");
}

/// Each envelope variant uses the correct `t` value.
#[test]
fn protocol_envelope_t_values() {
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let json = serde_json::to_value(&hello).unwrap();
    assert_eq!(json["t"], "hello");

    let wo = simple_work_order("test");
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = serde_json::to_value(&run).unwrap();
    assert_eq!(json["t"], "run");

    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "hi".into(),
            },
            ext: None,
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["t"], "event");

    let receipt = make_receipt("test");
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = serde_json::to_value(&fin).unwrap();
    assert_eq!(json["t"], "final");

    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
    };
    let json = serde_json::to_value(&fatal).unwrap();
    assert_eq!(json["t"], "fatal");
}

/// Hello envelope round-trips through JSONL encode/decode.
#[test]
fn protocol_hello_roundtrip() {
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello {
        contract_version,
        backend,
        ..
    } = decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
        assert_eq!(backend.id, "conformance-test");
    } else {
        panic!("expected Hello");
    }
}

/// Run envelope contains a valid WorkOrder with task field.
#[test]
fn protocol_run_contains_valid_work_order() {
    let wo = simple_work_order("protocol test");
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo.clone(),
    };
    let json = serde_json::to_value(&run).unwrap();
    let wo_json = &json["work_order"];
    assert!(wo_json.is_object());
    assert_eq!(wo_json["task"].as_str().unwrap(), "protocol test");
}

/// Event envelope carries a valid AgentEvent.
#[test]
fn protocol_event_carries_valid_agent_event() {
    let event = Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
            ext: None,
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert!(json["event"].is_object());
    assert!(json["event"]["ts"].is_string());
    // AgentEventKind uses #[serde(tag = "type")] so the kind fields
    // are flattened into the event object with a "type" discriminator.
    assert!(
        json["event"]["type"].is_string(),
        "event kind should have 'type' discriminator"
    );
}

/// Final envelope contains a valid Receipt with outcome.
#[test]
fn protocol_final_contains_valid_receipt() {
    let receipt = make_receipt("test");
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = serde_json::to_value(&fin).unwrap();
    assert!(json["receipt"].is_object());
    assert!(json["receipt"]["outcome"].is_string());
    assert!(json["receipt"]["meta"].is_object());
}

/// Fatal envelope contains error info.
#[test]
fn protocol_fatal_contains_error_info() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something went wrong".into(),
    };
    let json = serde_json::to_value(&fatal).unwrap();
    assert_eq!(json["error"].as_str().unwrap(), "something went wrong");
    assert_eq!(json["ref_id"].as_str().unwrap(), "run-1");
}

/// Fatal envelope can have null ref_id.
#[test]
fn protocol_fatal_null_ref_id() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "early failure".into(),
    };
    let json = serde_json::to_value(&fatal).unwrap();
    assert!(
        json["ref_id"].is_null(),
        "ref_id should be null when not set"
    );
}

/// JSONL stream encoding produces one line per envelope.
#[test]
fn protocol_jsonl_one_line_per_envelope() {
    let envelopes = vec![
        Envelope::hello(test_backend(), test_capabilities()),
        Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: make_receipt("test"),
        },
    ];
    let buf = encode_stream(&envelopes);
    let text = String::from_utf8(buf).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(
        lines.len(),
        envelopes.len(),
        "each envelope should be exactly one line"
    );
}

/// Ref ID correlation: all events in a stream should reference the same run.
#[test]
fn protocol_ref_id_correlation() {
    let run_id = "run-corr-1";
    let envelopes = vec![
        Envelope::hello(test_backend(), test_capabilities()),
        Envelope::Event {
            ref_id: run_id.into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "start".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: run_id.into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "msg".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: run_id.into(),
            receipt: make_receipt("test"),
        },
    ];

    for env in &envelopes {
        match env {
            Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => {
                assert_eq!(ref_id, run_id, "all ref_ids should match the run id");
            }
            _ => {}
        }
    }
}

/// EnvelopeValidator validates a well-formed Hello envelope.
#[test]
fn protocol_validator_accepts_valid_hello() {
    let v = EnvelopeValidator::new();
    let hello = Envelope::hello(test_backend(), test_capabilities());
    let result = v.validate(&hello);
    assert!(
        result.valid,
        "valid Hello should pass: {:?}",
        result.errors
    );
}

/// EnvelopeValidator detects sequence errors when Hello is not first.
#[test]
fn protocol_validator_sequence_hello_not_first() {
    let v = EnvelopeValidator::new();
    let envelopes = vec![
        Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunStarted {
                    message: "bad".into(),
                },
                ext: None,
            },
        },
        Envelope::hello(test_backend(), test_capabilities()),
    ];
    let errors = v.validate_sequence(&envelopes);
    assert!(
        !errors.is_empty(),
        "non-Hello first should produce sequence errors"
    );
}

/// JSONL decode rejects invalid JSON.
#[test]
fn protocol_decode_rejects_invalid_json() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
}

/// Envelope round-trips all variant types through JSONL.
#[test]
fn protocol_all_variants_roundtrip() {
    let envelopes = vec![
        Envelope::hello(test_backend(), test_capabilities()),
        Envelope::Run {
            id: "r1".into(),
            work_order: simple_work_order("rt"),
        },
        Envelope::Event {
            ref_id: "r1".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "hi".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "r1".into(),
            receipt: make_receipt("test"),
        },
        Envelope::Fatal {
            ref_id: Some("r1".into()),
            error: "boom".into(),
        },
    ];

    for env in &envelopes {
        let line = JsonlCodec::encode(env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        // Verify the discriminator matches.
        let orig_json = serde_json::to_value(env).unwrap();
        let dec_json = serde_json::to_value(&decoded).unwrap();
        assert_eq!(
            orig_json["t"], dec_json["t"],
            "discriminator should survive round-trip"
        );
    }
}
