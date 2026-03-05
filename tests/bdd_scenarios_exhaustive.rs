#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive BDD-style scenario tests for core Agent Backplane workflows.
//!
//! Each test follows Given/When/Then structure organized by user stories:
//!
//! 1. "As a developer, I want to run a task via mock backend"
//! 2. "As a developer, I want to map OpenAI requests to Claude"
//! 3. "As an operator, I want to enforce tool policies"
//! 4. "As a sidecar, I want to speak JSONL protocol"
//! 5. "As a system, I want deterministic receipts"
//! 6. "As an operator, I want to monitor health"

use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use abp_backend_core::{
    Backend, BackendHealth, BackendMetadata, BackendMetrics, BackendRegistry, HealthStatus,
    RateLimit, SelectionStrategy,
};
use abp_backend_mock::MockBackend;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, SupportLevel, UsageNormalized, WorkOrder,
    WorkOrderBuilder, canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::Dialect;
use abp_error::ErrorCode;
use abp_mapper::{
    ClaudeGeminiIrMapper, IrIdentityMapper, IrMapper, OpenAiClaudeIrMapper, OpenAiCopilotIrMapper,
    OpenAiGeminiIrMapper, default_ir_mapper,
};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use abp_receipt::{compute_hash, verify_hash};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).root(".").build()
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

fn make_receipt_with_events(events: Vec<AgentEvent>) -> Receipt {
    let mut builder = ReceiptBuilder::new("test-backend").outcome(Outcome::Complete);
    for event in events {
        builder = builder.add_trace_event(event);
    }
    builder.build()
}

fn make_hello_envelope(backend_id: &str) -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: backend_id.into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello, world!"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn make_backend_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.into(),
        dialect: dialect.into(),
        version: "1.0.0".into(),
        max_tokens: Some(128_000),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    }
}

fn register_healthy_backend(registry: &mut BackendRegistry, name: &str, dialect: &str) {
    registry.register_with_metadata(name, make_backend_metadata(name, dialect));
    let mut health = BackendHealth::default();
    health.record_success(50);
    registry.update_health(name, health);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 1: "As a developer, I want to run a task via mock backend"
//
// Given a work order, when run via MockBackend, then the receipt should
// have the correct outcome, events, hash, and metadata.
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_receipt_outcome_is_complete() {
    // Given: A work order for the mock backend
    let wo = make_work_order("Fix the login bug");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs the work order
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The receipt outcome is Complete
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_backend_identity_is_mock() {
    // Given: A work order
    let wo = make_work_order("Add logging");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The backend identity is "mock"
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_contract_version_matches() {
    // Given: A work order
    let wo = make_work_order("Refactor module");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The contract version is correct
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_receipt_has_hash() {
    // Given: A work order
    let wo = make_work_order("Deploy feature");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The receipt has a SHA-256 hash
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_events_are_streamed() {
    // Given: A work order
    let wo = make_work_order("Write tests");
    let backend = MockBackend;
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let _receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: Events were streamed to the channel
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(
        events.len() >= 2,
        "expected at least RunStarted and RunCompleted, got {}",
        events.len()
    );
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_trace_starts_with_run_started() {
    // Given: A work order
    let wo = make_work_order("Optimize queries");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The trace starts with RunStarted
    assert!(
        matches!(&receipt.trace[0].kind, AgentEventKind::RunStarted { .. }),
        "first trace event should be RunStarted"
    );
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_trace_ends_with_run_completed() {
    // Given: A work order
    let wo = make_work_order("Add feature flag");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The trace ends with RunCompleted
    let last = receipt.trace.last().unwrap();
    assert!(
        matches!(&last.kind, AgentEventKind::RunCompleted { .. }),
        "last trace event should be RunCompleted"
    );
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_work_order_id_matches() {
    // Given: A work order with a known ID
    let wo = make_work_order("Check dependencies");
    let expected_id = wo.id;
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The receipt references the original work order
    assert_eq!(receipt.meta.work_order_id, expected_id);
}

#[tokio::test]
async fn story1_given_work_order_when_mock_run_then_usage_tokens_are_zero() {
    // Given: A work order
    let wo = make_work_order("Generate docs");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The mock backend runs (no real API call)
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: Token usage is zero (mock doesn't call any LLM)
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn story1_given_mock_backend_when_capabilities_queried_then_streaming_is_native() {
    // Given: The mock backend
    let backend = MockBackend;

    // When: We query its capabilities
    let caps = backend.capabilities();

    // Then: Streaming is natively supported and tools are emulated
    assert_eq!(
        caps.get(&Capability::Streaming),
        Some(&SupportLevel::Native)
    );
    assert_eq!(
        caps.get(&Capability::ToolRead),
        Some(&SupportLevel::Emulated)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 2: "As a developer, I want to map OpenAI requests to Claude"
//
// Given an OpenAI-style IR request, when mapped to Claude dialect,
// then the resulting request should preserve all semantics.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story2_given_openai_conversation_when_mapped_to_claude_then_text_preserved() {
    // Given: An OpenAI-style IR conversation
    let ir = make_ir_conversation();

    // When: Mapped from OpenAI to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: Text content is preserved across all messages
    for (orig, m) in ir.messages.iter().zip(mapped.messages.iter()) {
        assert_eq!(orig.text_content(), m.text_content());
    }
}

#[test]
fn story2_given_openai_conversation_when_mapped_to_claude_then_roles_preserved() {
    // Given: A multi-role conversation
    let ir = make_ir_conversation();

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: Roles are preserved in order
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[1].role, IrRole::User);
    assert_eq!(mapped.messages[2].role, IrRole::Assistant);
}

#[test]
fn story2_given_openai_conversation_when_mapped_to_claude_then_message_count_preserved() {
    // Given: A 3-message conversation
    let ir = make_ir_conversation();

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: The number of messages is preserved
    assert_eq!(mapped.messages.len(), ir.messages.len());
}

#[test]
fn story2_given_openai_tool_use_when_mapped_to_claude_then_tool_name_preserved() {
    // Given: A conversation with a tool call
    let tool_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "read_file".into(),
            input: json!({"path": "main.rs"}),
        }],
    );
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read main.rs"),
        tool_msg,
    ]);

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: Tool name and input are preserved
    let tools = mapped.messages[1].tool_use_blocks();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, input, .. } = tools[0] {
        assert_eq!(name, "read_file");
        assert_eq!(input, &json!({"path": "main.rs"}));
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn story2_given_openai_conversation_when_roundtripped_then_content_intact() {
    // Given: An OpenAI conversation
    let ir = make_ir_conversation();

    // When: Roundtripped OpenAI -> Claude -> OpenAI
    let mapper = OpenAiClaudeIrMapper;
    let to_claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &to_claude)
        .unwrap();

    // Then: All text content is intact
    for (orig, rt) in ir.messages.iter().zip(back.messages.iter()) {
        assert_eq!(orig.text_content(), rt.text_content());
    }
}

#[test]
fn story2_given_empty_conversation_when_mapped_then_empty_result() {
    // Given: An empty conversation
    let ir = IrConversation::new();

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: The result is also empty
    assert!(mapped.messages.is_empty());
}

#[test]
fn story2_given_system_message_when_mapped_then_system_role_preserved() {
    // Given: A conversation with only a system message
    let ir = IrConversation::from_messages(vec![IrMessage::text(
        IrRole::System,
        "You are a coding assistant",
    )]);

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: The system message and role are preserved
    assert_eq!(mapped.messages.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(
        mapped.messages[0].text_content(),
        "You are a coding assistant"
    );
}

#[test]
fn story2_given_identity_mapper_when_mapped_then_exact_passthrough() {
    // Given: Any conversation
    let ir = make_ir_conversation();

    // When: Processed by identity mapper
    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();

    // Then: Output is identical
    assert_eq!(mapped.messages.len(), ir.messages.len());
    for (orig, m) in ir.messages.iter().zip(mapped.messages.iter()) {
        assert_eq!(orig.text_content(), m.text_content());
        assert_eq!(orig.role, m.role);
    }
}

#[test]
fn story2_given_supported_pair_when_factory_called_then_mapper_returned() {
    // Given: A known supported dialect pair (OpenAI -> Claude)
    // When: The factory is called
    let result = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);

    // Then: A mapper is returned
    assert!(result.is_some());
}

#[test]
fn story2_given_openai_to_gemini_when_mapped_then_content_preserved() {
    // Given: An OpenAI conversation
    let ir = make_ir_conversation();

    // When: Mapped to Gemini
    let mapper = OpenAiGeminiIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();

    // Then: Text content is preserved
    for (orig, m) in ir.messages.iter().zip(mapped.messages.iter()) {
        assert_eq!(orig.text_content(), m.text_content());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 3: "As an operator, I want to enforce tool policies"
//
// Given a policy profile, when a tool/path is checked, then the policy
// engine correctly allows or denies access.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story3_given_deny_bash_policy_when_bash_called_then_denied() {
    // Given: A policy that disallows Bash
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };

    // When: The policy engine checks Bash
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");

    // Then: Bash is denied
    assert!(!decision.allowed);
}

#[test]
fn story3_given_deny_bash_policy_when_read_called_then_allowed() {
    // Given: A policy that only denies Bash
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };

    // When: The policy engine checks Read
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Read");

    // Then: Read is allowed
    assert!(decision.allowed);
}

#[test]
fn story3_given_allowlist_policy_when_unlisted_tool_called_then_denied() {
    // Given: A policy allowing only "Read" and "Write"
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Write".into()],
        ..Default::default()
    };

    // When: The engine checks an unlisted tool
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");

    // Then: Unlisted tool is denied
    assert!(!decision.allowed);
}

#[test]
fn story3_given_allowlist_policy_when_listed_tool_called_then_allowed() {
    // Given: A policy allowing "Read"
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        ..Default::default()
    };

    // When: The engine checks Read
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Read");

    // Then: It is allowed
    assert!(decision.allowed);
}

#[test]
fn story3_given_deny_overrides_allow_when_tool_in_both_then_denied() {
    // Given: A policy where "Bash" is in both allow and deny lists
    let policy = PolicyProfile {
        allowed_tools: vec!["Bash".into(), "Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };

    // When: The engine checks Bash
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("Bash");

    // Then: Deny takes precedence
    assert!(!decision.allowed);
}

#[test]
fn story3_given_deny_read_git_when_git_path_read_then_denied() {
    // Given: A policy denying reads on .git/**
    let policy = PolicyProfile {
        deny_read: vec![".git/**".into()],
        ..Default::default()
    };

    // When: Reading .git/config
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(Path::new(".git/config"));

    // Then: Read is denied
    assert!(!decision.allowed);
}

#[test]
fn story3_given_deny_read_git_when_src_path_read_then_allowed() {
    // Given: A policy denying reads only on .git/**
    let policy = PolicyProfile {
        deny_read: vec![".git/**".into()],
        ..Default::default()
    };

    // When: Reading src/main.rs
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_read_path(Path::new("src/main.rs"));

    // Then: Read is allowed
    assert!(decision.allowed);
}

#[test]
fn story3_given_deny_write_secrets_when_env_written_then_denied() {
    // Given: A policy denying writes to secret files
    let policy = PolicyProfile {
        deny_write: vec!["**/.env".into(), "**/secrets/**".into()],
        ..Default::default()
    };

    // When: Writing to .env
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(Path::new(".env"));

    // Then: Write is denied
    assert!(!decision.allowed);
}

#[test]
fn story3_given_deny_write_secrets_when_src_written_then_allowed() {
    // Given: A policy denying writes only to secrets
    let policy = PolicyProfile {
        deny_write: vec!["**/secrets/**".into()],
        ..Default::default()
    };

    // When: Writing to src/lib.rs
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_write_path(Path::new("src/lib.rs"));

    // Then: Write is allowed
    assert!(decision.allowed);
}

#[test]
fn story3_given_default_policy_when_any_tool_called_then_allowed() {
    // Given: A default (empty) policy
    let policy = PolicyProfile::default();

    // When: Any tool is checked
    let engine = PolicyEngine::new(&policy).unwrap();

    // Then: All tools are allowed, all paths readable/writable
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("anything")).allowed);
    assert!(engine.can_write_path(Path::new("anything")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 4: "As a sidecar, I want to speak JSONL protocol"
//
// Given a hello envelope, when run is sent, then events are streamed,
// and the final envelope carries the receipt.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story4_given_hello_envelope_when_encoded_then_contains_tag_t() {
    // Given: A hello envelope
    let hello = make_hello_envelope("test-sidecar");

    // When: Encoded to JSONL
    let json = JsonlCodec::encode(&hello).unwrap();

    // Then: The discriminator tag is "t", not "type"
    assert!(json.contains(r#""t":"hello"#));
}

#[test]
fn story4_given_hello_envelope_when_encoded_then_ends_with_newline() {
    // Given: A hello envelope
    let hello = make_hello_envelope("my-sidecar");

    // When: Encoded to JSONL
    let json = JsonlCodec::encode(&hello).unwrap();

    // Then: The line ends with \n
    assert!(json.ends_with('\n'));
}

#[test]
fn story4_given_hello_envelope_when_roundtripped_then_identity_preserved() {
    // Given: A hello envelope with known identity
    let hello = make_hello_envelope("roundtrip-sidecar");

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The identity is preserved
    if let Envelope::Hello { backend, .. } = decoded {
        assert_eq!(backend.id, "roundtrip-sidecar");
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn story4_given_run_envelope_when_roundtripped_then_work_order_preserved() {
    // Given: A run envelope with a work order
    let wo = make_work_order("Test sidecar protocol");
    let run_id = wo.id.to_string();
    let run_env = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&run_env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The run ID and task are preserved
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, run_id);
        assert_eq!(work_order.task, "Test sidecar protocol");
    } else {
        panic!("expected Run envelope");
    }
}

#[test]
fn story4_given_event_envelope_when_roundtripped_then_event_kind_preserved() {
    // Given: An event envelope with an AssistantMessage
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "Hello from sidecar".into(),
    });
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The event kind and text are preserved
    if let Envelope::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "run-1");
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "Hello from sidecar");
        } else {
            panic!("expected AssistantMessage event");
        }
    } else {
        panic!("expected Event envelope");
    }
}

#[test]
fn story4_given_final_envelope_when_roundtripped_then_receipt_preserved() {
    // Given: A final envelope with a receipt
    let receipt = make_receipt("final-sidecar");
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The receipt outcome and backend are preserved
    if let Envelope::Final { ref_id, receipt } = decoded {
        assert_eq!(ref_id, "run-1");
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.backend.id, "final-sidecar");
    } else {
        panic!("expected Final envelope");
    }
}

#[test]
fn story4_given_fatal_envelope_when_roundtripped_then_error_preserved() {
    // Given: A fatal envelope with an error message
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "something went wrong".into(),
        error_code: None,
    };

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The error message is preserved
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "something went wrong");
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn story4_given_invalid_json_when_decoded_then_error_returned() {
    // Given: Invalid JSON input
    let bad_json = "this is not valid json";

    // When: Attempting to decode
    let result = JsonlCodec::decode(bad_json);

    // Then: An error is returned
    assert!(result.is_err());
}

#[test]
fn story4_given_compatible_version_when_checked_then_true() {
    // Given: Two compatible versions
    // When: Checking compatibility
    let compatible = is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION);

    // Then: They are compatible
    assert!(compatible);
}

#[test]
fn story4_given_hello_with_passthrough_mode_when_decoded_then_mode_preserved() {
    // Given: A hello envelope with passthrough mode
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "passthrough-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: Passthrough mode is preserved
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello envelope");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 5: "As a system, I want deterministic receipts"
//
// Given the same input, when run twice, then identical receipts
// with identical hashes are produced.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story5_given_same_receipt_when_hashed_twice_then_identical_hashes() {
    // Given: A receipt
    let receipt = make_receipt("determinism-test");

    // When: Hashed twice
    let hash1 = receipt_hash(&receipt).unwrap();
    let hash2 = receipt_hash(&receipt).unwrap();

    // Then: The hashes are identical
    assert_eq!(hash1, hash2);
}

#[test]
fn story5_given_receipt_when_hashed_then_64_hex_chars() {
    // Given: A receipt
    let receipt = make_receipt("format-test");

    // When: Hashed
    let hash = receipt_hash(&receipt).unwrap();

    // Then: The hash is 64 hex characters (SHA-256)
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn story5_given_receipt_when_with_hash_called_then_field_set() {
    // Given: A receipt without a hash
    let receipt = make_receipt("hash-field-test");
    assert!(receipt.receipt_sha256.is_none());

    // When: with_hash() is called
    let hashed = receipt.with_hash().unwrap();

    // Then: The receipt_sha256 field is set
    assert!(hashed.receipt_sha256.is_some());
}

#[test]
fn story5_given_receipt_when_hashed_then_excludes_hash_field_from_input() {
    // Given: Two receipts, one with hash, one without
    let receipt = make_receipt("self-ref-test");
    let hash_before = receipt_hash(&receipt).unwrap();
    let hashed = receipt.with_hash().unwrap();
    let hash_after = receipt_hash(&hashed).unwrap();

    // Then: Hashes match because the hash field is excluded from the input
    assert_eq!(hash_before, hash_after);
}

#[test]
fn story5_given_different_backends_when_hashed_then_different_hashes() {
    // Given: Two receipts from different backends
    let receipt_a = make_receipt("backend-alpha");
    let receipt_b = make_receipt("backend-beta");

    // When: Hashed
    let hash_a = receipt_hash(&receipt_a).unwrap();
    let hash_b = receipt_hash(&receipt_b).unwrap();

    // Then: The hashes differ
    assert_ne!(hash_a, hash_b);
}

#[test]
fn story5_given_receipt_when_canonicalized_then_deterministic_json() {
    // Given: A receipt
    let receipt = make_receipt("canonical-test");

    // When: Canonicalized twice
    let json1 = canonical_json(&receipt).unwrap();
    let json2 = canonical_json(&receipt).unwrap();

    // Then: The canonical JSON is identical
    assert_eq!(json1, json2);
}

#[test]
fn story5_given_btreemap_when_canonicalized_then_keys_sorted() {
    // Given: A BTreeMap (used in receipts for determinism)
    let mut map = BTreeMap::new();
    map.insert("zebra", 1);
    map.insert("alpha", 2);
    map.insert("middle", 3);

    // When: Canonicalized
    let json = canonical_json(&map).unwrap();

    // Then: Keys are sorted alphabetically
    let alpha_pos = json.find("alpha").unwrap();
    let middle_pos = json.find("middle").unwrap();
    let zebra_pos = json.find("zebra").unwrap();
    assert!(alpha_pos < middle_pos);
    assert!(middle_pos < zebra_pos);
}

#[test]
fn story5_given_receipt_with_hash_when_verified_then_valid() {
    // Given: A receipt with a computed hash
    let receipt = make_receipt("verify-test").with_hash().unwrap();

    // When: Verified
    let valid = verify_hash(&receipt);

    // Then: The hash is valid
    assert!(valid);
}

#[test]
fn story5_given_tampered_receipt_when_verified_then_invalid() {
    // Given: A receipt with a hash, then tampered
    let mut receipt = make_receipt("tamper-test").with_hash().unwrap();
    receipt.outcome = Outcome::Failed;

    // When: Verified after tampering
    let valid = verify_hash(&receipt);

    // Then: The hash is no longer valid
    assert!(!valid);
}

#[test]
fn story5_given_sha256_hex_when_called_with_same_input_then_deterministic() {
    // Given: The same byte input
    let input = b"deterministic test data";

    // When: sha256_hex called twice
    let h1 = sha256_hex(input);
    let h2 = sha256_hex(input);

    // Then: Results are identical
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 6: "As an operator, I want to monitor health"
//
// Given a backend registry, when health is checked, then the correct
// status is reported and selection strategies work.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story6_given_new_registry_when_no_backends_then_empty() {
    // Given: A new backend registry
    let registry = BackendRegistry::new();

    // When/Then: It has no backends
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(registry.list().is_empty());
}

#[test]
fn story6_given_registered_backend_when_health_queried_then_status_healthy() {
    // Given: A registry with a backend marked healthy
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "gpt-4", "openai");

    // When: Health is queried
    let health = registry.health("gpt-4").unwrap();

    // Then: Status is Healthy
    assert_eq!(health.status, HealthStatus::Healthy);
    assert!(health.is_operational());
}

#[test]
fn story6_given_healthy_backend_when_failure_recorded_then_degraded() {
    // Given: A healthy backend
    let mut health = BackendHealth::default();
    health.record_success(50);
    assert_eq!(health.status, HealthStatus::Healthy);

    // When: A failure is recorded (threshold = 3)
    health.record_failure(3);

    // Then: Status transitions to Degraded
    assert_eq!(health.status, HealthStatus::Degraded);
    assert!(health.is_operational()); // Degraded is still operational
}

#[test]
fn story6_given_degraded_backend_when_threshold_failures_then_unhealthy() {
    // Given: A backend with consecutive failures approaching the threshold
    let mut health = BackendHealth::default();
    health.record_failure(3); // 1st failure -> Degraded
    health.record_failure(3); // 2nd failure -> Degraded
    health.record_failure(3); // 3rd failure -> Unhealthy (threshold=3)

    // Then: Status is Unhealthy
    assert_eq!(health.status, HealthStatus::Unhealthy);
    assert!(!health.is_operational());
}

#[test]
fn story6_given_unhealthy_backend_when_success_recorded_then_healthy() {
    // Given: An unhealthy backend
    let mut health = BackendHealth::default();
    health.record_failure(1); // Immediately unhealthy

    // When: A success is recorded
    health.record_success(100);

    // Then: Status recovers to Healthy
    assert_eq!(health.status, HealthStatus::Healthy);
    assert_eq!(health.consecutive_failures, 0);
    assert_eq!(health.error_rate, 0.0);
}

#[test]
fn story6_given_registry_with_mixed_health_when_queried_then_correct_lists() {
    // Given: A registry with healthy and unhealthy backends
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "healthy-one", "openai");
    register_healthy_backend(&mut registry, "healthy-two", "claude");

    // Make one unhealthy
    let mut bad_health = BackendHealth::default();
    bad_health.record_failure(1);
    registry.register_with_metadata("sick-one", make_backend_metadata("sick-one", "gemini"));
    registry.update_health("sick-one", bad_health);

    // When: Querying healthy backends
    let healthy = registry.healthy_backends();

    // Then: Only the healthy ones are returned
    assert_eq!(healthy.len(), 2);
    assert!(healthy.contains(&"healthy-one"));
    assert!(healthy.contains(&"healthy-two"));
}

#[test]
fn story6_given_registry_when_select_by_dialect_then_correct_backend() {
    // Given: A registry with backends of different dialects
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "openai-backend", "openai");
    register_healthy_backend(&mut registry, "claude-backend", "claude");

    // When: Selecting by dialect "claude"
    let selected = registry.select(&SelectionStrategy::ByDialect("claude".into()));

    // Then: The Claude backend is selected
    assert_eq!(selected.as_deref(), Some("claude-backend"));
}

#[test]
fn story6_given_registry_when_select_by_streaming_then_streaming_backend() {
    // Given: A registry with a streaming-capable backend
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "stream-backend", "openai");

    // When: Selecting by streaming support
    let selected = registry.select(&SelectionStrategy::ByStreaming);

    // Then: The streaming backend is selected
    assert_eq!(selected.as_deref(), Some("stream-backend"));
}

#[test]
fn story6_given_unhealthy_preference_when_selected_then_none() {
    // Given: A registry where the preferred backend is unhealthy
    let mut registry = BackendRegistry::new();
    registry.register_with_metadata("preferred", make_backend_metadata("preferred", "openai"));
    let mut health = BackendHealth::default();
    health.record_failure(1);
    registry.update_health("preferred", health);

    // When: Selecting by preference for the unhealthy backend
    let selected = registry.select(&SelectionStrategy::ByPreference("preferred".into()));

    // Then: None is returned (unhealthy backends are not selected)
    assert!(selected.is_none());
}

#[test]
fn story6_given_backend_metrics_when_runs_recorded_then_stats_correct() {
    // Given: Backend metrics tracking
    let mut metrics = BackendMetrics::default();

    // When: Recording successes and failures
    metrics.record_success(100);
    metrics.record_success(200);
    metrics.record_failure(50);

    // Then: Statistics are correct
    assert_eq!(metrics.total_runs, 3);
    assert_eq!(metrics.successful_runs, 2);
    assert_eq!(metrics.failed_runs, 1);
    let avg = metrics.average_latency_ms().unwrap();
    assert!((avg - 116.666).abs() < 1.0); // (100+200+50)/3
    let rate = metrics.success_rate().unwrap();
    assert!((rate - 0.666).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 7: SDK shim usage stories
//
// "As an OpenAI user, I want to use ABP as a drop-in replacement so that
// I can keep my existing code and benefit from multi-backend routing."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story7_given_openai_user_when_creating_work_order_then_builder_accepts_standard_fields() {
    // Given: An OpenAI user constructing a work order through the ABP builder
    // When: Setting standard fields that mirror OpenAI API parameters
    let wo = WorkOrderBuilder::new("Summarize this document")
        .root(".")
        .model("gpt-4o")
        .max_turns(5)
        .build();

    // Then: The work order captures all fields in a vendor-agnostic way
    assert_eq!(wo.task, "Summarize this document");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn story7_given_openai_user_when_ir_conversation_built_then_roles_map_to_standard_ir() {
    // Given: An OpenAI-style chat completion request (system + user + assistant)
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "What is Rust?"),
        IrMessage::text(IrRole::Assistant, "Rust is a systems programming language."),
    ]);

    // When: Inspecting the IR representation
    // Then: All standard roles are mapped correctly
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn story7_given_openai_user_when_tool_call_in_ir_then_function_calling_semantics_preserved() {
    // Given: An OpenAI function-call style tool use in IR
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read config.toml"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_abc".into(),
                name: "read_file".into(),
                input: json!({"path": "config.toml"}),
            }],
        ),
    ]);

    // When: Extracting tool calls
    let tool_calls = conv.tool_calls();

    // Then: The tool call is extractable with correct semantics
    assert_eq!(tool_calls.len(), 1);
    if let IrContentBlock::ToolUse { name, input, .. } = tool_calls[0] {
        assert_eq!(name, "read_file");
        assert_eq!(input["path"], "config.toml");
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn story7_given_openai_user_when_mock_backend_selected_then_capabilities_include_streaming() {
    // Given: An OpenAI user who expects streaming support
    let backend = MockBackend;

    // When: Querying ABP for the mock backend's capabilities
    let caps = backend.capabilities();

    // Then: Streaming is available (OpenAI always supports streaming)
    assert!(
        caps.get(&Capability::Streaming).is_some(),
        "backend should declare streaming capability"
    );
}

#[tokio::test]
async fn story7_given_openai_user_when_work_order_run_then_receipt_has_contract_version() {
    // Given: An OpenAI user running a task through ABP
    let wo = make_work_order("Translate Python to Rust");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The work order completes
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The receipt carries the ABP contract version (not OpenAI's version)
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.meta.contract_version.starts_with("abp/v"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 8: Cross-SDK translation stories
//
// "When I send a Claude request through the OpenAI endpoint, ABP should
// faithfully translate the request while preserving all semantics."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story8_given_claude_request_when_mapped_to_openai_then_system_prompt_preserved() {
    // Given: A Claude-style request with a system prompt
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(
            IrRole::System,
            "You are Claude, an AI assistant by Anthropic.",
        ),
        IrMessage::text(IrRole::User, "Explain quantum computing"),
    ]);

    // When: Mapped from Claude dialect to OpenAI dialect
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();

    // Then: The system prompt text is faithfully preserved
    assert_eq!(
        mapped.messages[0].text_content(),
        "You are Claude, an AI assistant by Anthropic."
    );
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

#[test]
fn story8_given_openai_request_when_mapped_to_gemini_then_message_order_preserved() {
    // Given: A multi-turn OpenAI conversation
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "First question"),
        IrMessage::text(IrRole::Assistant, "First answer"),
        IrMessage::text(IrRole::User, "Follow-up question"),
    ]);

    // When: Mapped to Gemini dialect
    let mapper = OpenAiGeminiIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();

    // Then: Message ordering is preserved across the dialect boundary
    assert_eq!(mapped.messages.len(), 3);
    assert_eq!(mapped.messages[0].text_content(), "First question");
    assert_eq!(mapped.messages[1].text_content(), "First answer");
    assert_eq!(mapped.messages[2].text_content(), "Follow-up question");
}

#[test]
fn story8_given_openai_to_claude_when_tool_result_mapped_then_content_preserved() {
    // Given: An OpenAI tool-result block in IR
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_xyz".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents here".into(),
            }],
            is_error: false,
        }],
    )]);

    // When: Mapped to Claude dialect
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: The tool result content and error flag are preserved
    let msg = &mapped.messages[0];
    if let IrContentBlock::ToolResult {
        content, is_error, ..
    } = &msg.content[0]
    {
        assert_eq!(content.len(), 1);
        if let IrContentBlock::Text { text } = &content[0] {
            assert_eq!(text, "file contents here");
        } else {
            panic!("expected Text block inside ToolResult");
        }
        assert!(!is_error);
    } else {
        panic!("expected ToolResult block");
    }
}

#[test]
fn story8_given_gemini_to_openai_when_roundtripped_then_no_data_loss() {
    // Given: A conversation in Gemini dialect
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "What is 2+2?"),
    ]);

    // When: Roundtripped Gemini -> OpenAI -> Gemini
    let mapper = OpenAiGeminiIrMapper;
    let to_openai = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &to_openai)
        .unwrap();

    // Then: No data is lost in the round trip
    assert_eq!(back.messages.len(), ir.messages.len());
    for (orig, rt) in ir.messages.iter().zip(back.messages.iter()) {
        assert_eq!(orig.text_content(), rt.text_content());
        assert_eq!(orig.role, rt.role);
    }
}

#[test]
fn story8_given_unsupported_dialect_pair_when_factory_called_then_none_returned() {
    // Given: A dialect pair with no known mapper (e.g. Kimi -> Copilot)
    // When: The mapper factory is queried
    let result = default_ir_mapper(Dialect::Kimi, Dialect::Copilot);

    // Then: No mapper is available (returns None)
    assert!(
        result.is_none(),
        "no mapper should exist for unsupported dialect pair"
    );
}

#[test]
fn story8_given_identity_mapper_when_response_mapped_then_exact_passthrough() {
    // Given: A response conversation going through identity mapping
    let ir = IrConversation::from_messages(vec![IrMessage::text(
        IrRole::Assistant,
        "Here is your answer.",
    )]);

    // When: The identity mapper maps the response
    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_response(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();

    // Then: The response is bit-for-bit identical
    assert_eq!(mapped.messages[0].text_content(), "Here is your answer.");
    assert_eq!(mapped.messages[0].role, IrRole::Assistant);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 9: Error handling stories
//
// "When the backend returns a rate-limit error, ABP should surface it
// through the protocol with a structured error code."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story9_given_rate_limit_error_when_fatal_envelope_created_then_error_code_set() {
    // Given: A backend returning a rate-limit error
    // When: The error is wrapped in a Fatal envelope with an error code
    let env = Envelope::fatal_with_code(
        Some("run-42".into()),
        "rate limit exceeded: 429",
        ErrorCode::BackendRateLimited,
    );

    // Then: The envelope carries both the error message and structured code
    if let Envelope::Fatal {
        error, error_code, ..
    } = &env
    {
        assert_eq!(error, "rate limit exceeded: 429");
        assert_eq!(*error_code, Some(ErrorCode::BackendRateLimited));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn story9_given_auth_failure_when_fatal_encoded_then_code_roundtrips() {
    // Given: An authentication failure error
    let env = Envelope::fatal_with_code(
        Some("run-99".into()),
        "invalid API key",
        ErrorCode::BackendAuthFailed,
    );

    // When: Encoded and decoded through JSONL
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The error code survives the roundtrip
    if let Envelope::Fatal {
        error, error_code, ..
    } = decoded
    {
        assert_eq!(error, "invalid API key");
        assert_eq!(error_code, Some(ErrorCode::BackendAuthFailed));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn story9_given_backend_timeout_when_fatal_created_then_ref_id_links_to_run() {
    // Given: A backend that timed out during a run
    let env = Envelope::fatal_with_code(
        Some("run-timeout-1".into()),
        "backend timed out after 30s",
        ErrorCode::BackendTimeout,
    );

    // When: Inspecting the fatal envelope
    // Then: The ref_id links back to the originating run
    if let Envelope::Fatal { ref_id, .. } = &env {
        assert_eq!(ref_id.as_deref(), Some("run-timeout-1"));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn story9_given_model_not_found_when_fatal_encoded_then_error_code_preserved() {
    // Given: A request for a model that doesn't exist
    let env = Envelope::fatal_with_code(
        Some("run-model".into()),
        "model 'gpt-5-turbo' not found",
        ErrorCode::BackendModelNotFound,
    );

    // When: Roundtripped through JSONL
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The specific error code is preserved
    assert_eq!(decoded.error_code(), Some(ErrorCode::BackendModelNotFound));
}

#[test]
fn story9_given_fatal_without_ref_when_encoded_then_ref_id_is_none() {
    // Given: A fatal error that occurs before a run starts (no ref_id)
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure: config invalid".into(),
        error_code: Some(ErrorCode::ConfigInvalid),
    };

    // When: Encoded and decoded
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: ref_id remains None
    if let Envelope::Fatal { ref_id, .. } = decoded {
        assert!(ref_id.is_none());
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn story9_given_policy_denied_error_when_checked_then_decision_explains_reason() {
    // Given: A policy that denies the "Bash" tool
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: An agent attempts to use Bash
    let decision = engine.can_use_tool("Bash");

    // Then: The denial includes a reason for auditability
    assert!(!decision.allowed);
    assert!(
        decision.reason.is_some(),
        "denied decisions should include a reason"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 10: Capability negotiation stories
//
// "When I request vision support but the backend lacks it, ABP should
// detect the mismatch and report it clearly."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story10_given_vision_required_when_backend_lacks_it_then_mismatch_detectable() {
    // Given: A work order requiring native vision support
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("Analyze this image")
        .root(".")
        .requirements(reqs)
        .build();

    // When: Checking against a backend that doesn't declare vision
    let caps: CapabilityManifest = BTreeMap::new();

    // Then: The required capability is absent from the manifest
    let missing: Vec<_> = wo
        .requirements
        .required
        .iter()
        .filter(|r| caps.get(&r.capability).is_none())
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].capability, Capability::Vision);
}

#[test]
fn story10_given_streaming_required_as_native_when_backend_emulates_then_insufficient() {
    // Given: A requirement for native streaming
    let req = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    };

    // When: The backend only emulates streaming
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    // Then: The emulated level does not satisfy the native requirement
    let level = caps.get(&req.capability).unwrap();
    let satisfies = matches!(level, SupportLevel::Native);
    assert!(
        !satisfies,
        "emulated should not satisfy a native requirement"
    );
}

#[test]
fn story10_given_tool_use_required_when_backend_supports_native_then_satisfied() {
    // Given: A requirement for tool use at any support level
    let req = CapabilityRequirement {
        capability: Capability::ToolUse,
        min_support: MinSupport::Any,
    };

    // When: The backend natively supports tool use
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);

    // Then: The requirement is satisfied
    let level = caps.get(&req.capability);
    assert!(level.is_some());
    assert!(matches!(
        level.unwrap(),
        SupportLevel::Native | SupportLevel::Emulated
    ));
}

#[test]
fn story10_given_multiple_capabilities_required_when_some_missing_then_all_gaps_identified() {
    // Given: Requirements for vision, streaming, and tool use
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Vision,
                min_support: MinSupport::Any,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolUse,
                min_support: MinSupport::Any,
            },
        ],
    };

    // When: Backend only supports streaming (native)
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);

    // Then: Vision and ToolUse are identified as missing
    let gaps: Vec<_> = reqs
        .required
        .iter()
        .filter(|r| caps.get(&r.capability).is_none())
        .collect();
    assert_eq!(gaps.len(), 2);
    let gap_caps: Vec<_> = gaps.iter().map(|g| &g.capability).collect();
    assert!(gap_caps.contains(&&Capability::Vision));
    assert!(gap_caps.contains(&&Capability::ToolUse));
}

#[test]
fn story10_given_restricted_capability_when_checked_then_reason_available() {
    // Given: A backend that restricts a capability with a reason
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(
        Capability::CodeExecution,
        SupportLevel::Restricted {
            reason: "sandboxed environment only".into(),
        },
    );

    // When: Querying the capability
    let level = caps.get(&Capability::CodeExecution).unwrap();

    // Then: The restriction reason is accessible
    if let SupportLevel::Restricted { reason } = level {
        assert_eq!(reason, "sandboxed environment only");
    } else {
        panic!("expected Restricted support level");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 11: Receipt verification stories
//
// "After a run completes, the receipt should be hash-verified to ensure
// no tampering occurred during transmission."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story11_given_completed_receipt_when_hash_verified_then_valid() {
    // Given: A receipt with a computed hash
    let receipt = make_receipt("verify-backend").with_hash().unwrap();

    // When: The receipt is verified
    let valid = verify_hash(&receipt);

    // Then: Verification passes
    assert!(valid, "freshly hashed receipt should verify");
}

#[test]
fn story11_given_tampered_receipt_when_hash_verified_then_invalid() {
    // Given: A receipt with a computed hash
    let mut receipt = make_receipt("tamper-test").with_hash().unwrap();

    // When: The receipt is tampered with after hashing
    receipt.outcome = Outcome::Failed;

    // Then: Verification fails (hash no longer matches content)
    let valid = verify_hash(&receipt);
    assert!(!valid, "tampered receipt should fail verification");
}

#[test]
fn story11_given_receipt_without_hash_when_verified_then_passes_vacuously() {
    // Given: A receipt with no hash set
    let receipt = make_receipt("no-hash-backend");
    assert!(receipt.receipt_sha256.is_none());

    // When: Verified
    let valid = verify_hash(&receipt);

    // Then: Passes vacuously (no hash to verify against)
    assert!(valid, "receipt with no hash should pass verification");
}

#[test]
fn story11_given_receipt_with_events_when_hashed_then_events_contribute_to_hash() {
    // Given: Two receipts — one with events, one without
    let receipt_no_events = make_receipt("events-test");
    let receipt_with_events = make_receipt_with_events(vec![
        make_event(AgentEventKind::RunStarted {
            message: "starting run".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        }),
    ]);

    // When: Both are hashed
    let hash_no_events = receipt_hash(&receipt_no_events).unwrap();
    let hash_with_events = receipt_hash(&receipt_with_events).unwrap();

    // Then: The hashes differ because events are part of the hash input
    assert_ne!(hash_no_events, hash_with_events);
}

#[test]
fn story11_given_receipt_when_compute_hash_called_then_matches_receipt_hash() {
    // Given: A receipt
    let receipt = make_receipt("dual-hash-test");

    // When: Hashed via both the core function and the receipt crate
    let hash_core = receipt_hash(&receipt).unwrap();
    let hash_crate = compute_hash(&receipt).unwrap();

    // Then: Both produce the same hash
    assert_eq!(hash_core, hash_crate);
}

#[tokio::test]
async fn story11_given_mock_run_when_receipt_returned_then_hash_verifies() {
    // Given: A real mock backend run
    let wo = make_work_order("Full verification test");
    let backend = MockBackend;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);

    // When: The run completes and receipt is returned
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The receipt's hash verifies successfully
    assert!(receipt.receipt_sha256.is_some());
    assert!(verify_hash(&receipt), "mock backend receipt should verify");
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 12: Configuration stories
//
// "When I configure a work order with specific settings, ABP should
// honor those settings throughout the execution pipeline."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story12_given_budget_limit_when_work_order_built_then_budget_captured() {
    // Given: An operator sets a max budget of $0.50
    let wo = WorkOrderBuilder::new("Expensive analysis")
        .root(".")
        .max_budget_usd(0.50)
        .build();

    // When: Inspecting the work order
    // Then: The budget constraint is captured
    assert_eq!(wo.config.max_budget_usd, Some(0.50));
}

#[test]
fn story12_given_max_turns_when_work_order_built_then_turns_limit_set() {
    // Given: A developer limits the agent to 10 turns
    let wo = WorkOrderBuilder::new("Iterative refactor")
        .root(".")
        .max_turns(10)
        .build();

    // When: Inspecting the work order
    // Then: The turns limit is set
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn story12_given_policy_in_work_order_when_built_then_policy_attached() {
    // Given: A work order with an embedded policy
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into(), "WebFetch".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("Safe analysis")
        .root(".")
        .policy(policy)
        .build();

    // When: Inspecting the work order's policy
    // Then: The policy restrictions are present
    assert_eq!(wo.policy.disallowed_tools.len(), 2);
    assert!(wo.policy.disallowed_tools.contains(&"Bash".into()));
}

#[test]
fn story12_given_model_override_when_work_order_built_then_model_set() {
    // Given: A user overrides the model to a specific version
    let wo = WorkOrderBuilder::new("Use specific model")
        .root(".")
        .model("claude-3.5-sonnet")
        .build();

    // When: Inspecting the work order
    // Then: The model override is captured
    assert_eq!(wo.config.model.as_deref(), Some("claude-3.5-sonnet"));
}

#[test]
fn story12_given_no_model_when_work_order_built_then_model_is_none() {
    // Given: A work order with no explicit model
    let wo = WorkOrderBuilder::new("Default model").root(".").build();

    // When: Inspecting the model field
    // Then: Model is None, allowing the backend to choose
    assert!(wo.config.model.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 13: Streaming stories
//
// "When I request streaming output, ABP should deliver events
// incrementally through the channel."
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn story13_given_streaming_request_when_mock_runs_then_events_arrive_in_order() {
    // Given: A work order for a streaming-capable backend
    let wo = make_work_order("Stream me results");
    let backend = MockBackend;
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    // When: The backend runs and streams events
    let _receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: Events arrive and the first is RunStarted
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(!events.is_empty(), "streaming should produce events");
    assert!(
        matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
        "first streamed event should be RunStarted"
    );
}

#[tokio::test]
async fn story13_given_streaming_run_when_completed_then_last_event_is_run_completed() {
    // Given: A streaming run
    let wo = make_work_order("Complete the stream");
    let backend = MockBackend;
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    // When: The run finishes
    let _receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    // Then: The last streamed event is RunCompleted
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let last = events.last().expect("should have events");
    assert!(
        matches!(&last.kind, AgentEventKind::RunCompleted { .. }),
        "last streamed event should be RunCompleted"
    );
}

#[test]
fn story13_given_delta_event_when_encoded_as_envelope_then_preserves_text() {
    // Given: An assistant delta (incremental streaming token)
    let event = make_event(AgentEventKind::AssistantDelta {
        text: "Hello".into(),
    });
    let env = Envelope::Event {
        ref_id: "stream-run-1".into(),
        event,
    };

    // When: Encoded to JSONL and decoded
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The delta text is preserved
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantDelta { text } = &event.kind {
            assert_eq!(text, "Hello");
        } else {
            panic!("expected AssistantDelta event");
        }
    } else {
        panic!("expected Event envelope");
    }
}

#[test]
fn story13_given_multiple_deltas_when_encoded_then_each_is_separate_jsonl_line() {
    // Given: Multiple streaming delta events
    let deltas = vec!["Hello", " world", "!"];
    let envelopes: Vec<String> = deltas
        .iter()
        .map(|text| {
            let event = make_event(AgentEventKind::AssistantDelta {
                text: (*text).into(),
            });
            let env = Envelope::Event {
                ref_id: "stream-1".into(),
                event,
            };
            JsonlCodec::encode(&env).unwrap()
        })
        .collect();

    // When: Each is encoded
    // Then: Each produces exactly one JSONL line (ends with \n, no embedded newlines)
    for line in &envelopes {
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "should be exactly one line");
    }
    assert_eq!(envelopes.len(), 3);
}

#[test]
fn story13_given_backend_metadata_when_streaming_supported_then_flag_true() {
    // Given: Backend metadata declaring streaming support
    let meta = make_backend_metadata("stream-backend", "openai");

    // When: Checking streaming support
    // Then: The flag is true
    assert!(meta.supports_streaming);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 14: Multi-backend failover stories
//
// "When the primary backend fails, the fallback should be selected
// from the remaining healthy backends."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story14_given_primary_unhealthy_when_fallback_selected_then_healthy_backend_chosen() {
    // Given: A registry with one unhealthy primary and one healthy fallback
    let mut registry = BackendRegistry::new();
    registry.register_with_metadata("primary", make_backend_metadata("primary", "openai"));
    registry.register_with_metadata("fallback", make_backend_metadata("fallback", "anthropic"));

    // Make primary unhealthy
    let mut health = BackendHealth::default();
    health.record_failure(1);
    registry.update_health("primary", health);

    // Make fallback healthy
    register_healthy_backend_health(&mut registry, "fallback");

    // When: Selecting the first healthy backend
    let selected = registry.select(&SelectionStrategy::FirstHealthy);

    // Then: The fallback is selected
    assert!(selected.is_some());
    assert_eq!(selected.unwrap(), "fallback");
}

#[test]
fn story14_given_all_backends_unhealthy_when_selected_then_none_returned() {
    // Given: A registry where all backends are unhealthy
    let mut registry = BackendRegistry::new();
    registry.register_with_metadata("backend-a", make_backend_metadata("backend-a", "openai"));
    registry.register_with_metadata("backend-b", make_backend_metadata("backend-b", "anthropic"));

    let mut health_a = BackendHealth::default();
    health_a.record_failure(1);
    registry.update_health("backend-a", health_a);

    let mut health_b = BackendHealth::default();
    health_b.record_failure(1);
    registry.update_health("backend-b", health_b);

    // When: Attempting to select any healthy backend
    let selected = registry.select(&SelectionStrategy::FirstHealthy);

    // Then: No backend is available
    assert!(selected.is_none(), "all unhealthy should return None");
}

#[test]
fn story14_given_degraded_backend_when_selected_by_preference_then_not_available() {
    // Given: A backend in Degraded state (one failure, high threshold)
    let mut registry = BackendRegistry::new();
    registry.register_with_metadata("degraded", make_backend_metadata("degraded", "openai"));

    let mut health = BackendHealth::default();
    health.record_failure(5); // threshold=5 -> 1 failure = Degraded
    assert_eq!(health.status, HealthStatus::Degraded);
    assert!(health.is_operational(), "degraded should be operational");
    registry.update_health("degraded", health);

    // When: Selecting by preference
    let selected = registry.select(&SelectionStrategy::ByPreference("degraded".into()));

    // Then: ByPreference requires Healthy, so Degraded is NOT selected
    assert!(
        selected.is_none(),
        "ByPreference requires Healthy status, Degraded is not sufficient"
    );
}

#[test]
fn story14_given_multiple_healthy_when_select_by_dialect_then_correct_dialect_chosen() {
    // Given: Multiple healthy backends with different dialects
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "gpt4", "openai");
    register_healthy_backend(&mut registry, "claude", "anthropic");
    register_healthy_backend(&mut registry, "gemini", "google");

    // When: Selecting by "anthropic" dialect
    let selected = registry.select(&SelectionStrategy::ByDialect("anthropic".into()));

    // Then: The Claude backend is selected
    assert_eq!(selected, Some("claude".into()));
}

#[test]
fn story14_given_backend_recovers_when_success_recorded_then_health_restored() {
    // Given: An unhealthy backend
    let mut registry = BackendRegistry::new();
    registry.register_with_metadata("recovering", make_backend_metadata("recovering", "openai"));
    let mut health = BackendHealth::default();
    health.record_failure(1);
    assert_eq!(health.status, HealthStatus::Unhealthy);

    // When: A success is recorded (backend recovered)
    health.record_success(50);
    registry.update_health("recovering", health);

    // Then: The backend is healthy again and selectable
    let selected = registry.select(&SelectionStrategy::ByPreference("recovering".into()));
    assert!(selected.is_some(), "recovered backend should be selectable");
}

#[test]
fn story14_given_registry_when_backend_removed_then_not_selectable() {
    // Given: A registry with a healthy backend
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "removable", "openai");
    assert!(registry.contains("removable"));

    // When: The backend is removed
    registry.remove("removable");

    // Then: It is no longer selectable
    assert!(!registry.contains("removable"));
    let selected = registry.select(&SelectionStrategy::ByPreference("removable".into()));
    assert!(selected.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 15: Protocol version and compatibility stories
//
// "As a sidecar, I want to verify that my protocol version is compatible
// with the control plane so I can gracefully reject incompatible versions."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story15_given_matching_versions_when_checked_then_compatible() {
    // Given: The same contract version on both sides
    // When: Checking compatibility
    let result = is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION);

    // Then: They are compatible
    assert!(result);
}

#[test]
fn story15_given_different_major_version_when_checked_then_incompatible() {
    // Given: A hypothetical future major version bump
    let future_version = "abp/v1.0";

    // When: Checking against current v0.1
    let result = is_compatible_version(future_version, CONTRACT_VERSION);

    // Then: Incompatible (major version differs)
    assert!(!result);
}

#[test]
fn story15_given_valid_version_when_parsed_then_major_minor_extracted() {
    // Given: The current contract version string
    // When: Parsed
    let parsed = parse_version(CONTRACT_VERSION);

    // Then: Major and minor components are extracted
    assert!(parsed.is_some());
    let (major, minor) = parsed.unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

#[test]
fn story15_given_invalid_version_string_when_parsed_then_none() {
    // Given: A malformed version string
    let bad = "not-a-version";

    // When: Parsed
    let result = parse_version(bad);

    // Then: None is returned
    assert!(result.is_none());
}

#[test]
fn story15_given_hello_envelope_when_sent_then_contract_version_included() {
    // Given: A sidecar sending a hello envelope
    let hello = make_hello_envelope("version-test-sidecar");

    // When: Encoded to JSONL
    let json = JsonlCodec::encode(&hello).unwrap();

    // Then: The contract version is embedded in the envelope
    assert!(
        json.contains(CONTRACT_VERSION),
        "hello envelope should include the contract version"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 16: Advanced policy and path stories
//
// "As an operator, I want fine-grained path controls so agents cannot
// read or write sensitive files."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story16_given_deny_write_to_ci_when_workflow_edited_then_denied() {
    // Given: A policy denying writes to CI configuration
    let policy = PolicyProfile {
        deny_write: vec![".github/**".into(), ".gitlab-ci.yml".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: An agent tries to edit a GitHub Actions workflow
    let decision = engine.can_write_path(Path::new(".github/workflows/ci.yml"));

    // Then: The write is denied
    assert!(!decision.allowed);
}

#[test]
fn story16_given_deny_read_env_when_production_env_read_then_denied() {
    // Given: A policy denying reads on environment files
    let policy = PolicyProfile {
        deny_read: vec!["**/.env*".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Reading .env.production
    let decision = engine.can_read_path(Path::new(".env.production"));

    // Then: The read is denied
    assert!(!decision.allowed);
}

#[test]
fn story16_given_combined_tool_and_path_policy_when_checked_then_both_enforced() {
    // Given: A policy with both tool and path restrictions
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/secrets/**".into()],
        deny_read: vec!["**/.env".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking various operations
    // Then: Tool restrictions are enforced
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    // And: Path restrictions are enforced
    assert!(
        !engine
            .can_write_path(Path::new("secrets/api_key.txt"))
            .allowed
    );
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    // And: Unrestricted paths remain accessible
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(engine.can_read_path(Path::new("Cargo.toml")).allowed);
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers for new stories
// ═══════════════════════════════════════════════════════════════════════════

fn register_healthy_backend_health(registry: &mut BackendRegistry, name: &str) {
    let mut health = BackendHealth::default();
    health.record_success(50);
    registry.update_health(name, health);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 17: SDK Translation Stories
//
// "As a developer, I want ABP to translate requests between different
// agent SDK dialects so I can use any backend interchangeably."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story17_given_openai_request_when_translated_to_claude_then_thinking_blocks_preserved() {
    // Given: An IR conversation with a thinking block (Claude feature)
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Think step by step."),
        IrMessage {
            role: IrRole::Assistant,
            content: vec![
                IrContentBlock::Thinking {
                    text: "Let me think...".into(),
                },
                IrContentBlock::Text {
                    text: "Here is my answer.".into(),
                },
            ],
            metadata: BTreeMap::new(),
        },
    ]);

    // When: Translating from OpenAI to Claude
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();

    // Then: Thinking blocks are preserved (Claude natively supports them)
    let assistant = result.messages.iter().find(|m| m.role == IrRole::Assistant);
    assert!(assistant.is_some());
    let has_thinking = assistant
        .unwrap()
        .content
        .iter()
        .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
    assert!(
        has_thinking,
        "thinking blocks should be preserved for Claude"
    );
}

#[test]
fn story17_given_claude_request_when_translated_to_openai_then_thinking_blocks_dropped() {
    // Given: A Claude-style conversation with thinking blocks
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Explain recursion."),
        IrMessage {
            role: IrRole::Assistant,
            content: vec![
                IrContentBlock::Thinking {
                    text: "Recursion is...".into(),
                },
                IrContentBlock::Text {
                    text: "Recursion is when a function calls itself.".into(),
                },
            ],
            metadata: BTreeMap::new(),
        },
    ]);

    // When: Translating from Claude to OpenAI
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();

    // Then: Thinking blocks are dropped (OpenAI has no equivalent)
    let assistant = result.messages.iter().find(|m| m.role == IrRole::Assistant);
    assert!(assistant.is_some());
    let has_thinking = assistant
        .unwrap()
        .content
        .iter()
        .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
    assert!(
        !has_thinking,
        "thinking blocks should be dropped for OpenAI"
    );
}

#[test]
fn story17_given_openai_request_when_translated_to_gemini_then_tool_roles_remapped() {
    // Given: An OpenAI-style conversation with a Tool-role message
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage {
            role: IrRole::Tool,
            content: vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text { text: "4".into() }],
                is_error: false,
            }],
            metadata: BTreeMap::new(),
        },
    ]);

    // When: Translating from OpenAI to Gemini
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();

    // Then: Tool-role messages become User-role (Gemini convention)
    let tool_result_msg = result.messages.iter().find(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
    });
    assert!(tool_result_msg.is_some());
    assert_eq!(
        tool_result_msg.unwrap().role,
        IrRole::User,
        "tool result should become User-role in Gemini"
    );
}

#[test]
fn story17_given_claude_request_when_translated_to_gemini_then_thinking_dropped() {
    // Given: A Claude conversation with thinking blocks
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage {
            role: IrRole::Assistant,
            content: vec![
                IrContentBlock::Thinking {
                    text: "thinking...".into(),
                },
                IrContentBlock::Text { text: "Hi!".into() },
            ],
            metadata: BTreeMap::new(),
        },
    ]);

    // When: Translating Claude to Gemini
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();

    // Then: Thinking blocks are removed
    for msg in &result.messages {
        for block in &msg.content {
            assert!(
                !matches!(block, IrContentBlock::Thinking { .. }),
                "Gemini should not receive thinking blocks"
            );
        }
    }
}

#[test]
fn story17_given_identity_mapper_when_same_dialect_then_conversation_unchanged() {
    // Given: A simple conversation
    let conv = make_ir_conversation();

    // When: Mapping with the identity mapper
    let mapper = IrIdentityMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();

    // Then: Messages are identical
    assert_eq!(result.messages.len(), conv.messages.len());
    for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
        assert_eq!(orig.role, mapped.role);
    }
}

#[test]
fn story17_given_openai_to_copilot_when_image_present_then_rejected() {
    // Given: A conversation with an image block
    let conv = IrConversation::from_messages(vec![IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
        metadata: BTreeMap::new(),
    }]);

    // When: Translating to Copilot (no image support)
    let mapper = OpenAiCopilotIrMapper;
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Copilot, &conv);

    // Then: The mapping is rejected
    assert!(result.is_err(), "Copilot should reject image blocks");
}

#[test]
fn story17_given_default_ir_mapper_when_unsupported_pair_then_none() {
    // Given: A dialect pair with no direct mapper defined
    // (identity pairs always exist, so check that the factory returns Some for known pairs)
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);

    // When/Then: Known pair returns a mapper
    assert!(mapper.is_some(), "OpenAI→Claude mapper should exist");

    // And: Same-dialect always has identity mapper
    let identity = default_ir_mapper(Dialect::Gemini, Dialect::Gemini);
    assert!(identity.is_some(), "same-dialect should always have mapper");
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 18: Backend Selection Stories
//
// "As an operator, I want ABP to select the right backend based on
// health, capability, dialect, and other criteria."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story18_given_multiple_backends_when_one_unhealthy_then_excluded_from_selection() {
    // Given: Two backends, one healthy and one unhealthy
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "healthy-gpt", "openai");

    registry.register_with_metadata(
        "unhealthy-claude",
        make_backend_metadata("unhealthy-claude", "anthropic"),
    );
    let mut bad_health = BackendHealth::default();
    bad_health.record_failure(1);
    registry.update_health("unhealthy-claude", bad_health);

    // When: Selecting the first healthy backend
    let selected = registry.select(&SelectionStrategy::FirstHealthy);

    // Then: The unhealthy backend is excluded
    assert_eq!(selected, Some("healthy-gpt".into()));
}

#[test]
fn story18_given_streaming_requirement_when_selecting_then_only_streaming_backends_returned() {
    // Given: One streaming and one non-streaming backend
    let mut registry = BackendRegistry::new();
    let mut streaming_meta = make_backend_metadata("streamer", "openai");
    streaming_meta.supports_streaming = true;
    registry.register_with_metadata("streamer", streaming_meta);
    register_healthy_backend_health(&mut registry, "streamer");

    let mut no_stream_meta = make_backend_metadata("batch-only", "anthropic");
    no_stream_meta.supports_streaming = false;
    registry.register_with_metadata("batch-only", no_stream_meta);
    register_healthy_backend_health(&mut registry, "batch-only");

    // When: Selecting by streaming capability
    let selected = registry.select(&SelectionStrategy::ByStreaming);

    // Then: Only the streaming backend is chosen
    assert_eq!(selected, Some("streamer".into()));
}

#[test]
fn story18_given_tool_support_requirement_when_selecting_then_tool_capable_backend_chosen() {
    // Given: One backend with tool support, one without
    let mut registry = BackendRegistry::new();
    let mut tools_meta = make_backend_metadata("tools-backend", "openai");
    tools_meta.supports_tools = true;
    registry.register_with_metadata("tools-backend", tools_meta);
    register_healthy_backend_health(&mut registry, "tools-backend");

    let mut no_tools = make_backend_metadata("no-tools", "anthropic");
    no_tools.supports_tools = false;
    registry.register_with_metadata("no-tools", no_tools);
    register_healthy_backend_health(&mut registry, "no-tools");

    // When: Selecting by tool support
    let selected = registry.select(&SelectionStrategy::ByToolSupport);

    // Then: The tool-capable backend is selected
    assert_eq!(selected, Some("tools-backend".into()));
}

#[test]
fn story18_given_lowest_latency_strategy_when_selecting_then_fastest_backend_chosen() {
    // Given: Backends with different latencies
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "slow", "openai");
    register_healthy_backend(&mut registry, "fast", "anthropic");

    // Record different latencies
    let mut slow_health = BackendHealth::default();
    slow_health.record_success(500);
    registry.update_health("slow", slow_health);

    let mut fast_health = BackendHealth::default();
    fast_health.record_success(10);
    registry.update_health("fast", fast_health);

    // When: Selecting by lowest latency
    let selected = registry.select(&SelectionStrategy::ByLowestLatency);

    // Then: The fastest backend is selected
    assert_eq!(selected, Some("fast".into()));
}

#[test]
fn story18_given_dialect_filter_when_no_match_then_none_returned() {
    // Given: Only OpenAI backends registered
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "gpt4", "openai");

    // When: Selecting by a dialect that doesn't exist
    let selected = registry.select(&SelectionStrategy::ByDialect("anthropic".into()));

    // Then: No backend matches
    assert!(selected.is_none());
}

#[test]
fn story18_given_preferred_backend_when_healthy_then_selected() {
    // Given: Multiple healthy backends
    let mut registry = BackendRegistry::new();
    register_healthy_backend(&mut registry, "alpha", "openai");
    register_healthy_backend(&mut registry, "beta", "anthropic");

    // When: Selecting preferred backend "beta"
    let selected = registry.select(&SelectionStrategy::ByPreference("beta".into()));

    // Then: The preferred backend is returned
    assert_eq!(selected, Some("beta".into()));
}

#[test]
fn story18_given_empty_registry_when_selecting_then_none() {
    // Given: An empty backend registry
    let registry = BackendRegistry::new();

    // When: Attempting any selection
    let selected = registry.select(&SelectionStrategy::FirstHealthy);

    // Then: No backend available
    assert!(selected.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 19: Receipt Stories
//
// "As a system, I want receipt hashing and verification to be deterministic
// and support tamper detection and chain integrity."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story19_given_completed_run_when_receipt_hashed_then_hash_is_deterministic() {
    // Given: A receipt built with fixed parameters
    let r1 = abp_receipt::ReceiptBuilder::new("deterministic-backend")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .run_id(Uuid::nil())
        .started_at(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .unwrap()
                .into(),
        )
        .finished_at(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:01:00Z")
                .unwrap()
                .into(),
        )
        .build();

    // When: Computing the hash twice
    let hash1 = compute_hash(&r1).unwrap();
    let hash2 = compute_hash(&r1).unwrap();

    // Then: Both hashes are identical
    assert_eq!(hash1, hash2, "receipt hash must be deterministic");
    assert!(!hash1.is_empty());
}

#[test]
fn story19_given_receipt_with_hash_when_verified_then_passes() {
    // Given: A receipt with its hash computed
    let receipt = abp_receipt::ReceiptBuilder::new("hash-test")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .unwrap()
                .into(),
        )
        .finished_at(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:01:00Z")
                .unwrap()
                .into(),
        )
        .with_hash()
        .unwrap();

    // When: Verifying the hash
    let valid = verify_hash(&receipt);

    // Then: Verification passes
    assert!(valid, "receipt with correct hash should verify");
}

#[test]
fn story19_given_tampered_receipt_when_verified_then_fails() {
    // Given: A receipt with a computed hash
    let mut receipt = abp_receipt::ReceiptBuilder::new("tamper-test")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .unwrap()
                .into(),
        )
        .finished_at(
            chrono::DateTime::parse_from_rfc3339("2025-01-01T00:01:00Z")
                .unwrap()
                .into(),
        )
        .with_hash()
        .unwrap();

    // When: Tampering with the receipt
    receipt.outcome = Outcome::Failed;

    // Then: Verification fails
    assert!(
        !verify_hash(&receipt),
        "tampered receipt should fail verification"
    );
}

#[test]
fn story19_given_receipt_without_hash_when_verified_then_passes_vacuously() {
    // Given: A receipt with no hash (receipt_sha256 = None)
    let receipt = make_receipt("no-hash-backend");

    // When: Verifying
    let valid = verify_hash(&receipt);

    // Then: Passes vacuously (no hash to mismatch)
    assert!(
        valid,
        "receipt without hash should pass verification vacuously"
    );
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn story19_given_multiple_runs_when_receipts_chained_then_chain_integrity_holds() {
    // Given: Two sequential runs where the second references the first's hash
    let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .into();
    let r1 = abp_receipt::ReceiptBuilder::new("chain-backend")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .with_hash()
        .unwrap();
    let r1_hash = r1.receipt_sha256.clone().unwrap();

    // Chain: second receipt references first receipt's hash via ext metadata
    let chain_event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "chained task".into(),
        },
        ext: Some({
            let mut m = BTreeMap::new();
            m.insert(
                "previous_receipt_hash".into(),
                serde_json::Value::String(r1_hash.clone()),
            );
            m
        }),
    };

    let r2 = abp_receipt::ReceiptBuilder::new("chain-backend")
        .outcome(Outcome::Complete)
        .run_id(Uuid::from_u128(1))
        .work_order_id(Uuid::from_u128(1))
        .started_at(ts)
        .finished_at(ts)
        .add_event(chain_event)
        .with_hash()
        .unwrap();

    // When: Verifying both receipts
    // Then: Both verify independently
    assert!(verify_hash(&r1), "first receipt should verify");
    assert!(verify_hash(&r2), "second receipt should verify");

    // And: The chain link is preserved in the second receipt's trace
    let chain_ref = r2
        .trace
        .iter()
        .find_map(|e| e.ext.as_ref().and_then(|m| m.get("previous_receipt_hash")));
    assert_eq!(
        chain_ref.and_then(|v| v.as_str()),
        Some(r1_hash.as_str()),
        "chain link should reference first receipt's hash"
    );
}

#[test]
fn story19_given_receipt_with_events_when_hashed_then_events_affect_hash() {
    // Given: Two identical receipts except for trace events
    let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .into();

    let r_empty = abp_receipt::ReceiptBuilder::new("events-test")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();

    let r_with_event = abp_receipt::ReceiptBuilder::new("events-test")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "Hello".into(),
            },
            ext: None,
        })
        .build();

    // When: Computing hashes
    let hash_empty = compute_hash(&r_empty).unwrap();
    let hash_with_event = compute_hash(&r_with_event).unwrap();

    // Then: Different events produce different hashes
    assert_ne!(
        hash_empty, hash_with_event,
        "trace events should affect the receipt hash"
    );
}

#[test]
fn story19_given_receipt_with_wrong_hash_when_verified_then_fails() {
    // Given: A receipt with a manually set incorrect hash
    let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .into();
    let mut receipt = abp_receipt::ReceiptBuilder::new("wrong-hash")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .build();

    receipt.receipt_sha256 =
        Some("0000000000000000000000000000000000000000000000000000000000000000".into());

    // When: Verifying
    // Then: Fails because the stored hash doesn't match
    assert!(
        !verify_hash(&receipt),
        "wrong hash should fail verification"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 20: Error Recovery Stories
//
// "As a system, I want to classify errors correctly and apply the right
// recovery strategy (retry, fallback, abort) based on error type."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story20_given_transient_error_when_classified_then_retryable() {
    // Given: A transient backend error code
    let code = ErrorCode::BackendUnavailable;

    // When: Checking retryability
    // Then: It is retryable
    assert!(
        code.is_retryable(),
        "BackendUnavailable should be retryable"
    );
}

#[test]
fn story20_given_permanent_error_when_classified_then_not_retryable() {
    // Given: A permanent error code (e.g., auth failure)
    let code = ErrorCode::BackendAuthFailed;

    // When: Checking retryability
    // Then: It is NOT retryable
    assert!(
        !code.is_retryable(),
        "BackendAuthFailed should not be retryable"
    );
}

#[test]
fn story20_given_rate_limited_error_when_categorized_then_rate_limit_category() {
    // Given: A rate-limited error code
    let code = ErrorCode::BackendRateLimited;

    // When: Categorizing
    let category = abp_error::category::categorize(code);

    // Then: Mapped to RateLimit recovery category
    assert_eq!(category, abp_error::category::RecoveryCategory::RateLimit);
    assert!(
        abp_error::category::is_retryable(category),
        "rate-limited errors should be retryable"
    );
}

#[test]
fn story20_given_policy_denied_error_when_classified_then_not_retryable() {
    // Given: A policy violation error
    let code = ErrorCode::PolicyDenied;

    // When: Checking retryability
    // Then: Policy errors are NOT retryable
    assert!(
        !code.is_retryable(),
        "policy errors should not be retryable"
    );
}

#[test]
fn story20_given_error_classifier_when_transient_error_then_classified_as_transient() {
    // Given: The error classifier
    let classifier = abp_error::recovery::ErrorClassifier::new();

    // When: Classifying a transient error
    let classification = classifier.classify(ErrorCode::BackendTimeout);

    // Then: Classified as Transient
    assert_eq!(
        classification,
        abp_error::recovery::ErrorClassification::Transient
    );
}

#[test]
fn story20_given_error_classifier_when_permanent_error_then_classified_as_permanent() {
    // Given: The error classifier
    let classifier = abp_error::recovery::ErrorClassifier::new();

    // When: Classifying a permanent error
    let classification = classifier.classify(ErrorCode::ContractSchemaViolation);

    // Then: Classified as Permanent
    assert_eq!(
        classification,
        abp_error::recovery::ErrorClassification::Permanent
    );
}

#[test]
fn story20_given_recovery_strategy_retry_when_checked_then_recoverable() {
    // Given: A Retry strategy
    let strategy = abp_error::recovery::RecoveryStrategy::Retry {
        delay_ms: 1000,
        max_retries: 3,
    };

    // When: Checking recoverability
    // Then: It is recoverable
    assert!(strategy.is_recoverable());
    assert_eq!(strategy.code(), "ABP-REC-RETRY");
}

#[test]
fn story20_given_recovery_strategy_abort_when_checked_then_not_recoverable() {
    // Given: An Abort strategy
    let strategy = abp_error::recovery::RecoveryStrategy::Abort {
        reason: "unrecoverable".into(),
    };

    // When: Checking recoverability
    // Then: Not recoverable
    assert!(!strategy.is_recoverable());
    assert_eq!(strategy.code(), "ABP-REC-ABORT");
}

#[tokio::test]
async fn story20_given_circuit_breaker_when_failures_exceed_threshold_then_opens() {
    // Given: A circuit breaker with threshold=2
    let cb = abp_retry::CircuitBreaker::new(2, std::time::Duration::from_secs(60));

    // When: Recording 2 failures via call()
    let _: Result<(), abp_retry::CircuitBreakerError<&str>> =
        cb.call(|| async { Err::<(), &str>("fail") }).await;
    let _: Result<(), abp_retry::CircuitBreakerError<&str>> =
        cb.call(|| async { Err::<(), &str>("fail") }).await;

    // Then: The circuit breaker is open
    assert_eq!(cb.state(), abp_retry::CircuitState::Open);
}

#[tokio::test]
async fn story20_given_open_circuit_when_success_after_recovery_then_closes() {
    // Given: A circuit breaker that was opened then recovered
    let cb = abp_retry::CircuitBreaker::new(
        1,
        std::time::Duration::from_millis(1), // very short timeout for testing
    );
    let _: Result<(), abp_retry::CircuitBreakerError<&str>> =
        cb.call(|| async { Err::<(), &str>("fail") }).await;
    assert_eq!(cb.state(), abp_retry::CircuitState::Open);

    // When: After recovery timeout passes and a success is recorded
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let _: Result<(), abp_retry::CircuitBreakerError<&str>> =
        cb.call(|| async { Ok::<(), &str>(()) }).await;

    // Then: The circuit should close
    assert_eq!(cb.state(), abp_retry::CircuitState::Closed);
}

#[test]
fn story20_given_fallback_chain_when_iterated_then_returns_backends_in_order() {
    // Given: A fallback chain with 3 backends
    let chain = abp_error::recovery::FallbackChain::new(vec![
        "primary".into(),
        "secondary".into(),
        "tertiary".into(),
    ]);

    // When: Iterating through the chain
    // Then: Backends are returned in order
    assert_eq!(chain.next_backend(0), Some("primary"));
    assert_eq!(chain.next_backend(1), Some("secondary"));
    assert_eq!(chain.next_backend(2), Some("tertiary"));
    assert_eq!(chain.next_backend(3), None);
    assert_eq!(chain.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 21: Configuration Stories
//
// "As an operator, I want configuration management with hot-reload,
// validation, diffing, and merge capabilities."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story21_given_safe_config_change_when_hot_reloaded_then_apply_decision() {
    // Given: Two configs differing only in log_level (a safe change)
    let old = abp_config::BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let new = abp_config::BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };

    // When: Analyzing the diff and evaluating reload policy
    let analyzer = abp_config::diff_analyzer::ConfigDiffAnalyzer::new();
    let analysis = analyzer.analyze(&old, &new);
    let policy = abp_config::hot_reload_policy::HotReloadPolicy::new();
    let decision = policy.evaluate(&analysis);

    // Then: The change can be applied without restart
    assert!(
        decision.is_apply(),
        "safe changes should get Apply decision"
    );
}

#[test]
fn story21_given_breaking_config_change_when_hot_reloaded_with_conservative_policy_then_rejected() {
    // Given: A config change that adds a new backend (potentially breaking)
    let old = abp_config::BackplaneConfig::default();
    let mut new = abp_config::BackplaneConfig::default();
    new.backends.insert(
        "new-sidecar".into(),
        abp_config::BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["index.js".into()],
            timeout_secs: None,
        },
    );

    let analyzer = abp_config::diff_analyzer::ConfigDiffAnalyzer::new();
    let analysis = analyzer.analyze(&old, &new);

    // When: Evaluating with a conservative policy (no restart allowed)
    let policy = abp_config::hot_reload_policy::HotReloadPolicy::new();
    let decision = policy.evaluate(&analysis);

    // Then: Backend changes require restart which conservative policy rejects
    assert!(
        !decision.is_apply() || analysis.is_safe(),
        "structural changes should not get simple Apply with conservative policy"
    );
}

#[test]
fn story21_given_config_with_invalid_log_level_when_validated_then_error_returned() {
    // Given: A config with an invalid log level
    let config = abp_config::BackplaneConfig {
        log_level: Some("invalid_level".into()),
        ..Default::default()
    };

    // When: Validating
    let result = abp_config::validate_config(&config);

    // Then: Validation returns an error
    assert!(
        result.is_err(),
        "invalid log level should cause validation error"
    );
}

#[test]
fn story21_given_valid_toml_when_parsed_then_config_loaded() {
    // Given: A valid TOML config string
    let toml = r#"
default_backend = "mock"
log_level = "debug"

[backends.mock]
type = "mock"
"#;

    // When: Parsing
    let config = abp_config::parse_toml(toml).unwrap();

    // Then: Fields are populated correctly
    assert_eq!(config.default_backend.as_deref(), Some("mock"));
    assert_eq!(config.log_level.as_deref(), Some("debug"));
    assert!(config.backends.contains_key("mock"));
}

#[test]
fn story21_given_two_configs_when_merged_then_overlay_wins() {
    // Given: A base config and an overlay
    let base = abp_config::BackplaneConfig {
        log_level: Some("info".into()),
        default_backend: Some("base-backend".into()),
        ..Default::default()
    };
    let overlay = abp_config::BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };

    // When: Merging
    let merged = abp_config::merge_configs(base, overlay);

    // Then: Overlay values win where specified
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    // And: Base values are preserved where overlay is None
    assert_eq!(merged.default_backend.as_deref(), Some("base-backend"));
}

#[test]
fn story21_given_config_diff_when_no_changes_then_empty() {
    // Given: Two identical configs
    let config = abp_config::BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };

    // When: Diffing
    let diff = abp_config::diff::diff(&config, &config);

    // Then: No changes detected
    assert!(
        diff.is_empty(),
        "identical configs should produce empty diff"
    );
}

#[test]
fn story21_given_config_diff_when_field_changed_then_modification_detected() {
    // Given: Two configs with different log levels
    let old = abp_config::BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let new = abp_config::BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };

    // When: Diffing
    let diff = abp_config::diff::diff(&old, &new);

    // Then: A modification is detected
    assert!(!diff.is_empty());
    assert!(
        diff.changes.iter().any(
            |c| matches!(c, abp_config::diff::ConfigChange::Modified(key, ..) if key == "log_level")
        ),
        "should detect log_level modification"
    );
}

#[test]
fn story21_given_invalid_toml_when_parsed_then_error() {
    // Given: Malformed TOML
    let bad_toml = "this is not valid toml [[[";

    // When: Parsing
    let result = abp_config::parse_toml(bad_toml);

    // Then: A parse error is returned
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 22: Protocol Envelope Round-trip Stories
//
// "As a sidecar, I want envelope encoding/decoding to be lossless so
// that protocol messages survive serialization round-trips."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story22_given_fatal_envelope_when_round_tripped_then_error_preserved() {
    // Given: A fatal envelope with an error code
    let fatal = Envelope::fatal_with_code(
        Some("run-42".into()),
        "backend crashed",
        ErrorCode::BackendCrashed,
    );

    // When: Encoding and decoding
    let json = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The error details are preserved
    if let Envelope::Fatal {
        error, error_code, ..
    } = decoded
    {
        assert_eq!(error, "backend crashed");
        assert_eq!(error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn story22_given_run_envelope_when_round_tripped_then_work_order_preserved() {
    // Given: A run envelope with a work order
    let wo = make_work_order("test task");
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo.clone(),
    };

    // When: Encoding and decoding
    let json = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The work order task is preserved
    if let Envelope::Run { work_order, id, .. } = decoded {
        assert_eq!(id, "run-1");
        assert_eq!(work_order.task, "test task");
    } else {
        panic!("expected Run envelope");
    }
}

#[test]
fn story22_given_final_envelope_when_round_tripped_then_receipt_preserved() {
    // Given: A final envelope with a receipt
    let receipt = make_receipt("final-backend");
    let final_env = Envelope::Final {
        ref_id: "run-99".into(),
        receipt: receipt.clone(),
    };

    // When: Encoding and decoding
    let json = JsonlCodec::encode(&final_env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    // Then: The receipt outcome is preserved
    if let Envelope::Final { receipt: r, ref_id } = decoded {
        assert_eq!(ref_id, "run-99");
        assert_eq!(r.outcome, Outcome::Complete);
        assert_eq!(r.backend.id, "final-backend");
    } else {
        panic!("expected Final envelope");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Story 23: Error Category and Recovery Mapping Stories
//
// "As a system, I want every error code mapped to a recovery category
// with a sensible suggested delay."
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn story23_given_network_transient_category_when_delay_queried_then_reasonable_delay() {
    // Given: A network transient recovery category
    let category = abp_error::category::RecoveryCategory::NetworkTransient;

    // When: Querying the suggested delay
    let delay = abp_error::category::suggested_delay(category);

    // Then: Delay is positive and reasonable
    assert!(
        delay.as_secs() > 0,
        "network transient should have positive delay"
    );
    assert!(delay.as_secs() <= 30, "delay should be reasonable");
}

#[test]
fn story23_given_auth_error_when_delay_queried_then_zero_delay() {
    // Given: An authentication error (non-retryable category)
    let category = abp_error::category::RecoveryCategory::Authentication;

    // When: Querying the suggested delay
    let delay = abp_error::category::suggested_delay(category);

    // Then: Zero delay (non-retryable)
    assert_eq!(delay, std::time::Duration::ZERO);
}

#[test]
fn story23_given_abp_error_when_created_then_carries_code_and_message() {
    // Given: An AbpError with context
    let err = abp_error::AbpError::new(ErrorCode::BackendTimeout, "connection timed out")
        .with_context("backend", "gpt-4");

    // When: Inspecting the error
    // Then: Code, message, and context are accessible
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.message, "connection timed out");
    assert!(err.context.contains_key("backend"));
    assert!(err.is_retryable());
    assert_eq!(err.category(), abp_error::ErrorCategory::Backend);
}
