// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive documentation verification tests.
//!
//! Validates that all documented examples, patterns, and public API
//! contracts work as described in the crate documentation.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::{Decision, PolicyEngine};
use abp_protocol::builder::EnvelopeBuilder;
use abp_protocol::validate::{EnvelopeValidator, SequenceError, ValidationError};
use abp_protocol::version::{ProtocolVersion, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use uuid::Uuid;

// =========================================================================
// 1. CONTRACT_VERSION documentation examples
// =========================================================================

#[test]
fn doc_contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn doc_contract_version_format() {
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
    let (major, minor) = parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(major, 0);
    assert_eq!(minor, 1);
}

// =========================================================================
// 2. WorkOrderBuilder doc examples
// =========================================================================

#[test]
fn doc_work_order_builder_basic() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    assert_eq!(wo.task, "Refactor auth module");
}

#[test]
fn doc_work_order_builder_full() {
    let wo = WorkOrderBuilder::new("Fix the login bug")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/workspace")
        .model("gpt-4")
        .max_turns(10)
        .build();

    assert_eq!(wo.task, "Fix the login bug");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn doc_work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.workspace.root, ".");
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn doc_work_order_builder_workspace_mode() {
    let wo = WorkOrderBuilder::new("task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn doc_work_order_builder_include_exclude() {
    let wo = WorkOrderBuilder::new("task")
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn doc_work_order_builder_policy() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.disallowed_tools, vec!["Bash"]);
}

#[test]
fn doc_work_order_builder_max_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn doc_work_order_builder_context() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "Use pattern X".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    assert_eq!(wo.context.files, vec!["src/main.rs"]);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn doc_work_order_builder_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn doc_work_order_builder_config() {
    let config = RuntimeConfig {
        model: Some("claude-3".into()),
        max_turns: Some(5),
        ..RuntimeConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn doc_work_order_has_unique_id() {
    let wo1 = WorkOrderBuilder::new("task1").build();
    let wo2 = WorkOrderBuilder::new("task2").build();
    assert_ne!(wo1.id, wo2.id);
}

// =========================================================================
// 3. ReceiptBuilder doc examples
// =========================================================================

#[test]
fn doc_receipt_builder_basic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn doc_receipt_with_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn doc_receipt_builder_with_hash_shortcut() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn doc_receipt_builder_versions() {
    let receipt = ReceiptBuilder::new("test")
        .backend_version("2.0")
        .adapter_version("1.0")
        .build();
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("2.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("1.0"));
}

#[test]
fn doc_receipt_builder_mode() {
    let receipt = ReceiptBuilder::new("test")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn doc_receipt_builder_trace_events() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(receipt.trace.len(), 1);
}

#[test]
fn doc_receipt_builder_artifacts() {
    let receipt = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

#[test]
fn doc_receipt_builder_work_order_id() {
    let id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("mock").work_order_id(id).build();
    assert_eq!(receipt.meta.work_order_id, id);
}

#[test]
fn doc_receipt_builder_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..UsageNormalized::default()
    };
    let receipt = ReceiptBuilder::new("mock").usage(usage).build();
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
}

#[test]
fn doc_receipt_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff output".into()),
        git_status: Some("M src/lib.rs".into()),
        harness_ok: true,
    };
    let receipt = ReceiptBuilder::new("mock").verification(v).build();
    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_some());
}

// =========================================================================
// 4. Receipt hashing doc examples
// =========================================================================

#[test]
fn doc_receipt_hash_deterministic() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&receipt).unwrap();
    let h2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn doc_receipt_hash_nullifies_sha256_field() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    let hash_before = abp_core::receipt_hash(&receipt).unwrap();
    receipt.receipt_sha256 = Some("bogus".into());
    let hash_after = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash_before, hash_after);
}

#[test]
fn doc_sha256_hex() {
    let hex = abp_core::sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
    // Known SHA-256 of "hello"
    assert_eq!(
        hex,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn doc_canonical_json_sorted_keys() {
    let json = abp_core::canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
    assert!(json.starts_with(r#"{"a":1"#));
}

// =========================================================================
// 5. Capability and SupportLevel doc examples
// =========================================================================

#[test]
fn doc_capability_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    assert!(manifest.contains_key(&Capability::ToolRead));
}

#[test]
fn doc_support_level_satisfies() {
    let native = SupportLevel::Native;
    assert!(native.satisfies(&MinSupport::Native));
    assert!(native.satisfies(&MinSupport::Emulated));

    let emulated = SupportLevel::Emulated;
    assert!(!emulated.satisfies(&MinSupport::Native));
    assert!(emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn doc_support_level_unsupported() {
    let unsupported = SupportLevel::Unsupported;
    assert!(!unsupported.satisfies(&MinSupport::Native));
    assert!(!unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn doc_support_level_restricted() {
    let restricted = SupportLevel::Restricted {
        reason: "disabled by policy".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

// =========================================================================
// 6. ExecutionMode doc examples
// =========================================================================

#[test]
fn doc_execution_mode_default() {
    let mode = ExecutionMode::default();
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn doc_execution_mode_serde() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, r#""passthrough""#);
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, r#""mapped""#);
}

// =========================================================================
// 7. BackendIdentity doc examples
// =========================================================================

#[test]
fn doc_backend_identity() {
    let id = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    };
    assert_eq!(id.id, "sidecar:node");
}

// =========================================================================
// 8. AgentEvent doc examples
// =========================================================================

#[test]
fn doc_agent_event_creation() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello, world!".into(),
        },
        ext: None,
    };
    assert!(matches!(
        event.kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn doc_agent_event_kinds() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        AgentEventKind::AssistantDelta { text: "tok".into() },
        AgentEventKind::AssistantMessage { text: "msg".into() },
        AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: None,
            output: serde_json::json!("content"),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "modified".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "caution".into(),
        },
        AgentEventKind::Error {
            message: "fail".into(),
            error_code: None,
        },
    ];
    assert_eq!(kinds.len(), 10);
}

#[test]
fn doc_agent_event_serde_tag_type() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    // AgentEventKind uses serde(tag = "type")
    assert!(json.contains(r#""type":"assistant_message""#));
}

// =========================================================================
// 9. Outcome doc examples
// =========================================================================

#[test]
fn doc_outcome_serde() {
    let outcome: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(outcome, Outcome::Complete);

    let outcome: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(outcome, Outcome::Partial);

    let outcome: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(outcome, Outcome::Failed);
}

// =========================================================================
// 10. Envelope doc examples (protocol)
// =========================================================================

#[test]
fn doc_envelope_hello() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "my-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"hello\""));

    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn doc_envelope_hello_with_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    if let Envelope::Hello { mode, .. } = &hello {
        assert_eq!(*mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn doc_envelope_fatal() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-123".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&fatal).unwrap();
    assert!(json.ends_with('\n'));
}

#[test]
fn doc_envelope_decode_fatal() {
    let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let envelope = JsonlCodec::decode(line).unwrap();
    assert!(matches!(
        envelope,
        Envelope::Fatal { ref error, .. } if error == "boom"
    ));
}

// =========================================================================
// 11. JsonlCodec doc examples
// =========================================================================

#[test]
fn doc_jsonl_codec_decode_invalid() {
    let err = JsonlCodec::decode("not valid json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn doc_jsonl_codec_decode_stream() {
    let input = r#"{"t":"fatal","ref_id":null,"error":"boom"}
{"t":"fatal","ref_id":null,"error":"bang"}
"#;
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn doc_jsonl_codec_encode_to_writer() {
    let envelope = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &envelope).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(s.contains("\"t\":\"fatal\""));
}

#[test]
fn doc_jsonl_codec_encode_many() {
    let envelopes = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        },
        Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        },
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.lines().count(), 2);
}

// =========================================================================
// 12. parse_version and is_compatible_version doc examples
// =========================================================================

#[test]
fn doc_parse_version() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("invalid"), None);
}

#[test]
fn doc_is_compatible_version() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

// =========================================================================
// 13. ProtocolVersion doc examples (version module)
// =========================================================================

#[test]
fn doc_protocol_version_parse() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    assert!(ProtocolVersion::parse("invalid").is_err());
}

#[test]
fn doc_protocol_version_current() {
    let current = ProtocolVersion::current();
    assert_eq!(current.major, 0);
    assert_eq!(current.minor, 1);
}

#[test]
fn doc_protocol_version_display() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(format!("{v}"), "abp/v0.1");
}

#[test]
fn doc_protocol_version_compatibility() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01.is_compatible(&v02));
    assert!(!v01.is_compatible(&v10));
}

#[test]
fn doc_version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.2").unwrap()));
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.4").unwrap()));
}

#[test]
fn doc_negotiate_version_ok() {
    let local = ProtocolVersion::parse("abp/v0.2").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result.minor, 1); // minimum
}

#[test]
fn doc_negotiate_version_incompatible() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&local, &remote).is_err());
}

// =========================================================================
// 14. EnvelopeBuilder doc examples
// =========================================================================

#[test]
fn doc_envelope_builder_hello() {
    let envelope = EnvelopeBuilder::hello()
        .backend("my-sidecar")
        .version("1.0.0")
        .build()
        .unwrap();

    match &envelope {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "my-sidecar"),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn doc_envelope_builder_hello_missing_backend() {
    let err = EnvelopeBuilder::hello().build().unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("backend")
    );
}

#[test]
fn doc_envelope_builder_hello_all_fields() {
    let env = EnvelopeBuilder::hello()
        .backend("sidecar")
        .version("2.0")
        .adapter_version("1.0")
        .mode(ExecutionMode::Passthrough)
        .capabilities(CapabilityManifest::new())
        .build()
        .unwrap();
    if let Envelope::Hello {
        backend,
        mode,
        contract_version,
        ..
    } = &env
    {
        assert_eq!(backend.id, "sidecar");
        assert_eq!(*mode, ExecutionMode::Passthrough);
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn doc_envelope_builder_run() {
    let wo = WorkOrderBuilder::new("test task").build();
    let wo_id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    if let Envelope::Run { id, work_order } = &env {
        assert_eq!(id, &wo_id);
        assert_eq!(work_order.task, "test task");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn doc_envelope_builder_fatal() {
    let env = EnvelopeBuilder::fatal("something broke")
        .ref_id("run-1")
        .build()
        .unwrap();
    if let Envelope::Fatal { ref_id, error, .. } = &env {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "something broke");
    } else {
        panic!("expected Fatal");
    }
}

// =========================================================================
// 15. Documented protocol flow: hello → run → event* → final
// =========================================================================

#[test]
fn doc_protocol_flow_hello_run_events_final() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );

    let wo = WorkOrderBuilder::new("test").build();
    let run_id = wo.id.to_string();
    let run = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };

    let event = Envelope::Event {
        ref_id: run_id.clone(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "working...".into(),
            },
            ext: None,
        },
    };

    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build();
    let final_env = Envelope::Final {
        ref_id: run_id.clone(),
        receipt,
    };

    // Encode all as JSONL and decode the stream
    let mut buf = Vec::new();
    for env in &[&hello, &run, &event, &final_env] {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

#[test]
fn doc_protocol_flow_fatal_instead_of_final() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );

    let wo = WorkOrderBuilder::new("task").build();
    let run_id = wo.id.to_string();
    let run = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };

    let fatal = Envelope::Fatal {
        ref_id: Some(run_id),
        error: "out of memory".into(),
        error_code: None,
    };

    let mut buf = Vec::new();
    for env in &[&hello, &run, &fatal] {
        JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
    }
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 3);
    assert!(matches!(decoded[2], Envelope::Fatal { .. }));
}

// =========================================================================
// 16. EnvelopeValidator doc examples
// =========================================================================

#[test]
fn doc_validator_hello_valid() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("1.0".into()),
        },
        CapabilityManifest::new(),
    );
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn doc_validator_hello_empty_backend_id() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyField { field } if field == "backend.id"))
    );
}

#[test]
fn doc_validator_hello_invalid_version() {
    let hello = Envelope::Hello {
        contract_version: "invalid".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let v = EnvelopeValidator::new();
    let result = v.validate(&hello);
    assert!(!result.valid);
    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. }))
    );
}

#[test]
fn doc_validator_sequence_valid() {
    let wo = WorkOrderBuilder::new("task").build();
    let run_id = wo.id.to_string();

    let sequence = vec![
        Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        ),
        Envelope::Run {
            id: run_id.clone(),
            work_order: wo,
        },
        Envelope::Event {
            ref_id: run_id.clone(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "hi".into() },
                ext: None,
            },
        },
        Envelope::Final {
            ref_id: run_id,
            receipt: ReceiptBuilder::new("test").build(),
        },
    ];

    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&sequence);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn doc_validator_sequence_missing_hello() {
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

// =========================================================================
// 17. PolicyEngine doc examples
// =========================================================================

#[test]
fn doc_policy_engine_basic() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
}

#[test]
fn doc_policy_engine_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["Read".into(), "Grep".into()],
        disallowed_tools: vec!["Bash".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    assert!(engine.can_use_tool("Read").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed); // not in allowlist
}

#[test]
fn doc_policy_engine_empty_allows_everything() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
    assert!(engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn doc_policy_deny_read() {
    let policy = PolicyProfile {
        deny_read: vec!["**/.env".into(), "**/id_rsa".into()],
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

// =========================================================================
// 18. Decision doc examples
// =========================================================================

#[test]
fn doc_decision_allow() {
    let d = Decision::allow();
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn doc_decision_deny() {
    let d = Decision::deny("not permitted");
    assert!(!d.allowed);
    assert_eq!(d.reason.as_deref(), Some("not permitted"));
}

// =========================================================================
// 19. IncludeExcludeGlobs doc examples
// =========================================================================

#[test]
fn doc_glob_include_exclude() {
    let globs = IncludeExcludeGlobs::new(
        &["src/**".into(), "tests/**".into()],
        &["src/generated/**".into()],
    )
    .unwrap();
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
}

#[test]
fn doc_glob_empty_allows_all() {
    let open = IncludeExcludeGlobs::new(&[], &[]).unwrap();
    assert_eq!(open.decide_str("any/path.txt"), MatchDecision::Allowed);
}

#[test]
fn doc_glob_exclude_only() {
    let no_logs = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
    assert_eq!(
        no_logs.decide_str("app.log"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(no_logs.decide_str("src/main.rs"), MatchDecision::Allowed);
}

#[test]
fn doc_glob_invalid_pattern_error() {
    let err = IncludeExcludeGlobs::new(&["[".into()], &[]).unwrap_err();
    assert!(err.to_string().contains("invalid glob"));
}

#[test]
fn doc_glob_match_decision_is_allowed() {
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn doc_glob_decide_path_consistency() {
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["src/secret/**".into()]).unwrap();
    let cases = &["src/lib.rs", "src/secret/key.pem", "README.md"];
    for &c in cases {
        assert_eq!(globs.decide_str(c), globs.decide_path(Path::new(c)));
    }
}

#[test]
fn doc_build_globset_empty() {
    let result = abp_glob::build_globset(&[]).unwrap();
    assert!(result.is_none());
}

#[test]
fn doc_build_globset_with_patterns() {
    let result = abp_glob::build_globset(&["*.rs".into(), "src/**".into()]).unwrap();
    assert!(result.is_some());
    let set = result.unwrap();
    assert!(set.is_match("main.rs"));
}

// =========================================================================
// 20. Documented workflow: create work order → receipt → hash
// =========================================================================

#[test]
fn doc_workflow_end_to_end() {
    // Step 1: Create a work order
    let wo = WorkOrderBuilder::new("Implement feature X")
        .lane(ExecutionLane::PatchFirst)
        .root(".")
        .model("mock")
        .build();

    // Step 2: Build a receipt
    let receipt = ReceiptBuilder::new("mock")
        .work_order_id(wo.id)
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build()
        .with_hash()
        .unwrap();

    assert_eq!(receipt.meta.work_order_id, wo.id);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.trace.len(), 2);
}

// =========================================================================
// 21. Serde round-trip documentation patterns
// =========================================================================

#[test]
fn doc_work_order_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("test task")
        .model("gpt-4")
        .max_turns(5)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.config.model, wo2.config.model);
}

#[test]
fn doc_receipt_serde_roundtrip() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&receipt).unwrap();
    let receipt2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt.receipt_sha256, receipt2.receipt_sha256);
    assert_eq!(receipt.outcome, receipt2.outcome);
}

#[test]
fn doc_envelope_serde_roundtrip() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_string(&hello).unwrap();
    let hello2: Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(hello2, Envelope::Hello { .. }));
}

#[test]
fn doc_envelope_tag_is_t() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
    assert!(!json.contains(r#""type":"hello""#));
}

// =========================================================================
// 22. ProtocolError doc examples
// =========================================================================

#[test]
fn doc_protocol_error_json() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn doc_protocol_error_display() {
    let err = ProtocolError::Violation("test violation".into());
    assert!(err.to_string().contains("protocol violation"));
}

// =========================================================================
// 23. Documented BTreeMap usage for deterministic serialization
// =========================================================================

#[test]
fn doc_btreemap_deterministic_order() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::ToolWrite, SupportLevel::Emulated);

    let json1 = serde_json::to_string(&manifest).unwrap();
    let json2 = serde_json::to_string(&manifest).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn doc_runtime_config_vendor_btreemap() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("key1".into(), serde_json::json!("val1"));
    config
        .vendor
        .insert("key2".into(), serde_json::json!("val2"));
    let json1 = serde_json::to_string(&config).unwrap();
    let json2 = serde_json::to_string(&config).unwrap();
    assert_eq!(json1, json2);
}

// =========================================================================
// 24. Documented error handling patterns
// =========================================================================

#[test]
fn doc_contract_error_from_json() {
    // ContractError::Json is produced when serialization fails (hard to trigger
    // in practice, but verify the type exists and can be matched)
    let result = abp_core::canonical_json(&serde_json::json!(null));
    assert!(result.is_ok());
}

#[test]
fn doc_fatal_envelope_error_handling() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "backend crashed".into(),
        error_code: None,
    };
    if let Envelope::Fatal { ref_id, error, .. } = &fatal {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "backend crashed");
    }
}

// =========================================================================
// 25. Configuration patterns
// =========================================================================

#[test]
fn doc_runtime_config_defaults() {
    let config = RuntimeConfig::default();
    assert!(config.model.is_none());
    assert!(config.vendor.is_empty());
    assert!(config.env.is_empty());
    assert!(config.max_budget_usd.is_none());
    assert!(config.max_turns.is_none());
}

#[test]
fn doc_runtime_config_env_vars() {
    let mut config = RuntimeConfig::default();
    config.env.insert("OPENAI_API_KEY".into(), "sk-xxx".into());
    assert_eq!(config.env.get("OPENAI_API_KEY").unwrap(), "sk-xxx");
}

#[test]
fn doc_policy_profile_defaults() {
    let policy = PolicyProfile::default();
    assert!(policy.allowed_tools.is_empty());
    assert!(policy.disallowed_tools.is_empty());
    assert!(policy.deny_read.is_empty());
    assert!(policy.deny_write.is_empty());
    assert!(policy.allow_network.is_empty());
    assert!(policy.deny_network.is_empty());
    assert!(policy.require_approval_for.is_empty());
}

#[test]
fn doc_workspace_spec_serde() {
    let spec = WorkspaceSpec {
        root: "/tmp/ws".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let spec2: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(spec.root, spec2.root);
}

#[test]
fn doc_execution_lane_serde() {
    let patch: ExecutionLane = serde_json::from_str(r#""patch_first""#).unwrap();
    assert!(matches!(patch, ExecutionLane::PatchFirst));

    let ws: ExecutionLane = serde_json::from_str(r#""workspace_first""#).unwrap();
    assert!(matches!(ws, ExecutionLane::WorkspaceFirst));
}

// =========================================================================
// 26. Type documentation accuracy
// =========================================================================

#[test]
fn doc_context_packet_default_empty() {
    let ctx = ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

#[test]
fn doc_usage_normalized_default() {
    let usage = UsageNormalized::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.cache_read_tokens.is_none());
    assert!(usage.cache_write_tokens.is_none());
    assert!(usage.request_units.is_none());
    assert!(usage.estimated_cost_usd.is_none());
}

#[test]
fn doc_verification_report_default() {
    let v = VerificationReport::default();
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
    assert!(!v.harness_ok);
}

#[test]
fn doc_receipt_meta_has_contract_version() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn doc_capability_requirements_default_empty() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

// =========================================================================
// 27. Agent event extension field (passthrough mode)
// =========================================================================

#[test]
fn doc_agent_event_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"original": true}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("raw_message"));

    // ext is skipped when None
    let event2 = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json2 = serde_json::to_string(&event2).unwrap();
    assert!(!json2.contains("raw_message"));
}
