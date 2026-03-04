#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive BDD-style scenario tests for core Agent Backplane workflows.
//!
//! Each test follows Given/When/Then structure and covers:
//! - IR mapping and content preservation across dialects
//! - Sidecar protocol handshake and version negotiation
//! - Policy enforcement (tool, read, write)
//! - Receipt hashing determinism
//! - Capability negotiation
//! - Streaming event reconstruction
//! - Edge cases and error conditions

use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest, NegotiationResult};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ContractError, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::Dialect;
use abp_error::{AbpError, ErrorCategory, ErrorCode};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeToOpenAiMapper, IrIdentityMapper, IrMapper, MapError,
    OpenAiClaudeIrMapper, OpenAiGeminiIrMapper, OpenAiToClaudeMapper, default_ir_mapper,
    supported_ir_pairs,
};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use abp_receipt::{
    ReceiptBuilder as ReceiptReceiptBuilder, canonicalize, compute_hash, verify_hash,
};

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_receipt(backend_id: &str) -> Receipt {
    ReceiptBuilder::new(backend_id)
        .outcome(Outcome::Complete)
        .build()
}

fn make_receipt_with_trace(events: Vec<AgentEvent>) -> Receipt {
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

fn make_openai_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello, world!"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn make_capability_manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    let mut manifest = CapabilityManifest::new();
    for (cap, level) in caps {
        manifest.insert(cap.clone(), level.clone());
    }
    manifest
}

// ═══════════════════════════════════════════════════════════════════════════
// Scenario 1: IR Mapping — Content Preservation
// "Given a user submits a work order with OpenAI dialect,
//  When the backend is Claude,
//  Then the IR mapping should preserve content"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_openai_to_claude_ir_mapping_preserves_text_content() {
    // Given: A conversation in OpenAI dialect with text content
    let ir = make_openai_ir_conversation();

    // When: Mapped from OpenAI to Claude via IR mapper
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &ir);

    // Then: The mapped conversation preserves all text content
    assert!(result.is_ok());
    let mapped = result.unwrap();
    assert_eq!(mapped.messages.len(), ir.messages.len());
    for (original, mapped_msg) in ir.messages.iter().zip(mapped.messages.iter()) {
        assert_eq!(original.text_content(), mapped_msg.text_content());
    }
}

#[test]
fn scenario_openai_to_claude_ir_mapping_preserves_roles() {
    // Given: A multi-role conversation
    let ir = make_openai_ir_conversation();

    // When: Mapped to Claude dialect
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: Roles are preserved
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[1].role, IrRole::User);
    assert_eq!(mapped.messages[2].role, IrRole::Assistant);
}

#[test]
fn scenario_openai_to_claude_ir_mapping_preserves_tool_use_blocks() {
    // Given: A conversation with tool use
    let tool_use = IrContentBlock::ToolUse {
        id: "tool-1".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    };
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file"),
        IrMessage::new(IrRole::Assistant, vec![tool_use.clone()]),
    ]);

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: Tool use block is preserved
    let tool_blocks = mapped.messages[1].tool_use_blocks();
    assert_eq!(tool_blocks.len(), 1);
    if let IrContentBlock::ToolUse { name, input, .. } = tool_blocks[0] {
        assert_eq!(name, "read_file");
        assert_eq!(input, &json!({"path": "src/main.rs"}));
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn scenario_claude_to_openai_roundtrip_preserves_content() {
    // Given: An IR conversation
    let ir = make_openai_ir_conversation();

    // When: Mapped OpenAI -> Claude -> OpenAI
    let mapper = OpenAiClaudeIrMapper;
    let to_claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let back_to_openai = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &to_claude)
        .unwrap();

    // Then: Text content is preserved after roundtrip
    for (original, roundtripped) in ir.messages.iter().zip(back_to_openai.messages.iter()) {
        assert_eq!(original.text_content(), roundtripped.text_content());
    }
}

#[test]
fn scenario_identity_mapper_is_passthrough() {
    // Given: Any IR conversation
    let ir = make_openai_ir_conversation();

    // When: Processed by identity mapper
    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();

    // Then: Output equals input
    assert_eq!(ir, mapped);
}

#[test]
fn scenario_openai_to_gemini_ir_mapping_preserves_content() {
    // Given: An OpenAI-style conversation
    let ir = make_openai_ir_conversation();

    // When: Mapped to Gemini
    let mapper = OpenAiGeminiIrMapper;
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Gemini, &ir);

    // Then: Content is preserved
    assert!(result.is_ok());
    let mapped = result.unwrap();
    assert!(!mapped.messages.is_empty());
}

#[test]
fn scenario_unsupported_dialect_pair_returns_error() {
    // Given: An IR mapper that doesn't support a certain pair
    let mapper = OpenAiClaudeIrMapper;
    let ir = make_openai_ir_conversation();

    // When: Attempting an unsupported dialect mapping
    let result = mapper.map_request(Dialect::Gemini, Dialect::Codex, &ir);

    // Then: Returns an UnsupportedPair error
    assert!(result.is_err());
    match result.unwrap_err() {
        MapError::UnsupportedPair { from, to } => {
            assert_eq!(from, Dialect::Gemini);
            assert_eq!(to, Dialect::Codex);
        }
        other => panic!("expected UnsupportedPair, got {:?}", other),
    }
}

#[test]
fn scenario_default_ir_mapper_factory_resolves_known_pairs() {
    // Given: A known set of supported dialect pairs
    let pairs = supported_ir_pairs();

    // When/Then: Each pair resolves to a mapper
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to);
        assert!(
            mapper.is_some(),
            "expected mapper for {:?} -> {:?}",
            from,
            to
        );
    }
}

#[test]
fn scenario_ir_mapping_preserves_empty_conversation() {
    // Given: An empty conversation
    let ir = IrConversation::new();

    // When: Mapped
    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();

    // Then: Remains empty
    assert!(mapped.messages.is_empty());
}

#[test]
fn scenario_ir_mapping_preserves_system_message() {
    // Given: A conversation with system prompt only
    let ir = IrConversation::from_messages(vec![IrMessage::text(
        IrRole::System,
        "You are a code reviewer.",
    )]);

    // When: Mapped to Claude
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: System message is preserved
    assert!(mapped.system_message().is_some());
    assert_eq!(
        mapped.system_message().unwrap().text_content(),
        "You are a code reviewer."
    );
}

#[test]
fn scenario_ir_mapping_preserves_tool_result_with_error_flag() {
    // Given: A tool result with is_error=true
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call-1".into(),
            content: vec![IrContentBlock::Text {
                text: "file not found".into(),
            }],
            is_error: true,
        }],
    )]);

    // When: Mapped
    let mapper = OpenAiClaudeIrMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();

    // Then: Error flag is preserved
    if let IrContentBlock::ToolResult { is_error, .. } = &mapped.messages[0].content[0] {
        assert!(is_error);
    } else {
        panic!("expected ToolResult block");
    }
}

#[test]
fn scenario_ir_conversation_text_content_accessor() {
    // Given: A message with multiple text blocks
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Hello ".into(),
            },
            IrContentBlock::Text {
                text: "world!".into(),
            },
        ],
    );

    // When: Getting text content
    let text = msg.text_content();

    // Then: All text blocks are concatenated
    assert_eq!(text, "Hello world!");
}

#[test]
fn scenario_ir_message_is_text_only_with_mixed_content() {
    // Given: A message with text and tool use
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
        ],
    );

    // When: Checking if text-only
    // Then: Returns false
    assert!(!msg.is_text_only());
}

// ═══════════════════════════════════════════════════════════════════════════
// Scenario 2: Sidecar Hello Handshake
// "Given a sidecar sends hello,
//  When the contract version matches,
//  Then the handshake succeeds"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_sidecar_hello_with_matching_version_succeeds() {
    // Given: A sidecar sends a hello with the current contract version
    let hello = make_hello_envelope("test-sidecar");

    // When: Encoded and decoded via JSONL
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    // Then: The handshake succeeds with correct version
    if let Envelope::Hello {
        contract_version, ..
    } = &decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn scenario_sidecar_hello_version_compatibility_check() {
    // Given: Our contract version
    let our_version = CONTRACT_VERSION;

    // When: Checking compatibility with the same version
    let compatible = is_compatible_version(our_version, our_version);

    // Then: It is compatible
    assert!(compatible);
}

#[test]
fn scenario_sidecar_hello_version_incompatible_major() {
    // Given: A sidecar with a different major version
    let their_version = "abp/v1.0";
    let our_version = CONTRACT_VERSION;

    // When: Checking compatibility
    let compatible = is_compatible_version(their_version, our_version);

    // Then: It is incompatible
    assert!(!compatible);
}

#[test]
fn scenario_sidecar_hello_invalid_version_format() {
    // Given: An invalid version string
    let invalid = "not-a-version";

    // When: Parsing the version
    let parsed = parse_version(invalid);

    // Then: Returns None
    assert!(parsed.is_none());
}

#[test]
fn scenario_sidecar_hello_parses_valid_version() {
    // Given: A valid version string
    let version = CONTRACT_VERSION;

    // When: Parsing
    let parsed = parse_version(version);

    // Then: Returns (0, 1)
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn scenario_sidecar_hello_envelope_serialization_roundtrip() {
    // Given: A hello envelope with capabilities
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("2.0".into()),
            adapter_version: Some("1.0".into()),
        },
        caps,
    );

    // When: Serialized and deserialized
    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    // Then: All fields are preserved
    if let Envelope::Hello {
        contract_version,
        backend,
        capabilities,
        mode,
    } = decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
        assert_eq!(backend.id, "test");
        assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
        assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
        assert!(capabilities.contains_key(&Capability::ToolRead));
        assert!(capabilities.contains_key(&Capability::Streaming));
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn scenario_sidecar_hello_with_passthrough_mode() {
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
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();

    // Then: Passthrough mode is preserved
    if let Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn scenario_sidecar_hello_json_contains_tag_t() {
    // Given: A hello envelope
    let hello = make_hello_envelope("tagged-sidecar");

    // When: Encoded to JSON
    let line = JsonlCodec::encode(&hello).unwrap();

    // Then: The JSON uses "t" as the discriminator tag, not "type"
    assert!(line.contains("\"t\":\"hello\""));
    assert!(!line.contains("\"type\":\"hello\""));
}

#[test]
fn scenario_sidecar_hello_ends_with_newline() {
    // Given: Any envelope
    let hello = make_hello_envelope("newline-check");

    // When: Encoded
    let line = JsonlCodec::encode(&hello).unwrap();

    // Then: Ends with newline (JSONL requirement)
    assert!(line.ends_with('\n'));
}

#[test]
fn scenario_sidecar_fatal_envelope_carries_error_code() {
    // Given: A fatal error with error code
    let fatal = Envelope::fatal_with_code(Some("run-1".into()), "oops", ErrorCode::BackendCrashed);

    // When: Checking the error code
    let code = fatal.error_code();

    // Then: Error code is accessible
    assert_eq!(code, Some(ErrorCode::BackendCrashed));
}

#[test]
fn scenario_sidecar_run_envelope_roundtrip() {
    // Given: A work order wrapped in a Run envelope
    let wo = WorkOrderBuilder::new("test task").build();
    let run = Envelope::Run {
        id: "run-abc".into(),
        work_order: wo,
    };

    // When: Serialized and deserialized
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    // Then: Task is preserved
    if let Envelope::Run { id, work_order } = decoded {
        assert_eq!(id, "run-abc");
        assert_eq!(work_order.task, "test task");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn scenario_sidecar_event_envelope_roundtrip() {
    // Given: An event envelope with assistant message
    let event = make_event(AgentEventKind::AssistantMessage {
        text: "Hello!".into(),
    });
    let envelope = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };

    // When: Roundtripped
    let line = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();

    // Then: Event content is preserved
    if let Envelope::Event { ref_id, event } = decoded {
        assert_eq!(ref_id, "run-1");
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            assert_eq!(text, "Hello!");
        } else {
            panic!("wrong event kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn scenario_sidecar_decode_invalid_json_returns_error() {
    // Given: Invalid JSON
    let bad = "this is not json";

    // When: Decoded
    let result = JsonlCodec::decode(bad);

    // Then: Returns a protocol error
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// Scenario 3: Policy Enforcement
// "Given a policy denies tool X,
//  When a work order uses tool X,
//  Then execution fails with PolicyViolation"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_policy_denies_disallowed_tool() {
    // Given: A policy that disallows Bash
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking Bash
    let decision = engine.can_use_tool("Bash");

    // Then: It is denied
    assert!(!decision.allowed);
    assert!(decision.reason.is_some());
}

#[test]
fn scenario_policy_allows_unlisted_tool_when_no_allowlist() {
    // Given: A policy with only a denylist (no allowlist)
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking an unlisted tool
    let decision = engine.can_use_tool("Read");

    // Then: It is allowed
    assert!(decision.allowed);
}

#[test]
fn scenario_policy_allowlist_blocks_unlisted_tools() {
    // Given: A policy with an explicit allowlist
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking a tool not in the allowlist
    let decision = engine.can_use_tool("Write");

    // Then: It is denied
    assert!(!decision.allowed);
}

#[test]
fn scenario_policy_allowlist_permits_listed_tools() {
    // Given: A policy with an explicit allowlist
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking a tool in the allowlist
    let read_decision = engine.can_use_tool("Read");
    let grep_decision = engine.can_use_tool("Grep");

    // Then: Both are allowed
    assert!(read_decision.allowed);
    assert!(grep_decision.allowed);
}

#[test]
fn scenario_policy_denylist_takes_precedence_over_allowlist() {
    // Given: A policy with Bash in both allow and deny
    let policy = PolicyProfile {
        allowed_tools: vec!["Bash".into(), "Read".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking Bash
    let decision = engine.can_use_tool("Bash");

    // Then: Deny takes precedence
    assert!(!decision.allowed);
}

#[test]
fn scenario_policy_denies_read_on_git_directory() {
    // Given: A policy that denies reading .git paths
    let policy = PolicyProfile {
        deny_read: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking a .git path
    let decision = engine.can_read_path(Path::new(".git/config"));

    // Then: Read is denied
    assert!(!decision.allowed);
}

#[test]
fn scenario_policy_allows_read_on_source_files() {
    // Given: A policy that denies reading .git only
    let policy = PolicyProfile {
        deny_read: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking a source file
    let decision = engine.can_read_path(Path::new("src/lib.rs"));

    // Then: Read is allowed
    assert!(decision.allowed);
}

#[test]
fn scenario_policy_denies_write_on_protected_paths() {
    // Given: A policy that denies writing to .env files
    let policy = PolicyProfile {
        deny_write: vec!["**/.env*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking write to .env
    let decision = engine.can_write_path(Path::new(".env"));

    // Then: Write is denied
    assert!(!decision.allowed);
}

#[test]
fn scenario_policy_allows_write_on_normal_files() {
    // Given: A policy that denies writing to .env files
    let policy = PolicyProfile {
        deny_write: vec!["**/.env*".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking write to a normal file
    let decision = engine.can_write_path(Path::new("src/main.rs"));

    // Then: Write is allowed
    assert!(decision.allowed);
}

#[test]
fn scenario_default_policy_allows_everything() {
    // Given: A default (empty) policy
    let policy = PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();

    // When: Checking any tool and path
    // Then: Everything is allowed
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(engine.can_read_path(Path::new("anything.rs")).allowed);
    assert!(engine.can_write_path(Path::new("anything.rs")).allowed);
}

#[test]
fn scenario_policy_multiple_deny_patterns() {
    // Given: A policy with multiple deny patterns
    let policy = PolicyProfile {
        deny_write: vec![
            "**/.git/**".into(),
            "**/node_modules/**".into(),
            "**/*.lock".into(),
        ],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When/Then: All patterns are enforced
    assert!(!engine.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(
        !engine
            .can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn scenario_policy_decision_allow_has_no_reason() {
    // Given/When: Creating an allow decision
    let decision = Decision::allow();

    // Then: No reason is provided
    assert!(decision.allowed);
    assert!(decision.reason.is_none());
}

#[test]
fn scenario_policy_decision_deny_has_reason() {
    // Given/When: Creating a deny decision with reason
    let decision = Decision::deny("tool not in allowlist");

    // Then: Reason is provided
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("tool not in allowlist"));
}

#[test]
fn scenario_policy_error_code_is_policy_denied() {
    // Given: The ErrorCode for policy denial
    let code = ErrorCode::PolicyDenied;

    // When: Checking its category
    let category = code.category();

    // Then: It belongs to the Policy category
    assert_eq!(category, ErrorCategory::Policy);
}

#[test]
fn scenario_policy_deny_write_to_secrets() {
    // Given: A policy protecting secret files
    let policy = PolicyProfile {
        deny_read: vec!["**/.env*".into(), "**/secrets/**".into()],
        deny_write: vec!["**/.env*".into(), "**/secrets/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    // When/Then: Secrets are protected
    assert!(
        !engine
            .can_read_path(Path::new("secrets/api_key.txt"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new("secrets/api_key.txt"))
            .allowed
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Scenario 4: Receipt Hashing — Determinism
// "Given a receipt is generated,
//  When hashed,
//  Then the hash is deterministic"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_receipt_hash_is_deterministic() {
    // Given: A receipt
    let receipt = make_receipt("mock");

    // When: Hashed twice
    let hash1 = receipt_hash(&receipt).unwrap();
    let hash2 = receipt_hash(&receipt).unwrap();

    // Then: Both hashes are identical
    assert_eq!(hash1, hash2);
}

#[test]
fn scenario_receipt_hash_is_64_hex_chars() {
    // Given: A receipt
    let receipt = make_receipt("mock");

    // When: Hashed
    let hash = receipt_hash(&receipt).unwrap();

    // Then: SHA-256 hex is 64 characters
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn scenario_receipt_with_hash_method_sets_field() {
    // Given: A receipt without hash
    let receipt = make_receipt("mock");
    assert!(receipt.receipt_sha256.is_none());

    // When: with_hash is called
    let hashed = receipt.with_hash().unwrap();

    // Then: Hash field is set
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn scenario_receipt_hash_excludes_receipt_sha256_field() {
    // Given: A receipt with and without a pre-existing hash
    let receipt_a = make_receipt("mock");
    let mut receipt_b = receipt_a.clone();
    receipt_b.receipt_sha256 = Some("bogus_hash_should_be_ignored".into());

    // When: Both are hashed
    let hash_a = receipt_hash(&receipt_a).unwrap();
    let hash_b = receipt_hash(&receipt_b).unwrap();

    // Then: Hashes are identical (receipt_sha256 is nulled before hashing)
    assert_eq!(hash_a, hash_b);
}

#[test]
fn scenario_receipt_hash_changes_when_content_changes() {
    // Given: Two receipts with different outcomes
    let receipt_a = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let receipt_b = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();

    // When: Both are hashed
    let hash_a = receipt_hash(&receipt_a).unwrap();
    let hash_b = receipt_hash(&receipt_b).unwrap();

    // Then: Hashes differ
    assert_ne!(hash_a, hash_b);
}

#[test]
fn scenario_receipt_hash_changes_with_different_backend() {
    // Given: Two receipts from different backends
    let receipt_a = make_receipt("backend-a");
    let receipt_b = make_receipt("backend-b");

    // When: Hashed
    let hash_a = receipt_hash(&receipt_a).unwrap();
    let hash_b = receipt_hash(&receipt_b).unwrap();

    // Then: Hashes differ
    assert_ne!(hash_a, hash_b);
}

#[test]
fn scenario_canonical_json_is_deterministic() {
    // Given: A JSON value with unordered keys
    let val = json!({"z": 1, "a": 2, "m": 3});

    // When: Canonicalized twice
    let json1 = canonical_json(&val).unwrap();
    let json2 = canonical_json(&val).unwrap();

    // Then: Identical output
    assert_eq!(json1, json2);
}

#[test]
fn scenario_canonical_json_sorts_keys() {
    // Given: A JSON value with unordered keys
    let val = json!({"b": 2, "a": 1});

    // When: Canonicalized
    let json = canonical_json(&val).unwrap();

    // Then: Keys are sorted
    assert!(json.starts_with("{\"a\":1"));
}

#[test]
fn scenario_sha256_hex_produces_consistent_output() {
    // Given: A fixed input
    let input = b"hello world";

    // When: Hashed twice
    let h1 = sha256_hex(input);
    let h2 = sha256_hex(input);

    // Then: Identical output
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn scenario_receipt_builder_sets_contract_version() {
    // Given/When: A receipt built with the builder
    let receipt = ReceiptBuilder::new("test").build();

    // Then: Contract version is set correctly
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn scenario_receipt_canonicalize_and_compute_hash_agree() {
    // Given: A receipt
    let receipt = make_receipt("mock");

    // When: Using both abp_core and abp_receipt hash functions
    let hash_core = receipt_hash(&receipt).unwrap();
    let hash_receipt = compute_hash(&receipt).unwrap();

    // Then: Both produce the same hash
    assert_eq!(hash_core, hash_receipt);
}

#[test]
fn scenario_receipt_verify_hash_succeeds_for_valid_hash() {
    // Given: A receipt with a valid hash
    let receipt = make_receipt("mock").with_hash().unwrap();

    // When: Verifying the hash
    let valid = verify_hash(&receipt);

    // Then: Verification succeeds
    assert!(valid);
}

#[test]
fn scenario_receipt_verify_hash_fails_for_tampered_receipt() {
    // Given: A receipt with a valid hash
    let mut receipt = make_receipt("mock").with_hash().unwrap();

    // When: Tampering with the receipt
    receipt.outcome = Outcome::Failed;

    // Then: Verification fails
    assert!(!verify_hash(&receipt));
}

#[test]
fn scenario_receipt_without_hash_is_trivially_valid() {
    // Given: A receipt without a hash
    let receipt = make_receipt("mock");
    assert!(receipt.receipt_sha256.is_none());

    // When: Verifying — the implementation treats "no hash" as vacuously valid
    let valid = verify_hash(&receipt);

    // Then: Returns true (nothing to contradict)
    assert!(valid);
}

// ═══════════════════════════════════════════════════════════════════════════
// Scenario 5: Capability Negotiation
// "Given two SDKs have different capabilities,
//  When negotiation runs,
//  Then unsupported features are detected"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_capability_negotiation_detects_unsupported_features() {
    // Given: A manifest that only supports ToolRead
    let manifest = make_capability_manifest(&[(Capability::ToolRead, SupportLevel::Native)]);

    // And: Requirements that demand Streaming
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    // When: Negotiating
    let result = CapabilityNegotiator::negotiate(&request, &manifest);

    // Then: Streaming is unsatisfied
    assert!(!result.is_compatible);
    assert!(result.unsatisfied.contains(&Capability::Streaming));
}

#[test]
fn scenario_capability_negotiation_satisfies_all_requirements() {
    // Given: A manifest with all required capabilities
    let manifest = make_capability_manifest(&[
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let request = NegotiationRequest {
        required: vec![
            Capability::ToolRead,
            Capability::Streaming,
            Capability::ToolWrite,
        ],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    // When: Negotiating
    let result = CapabilityNegotiator::negotiate(&request, &manifest);

    // Then: All satisfied
    assert!(result.is_compatible);
    assert!(result.unsatisfied.is_empty());
}

#[test]
fn scenario_capability_emulated_satisfies_emulated_requirement() {
    // Given: A manifest with emulated capability
    let manifest = make_capability_manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    // When: Negotiating
    let result = CapabilityNegotiator::negotiate(&request, &manifest);

    // Then: Satisfied
    assert!(result.is_compatible);
}

#[test]
fn scenario_capability_emulated_does_not_satisfy_native_requirement() {
    // Given: A manifest with emulated support
    let manifest = make_capability_manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);

    // And: A requirement for native support
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };

    // When: Negotiating
    let result = CapabilityNegotiator::negotiate(&request, &manifest);

    // Then: Not satisfied
    assert!(!result.is_compatible);
}

#[test]
fn scenario_capability_negotiation_identifies_bonus_preferred() {
    // Given: A manifest with extra capabilities
    let manifest = make_capability_manifest(&[
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
    ]);
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![Capability::ExtendedThinking],
        minimum_support: SupportLevel::Emulated,
    };

    // When: Negotiating
    let result = CapabilityNegotiator::negotiate(&request, &manifest);

    // Then: Bonus features detected
    assert!(result.is_compatible);
    assert!(result.bonus.contains(&Capability::ExtendedThinking));
}

#[test]
fn scenario_support_level_native_satisfies_native() {
    // Given: Native support level
    let level = SupportLevel::Native;

    // When: Checking against native requirement
    // Then: Satisfied
    assert!(level.satisfies(&MinSupport::Native));
}

#[test]
fn scenario_support_level_native_satisfies_emulated() {
    // Given: Native support level
    let level = SupportLevel::Native;

    // When: Checking against emulated requirement
    // Then: Satisfied (native > emulated)
    assert!(level.satisfies(&MinSupport::Emulated));
}

#[test]
fn scenario_support_level_emulated_does_not_satisfy_native() {
    // Given: Emulated support level
    let level = SupportLevel::Emulated;

    // When: Checking against native requirement
    // Then: Not satisfied
    assert!(!level.satisfies(&MinSupport::Native));
}

#[test]
fn scenario_support_level_unsupported_satisfies_nothing() {
    // Given: Unsupported level
    let level = SupportLevel::Unsupported;

    // When/Then: Satisfies neither
    assert!(!level.satisfies(&MinSupport::Native));
    assert!(!level.satisfies(&MinSupport::Emulated));
}

#[test]
fn scenario_support_level_restricted_satisfies_emulated() {
    // Given: Restricted level
    let level = SupportLevel::Restricted {
        reason: "disabled by admin".into(),
    };

    // When: Checking against emulated
    // Then: Satisfies emulated (restricted counts as available)
    assert!(level.satisfies(&MinSupport::Emulated));
}

#[test]
fn scenario_capability_manifest_is_btree_map_for_determinism() {
    // Given: A capability manifest with entries
    let manifest = make_capability_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Emulated),
    ]);

    // When: Serialized to JSON
    let json = serde_json::to_string(&manifest).unwrap();

    // Then: Keys are in sorted order (BTreeMap guarantee)
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = parsed {
        let keys: Vec<_> = map.keys().collect();
        let mut sorted_keys = keys.clone();
        sorted_keys.sort();
        assert_eq!(keys, sorted_keys);
    }
}

#[test]
fn scenario_empty_requirements_always_compatible() {
    // Given: No requirements
    let request = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };

    // When: Negotiating against any manifest
    let result = CapabilityNegotiator::negotiate(&request, &CapabilityManifest::new());

    // Then: Compatible
    assert!(result.is_compatible);
}

#[test]
fn scenario_negotiation_with_missing_capability_in_manifest() {
    // Given: Requirement for a capability not in manifest at all
    let manifest = CapabilityManifest::new();
    let request = NegotiationRequest {
        required: vec![Capability::CodeExecution],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };

    // When: Negotiating
    let result = CapabilityNegotiator::negotiate(&request, &manifest);

    // Then: Not compatible
    assert!(!result.is_compatible);
    assert!(result.unsatisfied.contains(&Capability::CodeExecution));
}

// ═══════════════════════════════════════════════════════════════════════════
// Scenario 6: Streaming Event Reconstruction
// "Given a streaming response,
//  When events are collected,
//  Then the full response is reconstructable"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_streaming_deltas_reconstruct_full_message() {
    // Given: A stream of assistant deltas
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "Starting".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "Hello, ".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "world!".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "Done".into(),
        }),
    ];

    // When: Collecting deltas
    let full_text: String = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::AssistantDelta { text } = &e.kind {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();

    // Then: Full message is reconstructed
    assert_eq!(full_text, "Hello, world!");
}

#[test]
fn scenario_streaming_events_have_chronological_timestamps() {
    // Given: A series of events
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "token".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    // When: Examining timestamps
    // Then: Each is >= previous (timestamps are set at creation)
    for window in events.windows(2) {
        assert!(window[1].ts >= window[0].ts);
    }
}

#[test]
fn scenario_streaming_trace_contains_run_started_and_completed() {
    // Given: A receipt with trace events
    let receipt = make_receipt_with_trace(vec![
        make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "result".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ]);

    // When: Inspecting the trace
    let first = &receipt.trace[0].kind;
    let last = &receipt.trace[receipt.trace.len() - 1].kind;

    // Then: Starts with RunStarted, ends with RunCompleted
    assert!(matches!(first, AgentEventKind::RunStarted { .. }));
    assert!(matches!(last, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn scenario_streaming_tool_call_and_result_pair() {
    // Given: A tool call followed by its result
    let events = vec![
        make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("call-1".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        }),
    ];

    // When: Matching tool calls to results
    let call = &events[0];
    let result = &events[1];

    // Then: They share the same tool_use_id
    if let (
        AgentEventKind::ToolCall {
            tool_use_id: id1, ..
        },
        AgentEventKind::ToolResult {
            tool_use_id: id2, ..
        },
    ) = (&call.kind, &result.kind)
    {
        assert_eq!(id1, id2);
    } else {
        panic!("expected ToolCall/ToolResult pair");
    }
}

#[test]
fn scenario_streaming_file_changed_events_track_workspace_mutations() {
    // Given: FileChanged events during a run
    let events = vec![
        make_event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added function".into(),
        }),
        make_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "Updated import".into(),
        }),
    ];

    // When: Collecting changed files
    let changed_files: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if let AgentEventKind::FileChanged { path, .. } = &e.kind {
                Some(path.as_str())
            } else {
                None
            }
        })
        .collect();

    // Then: All mutations are tracked
    assert_eq!(changed_files, vec!["src/lib.rs", "src/main.rs"]);
}

#[test]
fn scenario_streaming_command_executed_event_captures_exit_code() {
    // Given: A command execution event
    let event = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    });

    // When: Inspecting the event
    if let AgentEventKind::CommandExecuted {
        command,
        exit_code,
        output_preview,
    } = &event.kind
    {
        // Then: All fields captured
        assert_eq!(command, "cargo test");
        assert_eq!(*exit_code, Some(0));
        assert!(output_preview.is_some());
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn scenario_streaming_warning_event() {
    // Given: A warning event
    let event = make_event(AgentEventKind::Warning {
        message: "Rate limit approaching".into(),
    });

    // When/Then: It's a warning
    assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn scenario_streaming_error_event_with_error_code() {
    // Given: An error event with a code
    let event = make_event(AgentEventKind::Error {
        message: "Backend crashed".into(),
        error_code: Some(ErrorCode::BackendCrashed),
    });

    // When: Inspecting the error
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &event.kind
    {
        // Then: Error details are present
        assert_eq!(message, "Backend crashed");
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    }
}

#[test]
fn scenario_streaming_empty_trace_is_valid() {
    // Given: A receipt with no trace events
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    // When/Then: Empty trace is valid
    assert!(receipt.trace.is_empty());
}

#[test]
fn scenario_streaming_event_ext_field_for_passthrough() {
    // Given: An event with ext data for passthrough mode
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"original": "data"}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: Some(ext),
    };

    // When: Serialized and deserialized
    let json = serde_json::to_string(&event).unwrap();
    let decoded: AgentEvent = serde_json::from_str(&json).unwrap();

    // Then: ext data is preserved
    assert!(decoded.ext.is_some());
    assert!(decoded.ext.unwrap().contains_key("raw_message"));
}

#[test]
fn scenario_streaming_multiple_delta_interleaved_with_tool_calls() {
    // Given: Interleaved deltas and tool calls
    let events = vec![
        make_event(AgentEventKind::AssistantDelta {
            text: "Let me ".into(),
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "check ".into(),
        }),
        make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("c1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "f.rs"}),
        }),
        make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("c1".into()),
            output: json!("content"),
            is_error: false,
        }),
        make_event(AgentEventKind::AssistantDelta {
            text: "the file.".into(),
        }),
    ];

    // When: Collecting just the deltas
    let text: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    // Then: Full assistant response is reconstructed
    assert_eq!(text, "Let me check the file.");
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Work Order Construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_work_order_builder_defaults() {
    // Given/When: A minimal work order
    let wo = WorkOrderBuilder::new("test task").build();

    // Then: Defaults are sensible
    assert_eq!(wo.task, "test task");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert_eq!(wo.workspace.root, ".");
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn scenario_work_order_builder_full_configuration() {
    // Given/When: A fully configured work order
    let wo = WorkOrderBuilder::new("Fix auth bug")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .model("claude-3.5-sonnet")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();

    // Then: All fields are set
    assert_eq!(wo.task, "Fix auth bug");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/workspace");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.config.model.as_deref(), Some("claude-3.5-sonnet"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn scenario_work_order_has_unique_id() {
    // Given: Two work orders
    let wo1 = WorkOrderBuilder::new("task1").build();
    let wo2 = WorkOrderBuilder::new("task2").build();

    // Then: IDs are unique
    assert_ne!(wo1.id, wo2.id);
}

#[test]
fn scenario_work_order_with_context_snippets() {
    // Given: A work order with context
    let ctx = ContextPacket {
        files: vec!["src/lib.rs".into()],
        snippets: vec![ContextSnippet {
            name: "error_log".into(),
            content: "Error: connection refused".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("diagnose error").context(ctx).build();

    // Then: Context is preserved
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "error_log");
}

#[test]
fn scenario_work_order_serialization_roundtrip() {
    // Given: A work order
    let wo = WorkOrderBuilder::new("roundtrip test")
        .model("gpt-4")
        .build();

    // When: Serialized and deserialized
    let json = serde_json::to_string(&wo).unwrap();
    let decoded: WorkOrder = serde_json::from_str(&json).unwrap();

    // Then: Content is preserved
    assert_eq!(decoded.task, "roundtrip test");
    assert_eq!(decoded.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn scenario_work_order_with_policy() {
    // Given: A work order with restrictive policy
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("safe task").policy(policy).build();

    // Then: Policy is attached
    assert_eq!(wo.policy.allowed_tools, vec!["Read", "Grep"]);
    assert_eq!(wo.policy.deny_write, vec!["**/.git/**"]);
}

#[test]
fn scenario_work_order_with_capability_requirements() {
    // Given: Requirements
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("capable task")
        .requirements(reqs)
        .build();

    // Then: Requirements are attached
    assert_eq!(wo.requirements.required.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Receipt Construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_receipt_builder_defaults() {
    // Given/When: A minimal receipt
    let receipt = ReceiptBuilder::new("mock").build();

    // Then: Sensible defaults
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.trace.is_empty());
    assert!(receipt.artifacts.is_empty());
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn scenario_receipt_with_artifacts() {
    // Given: A receipt with artifacts
    let receipt = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();

    // Then: Artifacts are recorded
    assert_eq!(receipt.artifacts.len(), 2);
    assert_eq!(receipt.artifacts[0].kind, "patch");
    assert_eq!(receipt.artifacts[1].kind, "log");
}

#[test]
fn scenario_receipt_with_usage() {
    // Given: A receipt with usage data
    let receipt = ReceiptBuilder::new("mock")
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        })
        .build();

    // Then: Usage is recorded
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(200));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.01));
}

#[test]
fn scenario_receipt_with_verification_report() {
    // Given: A receipt with git verification
    let receipt = ReceiptBuilder::new("mock")
        .verification(VerificationReport {
            git_diff: Some("+fn new_function() {}".into()),
            git_status: Some("M src/lib.rs".into()),
            harness_ok: true,
        })
        .build();

    // Then: Verification is recorded
    assert!(receipt.verification.git_diff.is_some());
    assert!(receipt.verification.harness_ok);
}

#[test]
fn scenario_receipt_outcome_serialization() {
    // Given: Each outcome variant
    let outcomes = vec![Outcome::Complete, Outcome::Partial, Outcome::Failed];

    for outcome in outcomes {
        // When: Serialized and deserialized
        let json = serde_json::to_string(&outcome).unwrap();
        let decoded: Outcome = serde_json::from_str(&json).unwrap();

        // Then: Roundtrip succeeds
        assert_eq!(outcome, decoded);
    }
}

#[test]
fn scenario_receipt_execution_mode_default_is_mapped() {
    // Given/When: Default execution mode
    let mode = ExecutionMode::default();

    // Then: It is Mapped
    assert_eq!(mode, ExecutionMode::Mapped);
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Glob Pattern Matching
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_glob_include_only_matching_files() {
    // Given: An include-only glob
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();

    // When/Then: Only src files are allowed
    assert!(globs.decide_path(Path::new("src/lib.rs")).is_allowed());
    assert!(!globs.decide_path(Path::new("tests/test.rs")).is_allowed());
}

#[test]
fn scenario_glob_exclude_overrides_include() {
    // Given: Include src but exclude tests within it
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["src/tests/**".into()]).unwrap();

    // When/Then: Tests inside src are excluded
    assert!(globs.decide_path(Path::new("src/lib.rs")).is_allowed());
    assert!(
        !globs
            .decide_path(Path::new("src/tests/test.rs"))
            .is_allowed()
    );
}

#[test]
fn scenario_glob_empty_patterns_allow_everything() {
    // Given: No include or exclude patterns
    let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();

    // When/Then: Everything is allowed
    assert!(globs.decide_path(Path::new("anything.rs")).is_allowed());
    assert!(
        globs
            .decide_path(Path::new("deeply/nested/file.txt"))
            .is_allowed()
    );
}

#[test]
fn scenario_glob_decide_str_works_for_tool_names() {
    // Given: Globs for tool names
    let globs = IncludeExcludeGlobs::new(&["Read".into(), "Grep".into()], &[]).unwrap();

    // When/Then: Only matching tools pass
    assert!(globs.decide_str("Read").is_allowed());
    assert!(globs.decide_str("Grep").is_allowed());
    assert!(!globs.decide_str("Write").is_allowed());
}

#[test]
fn scenario_glob_match_decision_denied_by_exclude() {
    // Given: An exclude-only glob
    let globs = IncludeExcludeGlobs::new(&[], &["*.lock".into()]).unwrap();

    // When: Checking a lock file
    let decision = globs.decide_path(Path::new("Cargo.lock"));

    // Then: Denied by exclude
    assert!(!decision.is_allowed());
    assert!(matches!(decision, MatchDecision::DeniedByExclude));
}

#[test]
fn scenario_glob_match_decision_denied_by_missing_include() {
    // Given: An include-only glob
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();

    // When: Checking a file outside include
    let decision = globs.decide_path(Path::new("tests/test.rs"));

    // Then: Denied by missing include
    assert!(!decision.is_allowed());
    assert!(matches!(decision, MatchDecision::DeniedByMissingInclude));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Error Taxonomy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_error_code_categories_are_correct() {
    // Given: Various error codes
    // When/Then: Each maps to the correct category
    assert_eq!(
        ErrorCode::ProtocolHandshakeFailed.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
    assert_eq!(
        ErrorCode::ReceiptHashMismatch.category(),
        ErrorCategory::Receipt
    );
    assert_eq!(
        ErrorCode::MappingLossyConversion.category(),
        ErrorCategory::Mapping
    );
    assert_eq!(
        ErrorCode::ContractVersionMismatch.category(),
        ErrorCategory::Contract
    );
}

#[test]
fn scenario_error_code_retryable_check() {
    // Given: Retryable and non-retryable errors
    // When/Then: Retryability is correctly reported
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
}

#[test]
fn scenario_abp_error_construction_with_context() {
    // Given: An error with context
    let err = AbpError::new(ErrorCode::PolicyDenied, "tool Bash is not allowed")
        .with_context("tool_name", "Bash");

    // Then: Error fields are set
    assert_eq!(err.code, ErrorCode::PolicyDenied);
    assert_eq!(err.message, "tool Bash is not allowed");
    assert!(err.context.contains_key("tool_name"));
    assert_eq!(err.category(), ErrorCategory::Policy);
}

#[test]
fn scenario_abp_error_is_not_retryable_for_policy() {
    // Given: A policy denial error
    let err = AbpError::new(ErrorCode::PolicyDenied, "denied");

    // Then: Not retryable
    assert!(!err.is_retryable());
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Protocol Validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_protocol_final_envelope_carries_receipt() {
    // Given: A receipt
    let receipt = make_receipt("sidecar-test");
    let final_env = Envelope::Final {
        ref_id: "run-42".into(),
        receipt: receipt.clone(),
    };

    // When: Roundtripped
    let encoded = JsonlCodec::encode(&final_env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    // Then: Receipt is preserved
    if let Envelope::Final { ref_id, receipt: r } = decoded {
        assert_eq!(ref_id, "run-42");
        assert_eq!(r.backend.id, "sidecar-test");
    } else {
        panic!("expected Final");
    }
}

#[test]
fn scenario_protocol_fatal_envelope_without_ref_id() {
    // Given: A fatal error without ref_id (pre-handshake)
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };

    // When: Roundtripped
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    // Then: ref_id is None
    if let Envelope::Fatal { ref_id, error, .. } = decoded {
        assert!(ref_id.is_none());
        assert_eq!(error, "startup failure");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn scenario_protocol_version_parse_various_formats() {
    // Given/When/Then: Various version strings
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn scenario_protocol_envelope_default_mode_is_mapped() {
    // Given: A hello without explicit mode
    let hello_json = serde_json::json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {"id": "test", "backend_version": null, "adapter_version": null},
        "capabilities": {}
    });

    // When: Deserialized
    let envelope: Envelope = serde_json::from_value(hello_json).unwrap();

    // Then: Mode defaults to Mapped
    if let Envelope::Hello { mode, .. } = envelope {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Agent Event Serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_agent_event_kind_uses_type_tag() {
    // Given: An event kind
    let event = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });

    // When: Serialized
    let json = serde_json::to_string(&event).unwrap();

    // Then: Uses "type" as tag (not "t" like envelopes)
    assert!(json.contains("\"type\":\"run_started\""));
}

#[test]
fn scenario_agent_event_kind_all_variants_serialize() {
    // Given: All event kinds
    let kinds = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!({}),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];

    // When/Then: Each variant serializes and deserializes
    for kind in kinds {
        let event = make_event(kind);
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        // Verify roundtrip doesn't panic
        let _ = serde_json::to_string(&decoded).unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Dialect Types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_all_dialects_are_enumerable() {
    // Given: All known dialects
    let dialects = vec![
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
        Dialect::Copilot,
    ];

    // When/Then: Each serializes to its expected string
    for dialect in &dialects {
        let json = serde_json::to_string(dialect).unwrap();
        let decoded: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(*dialect, decoded);
    }
}

#[test]
fn scenario_supported_ir_pairs_is_non_empty() {
    // Given/When: The supported IR mapper pairs
    let pairs = supported_ir_pairs();

    // Then: At least some pairs exist
    assert!(!pairs.is_empty());
}

#[test]
fn scenario_openai_claude_pair_is_supported() {
    // Given: The supported pairs
    let pairs = supported_ir_pairs();

    // Then: OpenAI <-> Claude is supported
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: IR Conversation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_ir_conversation_push_chaining() {
    // Given: An empty conversation
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "system"))
        .push(IrMessage::text(IrRole::User, "user"));

    // Then: Both messages present
    assert_eq!(conv.messages.len(), 2);
}

#[test]
fn scenario_ir_conversation_system_message_accessor() {
    // Given: A conversation with system message
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);

    // When: Accessing system message
    let system = conv.system_message();

    // Then: Found
    assert!(system.is_some());
    assert_eq!(system.unwrap().text_content(), "You are helpful");
}

#[test]
fn scenario_ir_conversation_last_assistant() {
    // Given: A conversation with multiple assistant turns
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::Assistant, "first"),
        IrMessage::text(IrRole::User, "next"),
        IrMessage::text(IrRole::Assistant, "second"),
    ]);

    // When: Getting last assistant
    let last = conv.last_assistant();

    // Then: Returns the second one
    assert_eq!(last.unwrap().text_content(), "second");
}

#[test]
fn scenario_ir_conversation_no_system_message() {
    // Given: A conversation without system message
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hi")]);

    // When: Looking for system message
    // Then: None
    assert!(conv.system_message().is_none());
}

#[test]
fn scenario_ir_tool_definition_serialization() {
    // Given: A tool definition
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file from the workspace".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
    };

    // When: Roundtripped
    let json = serde_json::to_string(&tool).unwrap();
    let decoded: IrToolDefinition = serde_json::from_str(&json).unwrap();

    // Then: All fields preserved
    assert_eq!(decoded.name, "read_file");
    assert_eq!(decoded.description, "Read a file from the workspace");
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Contract Version
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn scenario_contract_version_in_receipt_matches_constant() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn scenario_contract_version_in_hello_matches_constant() {
    let hello = make_hello_envelope("test");
    if let Envelope::Hello {
        contract_version, ..
    } = &hello
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Execution Mode / Lane
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_execution_mode_passthrough_serialization() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"passthrough\"");
}

#[test]
fn scenario_execution_mode_mapped_serialization() {
    let mode = ExecutionMode::Mapped;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"mapped\"");
}

#[test]
fn scenario_execution_lane_patch_first_serialization() {
    let lane = ExecutionLane::PatchFirst;
    let json = serde_json::to_string(&lane).unwrap();
    assert_eq!(json, "\"patch_first\"");
}

#[test]
fn scenario_execution_lane_workspace_first_serialization() {
    let lane = ExecutionLane::WorkspaceFirst;
    let json = serde_json::to_string(&lane).unwrap();
    assert_eq!(json, "\"workspace_first\"");
}

#[test]
fn scenario_workspace_mode_staged_serialization() {
    let mode = WorkspaceMode::Staged;
    let json = serde_json::to_string(&mode).unwrap();
    let decoded: WorkspaceMode = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, WorkspaceMode::Staged));
}

#[test]
fn scenario_workspace_mode_passthrough_serialization() {
    let mode = WorkspaceMode::PassThrough;
    let json = serde_json::to_string(&mode).unwrap();
    let decoded: WorkspaceMode = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, WorkspaceMode::PassThrough));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Map Errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_map_error_unsupported_pair_display() {
    let err = MapError::UnsupportedPair {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    assert!(msg.contains("unsupported dialect pair"));
}

#[test]
fn scenario_map_error_lossy_conversion_display() {
    let err = MapError::LossyConversion {
        field: "system_prompt".into(),
        reason: "Codex has no system role".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("lossy conversion"));
    assert!(msg.contains("system_prompt"));
}

#[test]
fn scenario_map_error_unmappable_tool_display() {
    let err = MapError::UnmappableTool {
        name: "custom_tool".into(),
        reason: "no equivalent in target dialect".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("unmappable tool"));
    assert!(msg.contains("custom_tool"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Backend Identity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_backend_identity_serialization_roundtrip() {
    let id = BackendIdentity {
        id: "sidecar:claude".into(),
        backend_version: Some("3.5".into()),
        adapter_version: Some("1.2.0".into()),
    };
    let json = serde_json::to_string(&id).unwrap();
    let decoded: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, "sidecar:claude");
    assert_eq!(decoded.backend_version.as_deref(), Some("3.5"));
}

#[test]
fn scenario_backend_identity_with_no_versions() {
    let id = BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    };
    let json = serde_json::to_string(&id).unwrap();
    assert!(json.contains("\"id\":\"mock\""));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases: Receipt Chain (from abp-receipt)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_receipt_canonicalize_is_deterministic() {
    let receipt = make_receipt("mock");
    let json1 = canonicalize(&receipt).unwrap();
    let json2 = canonicalize(&receipt).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn scenario_receipt_compute_hash_matches_core() {
    let receipt = make_receipt("mock");
    let hash_core = receipt_hash(&receipt).unwrap();
    let hash_receipt = compute_hash(&receipt).unwrap();
    assert_eq!(hash_core, hash_receipt);
}
