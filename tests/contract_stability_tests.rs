// SPDX-License-Identifier: MIT OR Apache-2.0
//! Contract version stability and backward compatibility tests.
//!
//! Every test documents the specific contract guarantee it verifies.
//! Breaking any of these tests means a wire-incompatible change was made.

use std::collections::BTreeMap;

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_protocol::version::{self, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};

// =========================================================================
// Helpers
// =========================================================================

fn backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn sample_receipt() -> Receipt {
    let ts = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: backend(),
        capabilities: caps(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn minimal_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

// =========================================================================
// 1. CONTRACT_VERSION constant value and format
// =========================================================================

/// Guarantee: CONTRACT_VERSION is exactly "abp/v0.1".
#[test]
fn contract_version_exact_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

/// Guarantee: CONTRACT_VERSION follows the "abp/vMAJOR.MINOR" format.
#[test]
fn contract_version_format_prefix() {
    assert!(
        CONTRACT_VERSION.starts_with("abp/v"),
        "CONTRACT_VERSION must start with 'abp/v'"
    );
}

/// Guarantee: CONTRACT_VERSION is parseable by parse_version.
#[test]
fn contract_version_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert!(parsed.is_some(), "CONTRACT_VERSION must be parseable");
    let (major, minor) = parsed.unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

/// Guarantee: CONTRACT_VERSION round-trips through ProtocolVersion.
#[test]
fn contract_version_protocol_version_roundtrip() {
    let pv = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    assert_eq!(pv.to_string(), CONTRACT_VERSION);
}

/// Guarantee: ProtocolVersion::current() matches CONTRACT_VERSION.
#[test]
fn protocol_version_current_matches_constant() {
    let current = ProtocolVersion::current();
    assert_eq!(current.major, 0);
    assert_eq!(current.minor, 1);
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

// =========================================================================
// 2. Contract version parsing and comparison
// =========================================================================

/// Guarantee: parse_version rejects strings without "abp/v" prefix.
#[test]
fn parse_version_rejects_missing_prefix() {
    assert_eq!(parse_version("0.1"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("invalid"), None);
}

/// Guarantee: parse_version rejects strings without a dot separator.
#[test]
fn parse_version_rejects_missing_dot() {
    assert_eq!(parse_version("abp/v01"), None);
}

/// Guarantee: parse_version rejects non-numeric components.
#[test]
fn parse_version_rejects_non_numeric() {
    assert_eq!(parse_version("abp/vX.1"), None);
    assert_eq!(parse_version("abp/v0.Y"), None);
}

/// Guarantee: parse_version correctly extracts multi-digit versions.
#[test]
fn parse_version_multi_digit() {
    assert_eq!(parse_version("abp/v12.34"), Some((12, 34)));
}

/// Guarantee: same-major versions are compatible.
#[test]
fn is_compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
}

/// Guarantee: different-major versions are incompatible.
#[test]
fn is_compatible_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

/// Guarantee: unparseable version strings are treated as incompatible.
#[test]
fn is_compatible_invalid_returns_false() {
    assert!(!is_compatible_version("garbage", CONTRACT_VERSION));
    assert!(!is_compatible_version(CONTRACT_VERSION, "garbage"));
}

/// Guarantee: ProtocolVersion ordering is (major, minor) lexicographic.
#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01 < v02);
    assert!(v02 < v10);
}

/// Guarantee: ProtocolVersion::is_compatible checks major match and minor >=.
#[test]
fn protocol_version_is_compatible() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(v01.is_compatible(&v02)); // remote is newer, ok
    assert!(!v02.is_compatible(&v01)); // remote is older than local min
}

// =========================================================================
// 3. Schema stability (JSON output matches expected shapes)
// =========================================================================

/// Guarantee: WorkOrder JSON has all required top-level keys.
#[test]
fn work_order_schema_required_keys() {
    let wo = minimal_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    let obj = v.as_object().unwrap();
    for key in &[
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(obj.contains_key(*key), "WorkOrder missing key: {key}");
    }
}

/// Guarantee: Receipt JSON has all required top-level keys.
#[test]
fn receipt_schema_required_keys() {
    let r = sample_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    let obj = v.as_object().unwrap();
    for key in &[
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
        "receipt_sha256",
    ] {
        assert!(obj.contains_key(*key), "Receipt missing key: {key}");
    }
}

/// Guarantee: RunMetadata JSON has all required fields.
#[test]
fn run_metadata_schema_required_keys() {
    let r = sample_receipt();
    let v: Value = serde_json::to_value(&r.meta).unwrap();
    let obj = v.as_object().unwrap();
    for key in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(obj.contains_key(*key), "RunMetadata missing key: {key}");
    }
}

/// Guarantee: BackendIdentity JSON shape is stable.
#[test]
fn backend_identity_schema_shape() {
    let bi = backend();
    let v: Value = serde_json::to_value(&bi).unwrap();
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("id"));
    assert!(obj.contains_key("backend_version"));
    assert!(obj.contains_key("adapter_version"));
}

// =========================================================================
// 4. WorkOrder schema backward compatibility
// =========================================================================

/// Guarantee: A WorkOrder serialized with the v0.1 schema can always be deserialized.
#[test]
fn work_order_roundtrip_stability() {
    let wo = WorkOrderBuilder::new("refactor auth")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .model("gpt-4")
        .max_turns(10)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "refactor auth");
    assert_eq!(back.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(back.config.max_turns, Some(10));
}

/// Guarantee: A minimal WorkOrder JSON from a prior version is parseable.
#[test]
fn work_order_backward_compat_minimal_json() {
    let json = json!({
        "id": "00000000-0000-0000-0000-000000000000",
        "task": "test",
        "lane": "patch_first",
        "workspace": {
            "root": ".",
            "mode": "staged",
            "include": [],
            "exclude": []
        },
        "context": { "files": [], "snippets": [] },
        "policy": {
            "allowed_tools": [],
            "disallowed_tools": [],
            "deny_read": [],
            "deny_write": [],
            "allow_network": [],
            "deny_network": [],
            "require_approval_for": []
        },
        "requirements": { "required": [] },
        "config": {
            "model": null,
            "vendor": {},
            "env": {},
            "max_budget_usd": null,
            "max_turns": null
        }
    });
    let wo: WorkOrder = serde_json::from_value(json).unwrap();
    assert_eq!(wo.task, "test");
}

/// Guarantee: WorkOrder with all ExecutionLane variants deserializes.
#[test]
fn work_order_execution_lane_variants() {
    for lane_str in &["patch_first", "workspace_first"] {
        let json = json!({
            "id": "00000000-0000-0000-0000-000000000000",
            "task": "test",
            "lane": lane_str,
            "workspace": { "root": ".", "mode": "staged", "include": [], "exclude": [] },
            "context": { "files": [], "snippets": [] },
            "policy": {
                "allowed_tools": [], "disallowed_tools": [],
                "deny_read": [], "deny_write": [],
                "allow_network": [], "deny_network": [],
                "require_approval_for": []
            },
            "requirements": { "required": [] },
            "config": { "model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null }
        });
        let wo: WorkOrder = serde_json::from_value(json).unwrap();
        assert_eq!(wo.task, "test");
    }
}

/// Guarantee: WorkspaceMode variants are stable across versions.
#[test]
fn workspace_mode_variants_stable() {
    let pass: WorkspaceMode = serde_json::from_str(r#""pass_through""#).unwrap();
    assert!(matches!(pass, WorkspaceMode::PassThrough));
    let staged: WorkspaceMode = serde_json::from_str(r#""staged""#).unwrap();
    assert!(matches!(staged, WorkspaceMode::Staged));
}

// =========================================================================
// 5. Receipt schema backward compatibility
// =========================================================================

/// Guarantee: A Receipt serialized with v0.1 schema can be deserialized.
#[test]
fn receipt_roundtrip_stability() {
    let r = sample_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.outcome, Outcome::Complete);
    assert_eq!(back.meta.contract_version, CONTRACT_VERSION);
}

/// Guarantee: Receipt with all Outcome variants deserializes.
#[test]
fn receipt_outcome_variants_stable() {
    for (s, expected) in [
        ("\"complete\"", Outcome::Complete),
        ("\"partial\"", Outcome::Partial),
        ("\"failed\"", Outcome::Failed),
    ] {
        let o: Outcome = serde_json::from_str(s).unwrap();
        assert_eq!(o, expected);
    }
}

/// Guarantee: Receipt with hashed value round-trips correctly.
#[test]
fn receipt_hash_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.receipt_sha256, r.receipt_sha256);
}

/// Guarantee: ReceiptBuilder produces contract_version matching CONTRACT_VERSION.
#[test]
fn receipt_builder_embeds_contract_version() {
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

/// Guarantee: UsageNormalized with all-null fields is valid.
#[test]
fn usage_normalized_all_null_backward_compat() {
    let json = json!({
        "input_tokens": null,
        "output_tokens": null,
        "cache_read_tokens": null,
        "cache_write_tokens": null,
        "request_units": null,
        "estimated_cost_usd": null
    });
    let u: UsageNormalized = serde_json::from_value(json).unwrap();
    assert!(u.input_tokens.is_none());
}

/// Guarantee: VerificationReport with defaults is valid.
#[test]
fn verification_report_default_backward_compat() {
    let json = json!({
        "git_diff": null,
        "git_status": null,
        "harness_ok": false
    });
    let v: VerificationReport = serde_json::from_value(json).unwrap();
    assert!(!v.harness_ok);
}

// =========================================================================
// 6. AgentEvent schema backward compatibility
// =========================================================================

/// Guarantee: AgentEvent with each AgentEventKind variant round-trips.
#[test]
fn agent_event_run_started_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::RunStarted { .. }));
}

/// Guarantee: AssistantMessage variant is stable.
#[test]
fn agent_event_assistant_message_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &back.kind {
        assert_eq!(text, "hello");
    } else {
        panic!("expected AssistantMessage");
    }
}

/// Guarantee: AssistantDelta variant is stable.
#[test]
fn agent_event_assistant_delta_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::AssistantDelta { .. }));
}

/// Guarantee: ToolCall variant is stable with optional fields.
#[test]
fn agent_event_tool_call_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs"}),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::ToolCall { tool_name, .. } = &back.kind {
        assert_eq!(tool_name, "Read");
    } else {
        panic!("expected ToolCall");
    }
}

/// Guarantee: ToolResult variant is stable.
#[test]
fn agent_event_tool_result_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            output: json!("file contents"),
            is_error: false,
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::ToolResult { .. }));
}

/// Guarantee: FileChanged variant is stable.
#[test]
fn agent_event_file_changed_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::FileChanged { .. }));
}

/// Guarantee: CommandExecuted variant is stable.
#[test]
fn agent_event_command_executed_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::CommandExecuted { .. }));
}

/// Guarantee: Warning variant is stable.
#[test]
fn agent_event_warning_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "slow".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::Warning { .. }));
}

/// Guarantee: Error variant is stable.
#[test]
fn agent_event_error_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::Error { .. }));
}

/// Guarantee: RunCompleted variant is stable.
#[test]
fn agent_event_run_completed_roundtrip() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::RunCompleted { .. }));
}

/// Guarantee: AgentEventKind uses "type" as its tag discriminator (not "t").
#[test]
fn agent_event_kind_tag_is_type() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let v: Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("type").is_some(), "AgentEventKind tag must be 'type'");
    assert!(v.get("t").is_none());
}

// =========================================================================
// 7. Capability manifest schema stability
// =========================================================================

/// Guarantee: All Capability enum variants serialize to snake_case strings.
#[test]
fn capability_variants_snake_case() {
    let variants = vec![
        (Capability::Streaming, "streaming"),
        (Capability::ToolRead, "tool_read"),
        (Capability::ToolWrite, "tool_write"),
        (Capability::ToolEdit, "tool_edit"),
        (Capability::ToolBash, "tool_bash"),
        (Capability::ToolGlob, "tool_glob"),
        (Capability::ToolGrep, "tool_grep"),
        (Capability::ToolWebSearch, "tool_web_search"),
        (Capability::ToolWebFetch, "tool_web_fetch"),
        (Capability::ToolAskUser, "tool_ask_user"),
        (Capability::HooksPreToolUse, "hooks_pre_tool_use"),
        (Capability::HooksPostToolUse, "hooks_post_tool_use"),
        (Capability::SessionResume, "session_resume"),
        (Capability::SessionFork, "session_fork"),
        (Capability::Checkpointing, "checkpointing"),
        (
            Capability::StructuredOutputJsonSchema,
            "structured_output_json_schema",
        ),
        (Capability::McpClient, "mcp_client"),
        (Capability::McpServer, "mcp_server"),
        (Capability::ToolUse, "tool_use"),
        (Capability::ExtendedThinking, "extended_thinking"),
        (Capability::ImageInput, "image_input"),
        (Capability::PdfInput, "pdf_input"),
        (Capability::CodeExecution, "code_execution"),
        (Capability::Logprobs, "logprobs"),
        (Capability::SeedDeterminism, "seed_determinism"),
        (Capability::StopSequences, "stop_sequences"),
    ];
    for (cap, expected) in variants {
        let json = serde_json::to_value(&cap).unwrap();
        assert_eq!(json.as_str().unwrap(), expected, "Capability::{cap:?}");
    }
}

/// Guarantee: SupportLevel variants are snake_case and round-trip.
#[test]
fn support_level_variants_stable() {
    let native: SupportLevel = serde_json::from_str(r#""native""#).unwrap();
    assert!(matches!(native, SupportLevel::Native));
    let emulated: SupportLevel = serde_json::from_str(r#""emulated""#).unwrap();
    assert!(matches!(emulated, SupportLevel::Emulated));
    let unsupported: SupportLevel = serde_json::from_str(r#""unsupported""#).unwrap();
    assert!(matches!(unsupported, SupportLevel::Unsupported));
}

/// Guarantee: SupportLevel::Restricted with reason round-trips.
#[test]
fn support_level_restricted_roundtrip() {
    let sl = SupportLevel::Restricted {
        reason: "sandboxed".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    if let SupportLevel::Restricted { reason } = &back {
        assert_eq!(reason, "sandboxed");
    } else {
        panic!("expected Restricted");
    }
}

/// Guarantee: CapabilityManifest (BTreeMap) serializes as object.
#[test]
fn capability_manifest_serializes_as_object() {
    let manifest = caps();
    let v: Value = serde_json::to_value(&manifest).unwrap();
    assert!(v.is_object());
    assert!(v.get("streaming").is_some());
}

/// Guarantee: MinSupport variants are stable.
#[test]
fn min_support_variants_stable() {
    let native: MinSupport = serde_json::from_str(r#""native""#).unwrap();
    assert!(matches!(native, MinSupport::Native));
    let emulated: MinSupport = serde_json::from_str(r#""emulated""#).unwrap();
    assert!(matches!(emulated, MinSupport::Emulated));
}

/// Guarantee: CapabilityRequirements with required list round-trips.
#[test]
fn capability_requirements_roundtrip() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let json = serde_json::to_string(&reqs).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 1);
}

// =========================================================================
// 8. Policy profile schema stability
// =========================================================================

/// Guarantee: PolicyProfile with all fields round-trips.
#[test]
fn policy_profile_full_roundtrip() {
    let pp = PolicyProfile {
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec!["**/.git/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["Bash".into()],
    };
    let json = serde_json::to_string(&pp).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.allowed_tools, vec!["Read"]);
    assert_eq!(back.disallowed_tools, vec!["Bash"]);
    assert_eq!(back.deny_read, vec!["**/.env"]);
    assert_eq!(back.deny_write, vec!["**/.git/**"]);
    assert_eq!(back.allow_network, vec!["*.example.com"]);
    assert_eq!(back.deny_network, vec!["evil.com"]);
    assert_eq!(back.require_approval_for, vec!["Bash"]);
}

/// Guarantee: PolicyProfile default is all-empty (no restrictions).
#[test]
fn policy_profile_default_is_permissive() {
    let pp = PolicyProfile::default();
    let json = serde_json::to_string(&pp).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    let obj = v.as_object().unwrap();
    for key in &[
        "allowed_tools",
        "disallowed_tools",
        "deny_read",
        "deny_write",
        "allow_network",
        "deny_network",
        "require_approval_for",
    ] {
        let arr = obj[*key].as_array().unwrap();
        assert!(arr.is_empty(), "PolicyProfile default {key} must be empty");
    }
}

/// Guarantee: PolicyProfile schema has all seven fields.
#[test]
fn policy_profile_schema_all_fields_present() {
    let pp = PolicyProfile::default();
    let v: Value = serde_json::to_value(&pp).unwrap();
    let obj = v.as_object().unwrap();
    let expected_keys = [
        "allowed_tools",
        "disallowed_tools",
        "deny_read",
        "deny_write",
        "allow_network",
        "deny_network",
        "require_approval_for",
    ];
    for key in &expected_keys {
        assert!(obj.contains_key(*key), "PolicyProfile missing field: {key}");
    }
}

// =========================================================================
// 9. Breaking change detection
// =========================================================================

/// Guarantee: Removing "task" from WorkOrder JSON breaks deserialization.
#[test]
fn breaking_change_work_order_missing_task() {
    let json = json!({
        "id": "00000000-0000-0000-0000-000000000000",
        "lane": "patch_first",
        "workspace": { "root": ".", "mode": "staged", "include": [], "exclude": [] },
        "context": { "files": [], "snippets": [] },
        "policy": {
            "allowed_tools": [], "disallowed_tools": [],
            "deny_read": [], "deny_write": [],
            "allow_network": [], "deny_network": [],
            "require_approval_for": []
        },
        "requirements": { "required": [] },
        "config": { "model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null }
    });
    assert!(serde_json::from_value::<WorkOrder>(json).is_err());
}

/// Guarantee: Removing "id" from WorkOrder JSON breaks deserialization.
#[test]
fn breaking_change_work_order_missing_id() {
    let json = json!({
        "task": "test",
        "lane": "patch_first",
        "workspace": { "root": ".", "mode": "staged", "include": [], "exclude": [] },
        "context": { "files": [], "snippets": [] },
        "policy": {
            "allowed_tools": [], "disallowed_tools": [],
            "deny_read": [], "deny_write": [],
            "allow_network": [], "deny_network": [],
            "require_approval_for": []
        },
        "requirements": { "required": [] },
        "config": { "model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null }
    });
    assert!(serde_json::from_value::<WorkOrder>(json).is_err());
}

/// Guarantee: Removing "outcome" from Receipt JSON breaks deserialization.
#[test]
fn breaking_change_receipt_missing_outcome() {
    let r = sample_receipt();
    let mut v: Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut().unwrap().remove("outcome");
    assert!(serde_json::from_value::<Receipt>(v).is_err());
}

/// Guarantee: Removing "meta" from Receipt JSON breaks deserialization.
#[test]
fn breaking_change_receipt_missing_meta() {
    let r = sample_receipt();
    let mut v: Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut().unwrap().remove("meta");
    assert!(serde_json::from_value::<Receipt>(v).is_err());
}

/// Guarantee: Removing "backend" from Receipt JSON breaks deserialization.
#[test]
fn breaking_change_receipt_missing_backend() {
    let r = sample_receipt();
    let mut v: Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut().unwrap().remove("backend");
    assert!(serde_json::from_value::<Receipt>(v).is_err());
}

/// Guarantee: Removing "type" tag from AgentEvent JSON breaks deserialization.
#[test]
fn breaking_change_agent_event_missing_type_tag() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "text": "hi"
    });
    assert!(serde_json::from_value::<AgentEvent>(json).is_err());
}

/// Guarantee: Removing "id" from BackendIdentity breaks deserialization.
#[test]
fn breaking_change_backend_identity_missing_id() {
    let json = json!({
        "backend_version": "1.0.0",
        "adapter_version": null
    });
    assert!(serde_json::from_value::<BackendIdentity>(json).is_err());
}

// =========================================================================
// 10. Optional field handling
// =========================================================================

/// Guarantee: WorkOrder with extra unknown fields can still be deserialized
/// if serde is not in deny_unknown_fields mode (forward-compat).
#[test]
fn optional_field_runtime_config_extra_ignored() {
    let json = json!({
        "model": "gpt-4",
        "vendor": {},
        "env": {},
        "max_budget_usd": null,
        "max_turns": null,
        "future_field": "value"
    });
    // RuntimeConfig does not use deny_unknown_fields, so extra fields are ok
    let rc: RuntimeConfig = serde_json::from_value(json).unwrap();
    assert_eq!(rc.model.as_deref(), Some("gpt-4"));
}

/// Guarantee: Receipt mode field has a default (backward compat for older JSON).
#[test]
fn optional_field_receipt_mode_defaults() {
    let r = sample_receipt();
    let mut v: Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut().unwrap().remove("mode");
    let back: Receipt = serde_json::from_value(v).unwrap();
    assert_eq!(back.mode, ExecutionMode::Mapped);
}

/// Guarantee: AgentEvent ext field is optional and defaults to None.
#[test]
fn optional_field_agent_event_ext_absent() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "assistant_message",
        "text": "hello"
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    assert!(e.ext.is_none());
}

/// Guarantee: AgentEvent ext field can carry passthrough data.
#[test]
fn optional_field_agent_event_ext_present() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"role": "assistant"}));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(back.ext.is_some());
    assert!(back.ext.unwrap().contains_key("raw_message"));
}

/// Guarantee: ToolCall optional fields (tool_use_id, parent_tool_use_id) can be null.
#[test]
fn optional_field_tool_call_ids_nullable() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "tool_call",
        "tool_name": "Read",
        "tool_use_id": null,
        "parent_tool_use_id": null,
        "input": {}
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_use_id,
        parent_tool_use_id,
        ..
    } = &e.kind
    {
        assert!(tool_use_id.is_none());
        assert!(parent_tool_use_id.is_none());
    } else {
        panic!("expected ToolCall");
    }
}

/// Guarantee: CommandExecuted optional fields can be null.
#[test]
fn optional_field_command_executed_nullables() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "command_executed",
        "command": "ls",
        "exit_code": null,
        "output_preview": null
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(e.kind, AgentEventKind::CommandExecuted { .. }));
}

/// Guarantee: BackendIdentity optional fields can be null.
#[test]
fn optional_field_backend_identity_nullables() {
    let json = json!({
        "id": "mock",
        "backend_version": null,
        "adapter_version": null
    });
    let bi: BackendIdentity = serde_json::from_value(json).unwrap();
    assert_eq!(bi.id, "mock");
    assert!(bi.backend_version.is_none());
}

/// Guarantee: UsageNormalized fields are all optional.
#[test]
fn optional_field_usage_normalized_all_optional() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

// =========================================================================
// 11. Enum variant stability
// =========================================================================

/// Guarantee: ExecutionMode::Mapped is the default.
#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

/// Guarantee: ExecutionMode variants are snake_case and stable.
#[test]
fn execution_mode_variants_stable() {
    let mapped: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(mapped, ExecutionMode::Mapped);
    let passthrough: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(passthrough, ExecutionMode::Passthrough);
}

/// Guarantee: ExecutionLane variants are snake_case and stable.
#[test]
fn execution_lane_variants_stable() {
    let pf: ExecutionLane = serde_json::from_str(r#""patch_first""#).unwrap();
    assert!(matches!(pf, ExecutionLane::PatchFirst));
    let wf: ExecutionLane = serde_json::from_str(r#""workspace_first""#).unwrap();
    assert!(matches!(wf, ExecutionLane::WorkspaceFirst));
}

/// Guarantee: Outcome enum variants are snake_case and exhaustive.
#[test]
fn outcome_variants_exhaustive() {
    let variants = ["complete", "partial", "failed"];
    for v in &variants {
        let json_str = format!("\"{v}\"");
        let o: Outcome = serde_json::from_str(&json_str).unwrap();
        let back = serde_json::to_string(&o).unwrap();
        assert_eq!(back, json_str);
    }
}

/// Guarantee: AgentEventKind variant tags are snake_case.
#[test]
fn agent_event_kind_variant_tags_snake_case() {
    let test_cases: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted { message: "".into() },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted { message: "".into() },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta { text: "".into() },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage { text: "".into() },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: "".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!(null),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: "".into(),
                tool_use_id: None,
                output: json!(null),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: "".into(),
                summary: "".into(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: "".into(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (AgentEventKind::Warning { message: "".into() }, "warning"),
        (
            AgentEventKind::Error {
                message: "".into(),
                error_code: None,
            },
            "error",
        ),
    ];
    for (kind, expected_tag) in test_cases {
        let v: Value = serde_json::to_value(&kind).unwrap();
        assert_eq!(
            v.get("type").unwrap().as_str().unwrap(),
            expected_tag,
            "AgentEventKind variant tag mismatch"
        );
    }
}

// =========================================================================
// 12. Wire protocol envelope format stability
// =========================================================================

/// Guarantee: Envelope uses "t" as the discriminator tag (not "type").
#[test]
fn envelope_discriminator_is_t() {
    let env = Envelope::hello(backend(), CapabilityManifest::new());
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\":\"hello\""), "Envelope tag must be 't'");
    assert!(
        !json.contains("\"type\":\"hello\""),
        "Envelope must not use 'type' as tag"
    );
}

/// Guarantee: Envelope::Hello variant includes contract_version, backend, capabilities.
#[test]
fn envelope_hello_shape() {
    let env = Envelope::hello(backend(), caps());
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "hello");
    assert!(v.get("contract_version").is_some());
    assert!(v.get("backend").is_some());
    assert!(v.get("capabilities").is_some());
}

/// Guarantee: Envelope::Hello contract_version matches CONTRACT_VERSION.
#[test]
fn envelope_hello_contract_version_value() {
    let env = Envelope::hello(backend(), caps());
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["contract_version"].as_str().unwrap(), CONTRACT_VERSION);
}

/// Guarantee: Envelope::Run includes id and work_order.
#[test]
fn envelope_run_shape() {
    let wo = minimal_work_order();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "run");
    assert!(v.get("id").is_some());
    assert!(v.get("work_order").is_some());
}

/// Guarantee: Envelope::Event includes ref_id and event.
#[test]
fn envelope_event_shape() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "event");
    assert!(v.get("ref_id").is_some());
    assert!(v.get("event").is_some());
}

/// Guarantee: Envelope::Final includes ref_id and receipt.
#[test]
fn envelope_final_shape() {
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: sample_receipt(),
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "final");
    assert!(v.get("ref_id").is_some());
    assert!(v.get("receipt").is_some());
}

/// Guarantee: Envelope::Fatal includes ref_id and error.
#[test]
fn envelope_fatal_shape() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let v: Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["t"], "fatal");
    assert!(v.get("error").is_some());
}

/// Guarantee: All five envelope variant tags are stable.
#[test]
fn envelope_variant_tags_stable() {
    let expected_tags = ["hello", "run", "event", "final", "fatal"];
    let envelopes: [Envelope; 5] = [
        Envelope::hello(backend(), caps()),
        Envelope::Run {
            id: "r".into(),
            work_order: minimal_work_order(),
        },
        Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Warning {
                    message: "w".into(),
                },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: "r".into(),
            receipt: sample_receipt(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "e".into(),
            error_code: None,
        },
    ];
    for (env, expected) in envelopes.iter().zip(expected_tags.iter()) {
        let v: Value = serde_json::to_value(env).unwrap();
        assert_eq!(v["t"].as_str().unwrap(), *expected);
    }
}

/// Guarantee: JSONL encoding produces newline-terminated output.
#[test]
fn jsonl_encoding_newline_terminated() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
}

/// Guarantee: JSONL round-trip preserves envelope variant.
#[test]
fn jsonl_roundtrip_preserves_variant() {
    let env = Envelope::hello(backend(), caps());
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

// =========================================================================
// 13. Default value stability
// =========================================================================

/// Guarantee: WorkOrderBuilder defaults lane to PatchFirst.
#[test]
fn default_work_order_lane_is_patch_first() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

/// Guarantee: WorkOrderBuilder defaults workspace mode to Staged.
#[test]
fn default_work_order_workspace_mode_is_staged() {
    let wo = WorkOrderBuilder::new("test").build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

/// Guarantee: WorkOrderBuilder defaults root to ".".
#[test]
fn default_work_order_root_is_dot() {
    let wo = WorkOrderBuilder::new("test").build();
    assert_eq!(wo.workspace.root, ".");
}

/// Guarantee: RuntimeConfig default model is None.
#[test]
fn default_runtime_config_model_is_none() {
    let rc = RuntimeConfig::default();
    assert!(rc.model.is_none());
}

/// Guarantee: RuntimeConfig default max_turns is None.
#[test]
fn default_runtime_config_max_turns_is_none() {
    let rc = RuntimeConfig::default();
    assert!(rc.max_turns.is_none());
}

/// Guarantee: RuntimeConfig default max_budget_usd is None.
#[test]
fn default_runtime_config_max_budget_is_none() {
    let rc = RuntimeConfig::default();
    assert!(rc.max_budget_usd.is_none());
}

/// Guarantee: ContextPacket default is empty files and snippets.
#[test]
fn default_context_packet_is_empty() {
    let cp = ContextPacket::default();
    assert!(cp.files.is_empty());
    assert!(cp.snippets.is_empty());
}

/// Guarantee: CapabilityRequirements default is empty.
#[test]
fn default_capability_requirements_is_empty() {
    let cr = CapabilityRequirements::default();
    assert!(cr.required.is_empty());
}

/// Guarantee: VerificationReport default has harness_ok=false.
#[test]
fn default_verification_report_harness_false() {
    let vr = VerificationReport::default();
    assert!(!vr.harness_ok);
    assert!(vr.git_diff.is_none());
    assert!(vr.git_status.is_none());
}

// =========================================================================
// 14. Version negotiation handshake compatibility
// =========================================================================

/// Guarantee: negotiate_version succeeds when majors match.
#[test]
fn version_negotiation_same_major_succeeds() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = version::negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.major, 0);
    assert_eq!(result.minor, 1); // min of 1 and 2
}

/// Guarantee: negotiate_version picks the lower minor version.
#[test]
fn version_negotiation_picks_minimum() {
    let local = ProtocolVersion::parse("abp/v0.3").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = version::negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.minor, 1);
}

/// Guarantee: negotiate_version fails when majors differ.
#[test]
fn version_negotiation_different_major_fails() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = version::negotiate_version(&local, &remote).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}

/// Guarantee: negotiate_version with identical versions succeeds.
#[test]
fn version_negotiation_identical_versions() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    let result = version::negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

/// Guarantee: VersionRange::contains works correctly.
#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
}

/// Guarantee: VersionRange::is_compatible requires same major for all bounds.
#[test]
fn version_range_is_compatible_same_major() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

/// Guarantee: VersionError::InvalidFormat for malformed strings.
#[test]
fn version_parse_error_invalid_format() {
    let err = ProtocolVersion::parse("invalid").unwrap_err();
    assert_eq!(err, VersionError::InvalidFormat);
}

/// Guarantee: VersionError::InvalidMajor for non-numeric major.
#[test]
fn version_parse_error_invalid_major() {
    let err = ProtocolVersion::parse("abp/vX.1").unwrap_err();
    assert_eq!(err, VersionError::InvalidMajor);
}

/// Guarantee: VersionError::InvalidMinor for non-numeric minor.
#[test]
fn version_parse_error_invalid_minor() {
    let err = ProtocolVersion::parse("abp/v0.Y").unwrap_err();
    assert_eq!(err, VersionError::InvalidMinor);
}

/// Guarantee: Handshake hello envelope from a compatible sidecar is decodable.
#[test]
fn handshake_hello_decodable_from_json() {
    let json = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"sidecar","backend_version":null,"adapter_version":null}},"capabilities":{{}},"mode":"mapped"}}"#,
        CONTRACT_VERSION
    );
    let env = JsonlCodec::decode(&json).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

/// Guarantee: Handshake hello envelope without mode field defaults to mapped.
#[test]
fn handshake_hello_mode_defaults_to_mapped() {
    let json = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"s","backend_version":null,"adapter_version":null}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION
    );
    let env = JsonlCodec::decode(&json).unwrap();
    if let Envelope::Hello { mode, .. } = &env {
        assert_eq!(*mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

/// Guarantee: Fatal envelope with null ref_id is valid.
#[test]
fn fatal_envelope_null_ref_id() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"oops"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = &env {
        assert!(ref_id.is_none());
        assert_eq!(error, "oops");
    } else {
        panic!("expected Fatal");
    }
}

/// Guarantee: Fatal envelope error_code is optional.
#[test]
fn fatal_envelope_error_code_optional() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"oops"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(env.error_code().is_none());
}

/// Guarantee: ProtocolVersion Display format matches "abp/vMAJOR.MINOR".
#[test]
fn protocol_version_display_format() {
    let v = ProtocolVersion { major: 2, minor: 5 };
    assert_eq!(format!("{v}"), "abp/v2.5");
}

/// Guarantee: ProtocolVersion serde round-trip.
#[test]
fn protocol_version_serde_roundtrip() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    let json = serde_json::to_string(&v).unwrap();
    let back: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(back, v);
}

/// Guarantee: ContextSnippet round-trips.
#[test]
fn context_snippet_roundtrip() {
    let cs = ContextSnippet {
        name: "readme".into(),
        content: "# Hello".into(),
    };
    let json = serde_json::to_string(&cs).unwrap();
    let back: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "readme");
    assert_eq!(back.content, "# Hello");
}

/// Guarantee: ArtifactRef round-trips.
#[test]
fn artifact_ref_roundtrip() {
    let ar = ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    };
    let json = serde_json::to_string(&ar).unwrap();
    let back: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.kind, "patch");
    assert_eq!(back.path, "out.patch");
}
