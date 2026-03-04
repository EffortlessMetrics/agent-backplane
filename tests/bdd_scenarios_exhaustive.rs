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
use abp_mapper::{
    IrIdentityMapper, IrMapper, OpenAiClaudeIrMapper, OpenAiGeminiIrMapper, default_ir_mapper,
};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version};
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
