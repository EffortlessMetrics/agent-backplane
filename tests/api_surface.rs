// SPDX-License-Identifier: MIT OR Apache-2.0
//! API surface tests — compile-time guarantees that public items remain exported.
//!
//! If someone accidentally removes a `pub` item, these tests will fail to compile.

// ---------------------------------------------------------------------------
// abp-core: public types
// ---------------------------------------------------------------------------

#[test]
fn core_work_order_accessible() {
    let _wo = abp_core::WorkOrderBuilder::new("test task").build();
    let _id = _wo.id;
    let _task: &str = &_wo.task;
    let _lane: &abp_core::ExecutionLane = &_wo.lane;
    let _ws: &abp_core::WorkspaceSpec = &_wo.workspace;
    let _ctx: &abp_core::ContextPacket = &_wo.context;
    let _pol: &abp_core::PolicyProfile = &_wo.policy;
    let _reqs: &abp_core::CapabilityRequirements = &_wo.requirements;
    let _cfg: &abp_core::RuntimeConfig = &_wo.config;
}

#[test]
fn core_receipt_and_builder_accessible() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let _meta: &abp_core::RunMetadata = &receipt.meta;
    let _backend: &abp_core::BackendIdentity = &receipt.backend;
    let _caps: &abp_core::CapabilityManifest = &receipt.capabilities;
    let _mode: &abp_core::ExecutionMode = &receipt.mode;
    let _usage_raw: &serde_json::Value = &receipt.usage_raw;
    let _usage: &abp_core::UsageNormalized = &receipt.usage;
    let _trace: &Vec<abp_core::AgentEvent> = &receipt.trace;
    let _arts: &Vec<abp_core::ArtifactRef> = &receipt.artifacts;
    let _ver: &abp_core::VerificationReport = &receipt.verification;
    let _out: &abp_core::Outcome = &receipt.outcome;
    let _hash: &Option<String> = &receipt.receipt_sha256;
}

#[test]
fn core_agent_event_kind_all_variants() {
    use abp_core::AgentEventKind;
    let _variants: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: String::new(),
        },
        AgentEventKind::RunCompleted {
            message: String::new(),
        },
        AgentEventKind::AssistantDelta {
            text: String::new(),
        },
        AgentEventKind::AssistantMessage {
            text: String::new(),
        },
        AgentEventKind::ToolCall {
            tool_name: String::new(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::Value::Null,
        },
        AgentEventKind::ToolResult {
            tool_name: String::new(),
            tool_use_id: None,
            output: serde_json::Value::Null,
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: String::new(),
            summary: String::new(),
        },
        AgentEventKind::CommandExecuted {
            command: String::new(),
            exit_code: None,
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: String::new(),
        },
        AgentEventKind::Error {
            message: String::new(),
        },
    ];
    assert_eq!(_variants.len(), 10);
}

#[test]
fn core_capability_all_variants() {
    use abp_core::Capability;
    let _variants: Vec<Capability> = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
    ];
    assert_eq!(_variants.len(), 18);
}

#[test]
fn core_execution_lane_variants() {
    let _patch = abp_core::ExecutionLane::PatchFirst;
    let _ws = abp_core::ExecutionLane::WorkspaceFirst;
}

#[test]
fn core_outcome_variants() {
    let _c = abp_core::Outcome::Complete;
    let _p = abp_core::Outcome::Partial;
    let _f = abp_core::Outcome::Failed;
}

#[test]
fn core_execution_mode_variants() {
    let _pt = abp_core::ExecutionMode::Passthrough;
    let _m = abp_core::ExecutionMode::Mapped;
    assert_eq!(
        abp_core::ExecutionMode::default(),
        abp_core::ExecutionMode::Mapped
    );
}

#[test]
fn core_workspace_mode_variants() {
    let _pt = abp_core::WorkspaceMode::PassThrough;
    let _st = abp_core::WorkspaceMode::Staged;
}

#[test]
fn core_capability_manifest_type() {
    use std::collections::BTreeMap;
    // CapabilityManifest is a type alias for BTreeMap<Capability, SupportLevel>
    let mut manifest: abp_core::CapabilityManifest = BTreeMap::new();
    manifest.insert(
        abp_core::Capability::Streaming,
        abp_core::SupportLevel::Native,
    );
    assert!(manifest.contains_key(&abp_core::Capability::Streaming));
}

#[test]
fn core_capability_requirement_accessible() {
    let _req = abp_core::CapabilityRequirement {
        capability: abp_core::Capability::ToolRead,
        min_support: abp_core::MinSupport::Native,
    };
}

#[test]
fn core_work_order_builder_fluent_api() {
    let wo = abp_core::WorkOrderBuilder::new("task")
        .lane(abp_core::ExecutionLane::WorkspaceFirst)
        .root("/tmp")
        .workspace_mode(abp_core::WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .model("gpt-4")
        .max_budget_usd(1.0)
        .max_turns(5)
        .build();
    assert_eq!(wo.task, "task");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn core_receipt_builder_fluent_api() {
    let receipt = abp_core::ReceiptBuilder::new("test-backend")
        .outcome(abp_core::Outcome::Complete)
        .mode(abp_core::ExecutionMode::Passthrough)
        .backend_version("1.0")
        .adapter_version("0.5")
        .build();
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.mode, abp_core::ExecutionMode::Passthrough);
}

#[test]
fn core_contract_version_constant() {
    assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn core_free_functions_accessible() {
    // canonical_json
    let json = abp_core::canonical_json(&"hello").unwrap();
    assert!(!json.is_empty());

    // sha256_hex
    let hash = abp_core::sha256_hex(b"hello");
    assert_eq!(hash.len(), 64);

    // receipt_hash
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let h = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn core_receipt_with_hash() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

// ---------------------------------------------------------------------------
// abp-core: module exports
// ---------------------------------------------------------------------------

#[test]
fn core_module_validate_accessible() {
    use abp_core::validate::{ValidationError, validate_receipt};
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(validate_receipt(&receipt).is_ok());
    let _e = ValidationError::EmptyBackendId;
}

#[test]
fn core_module_filter_accessible() {
    use abp_core::filter::EventFilter;
    let _f = EventFilter::include_kinds(&["run_started"]);
    let _f2 = EventFilter::exclude_kinds(&["error"]);
}

#[test]
fn core_module_stream_accessible() {
    use abp_core::stream::EventStream;
    let stream = EventStream::new(vec![]);
    assert!(stream.is_empty());
    assert_eq!(stream.len(), 0);
}

#[test]
fn core_module_chain_accessible() {
    use abp_core::chain::{ChainError, ReceiptChain};
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    let _e = ChainError::EmptyChain;
}

#[test]
fn core_module_error_accessible() {
    use abp_core::error::{ErrorCatalog, ErrorCode, ErrorInfo};
    let code = ErrorCode::InvalidContractVersion;
    assert_eq!(code.code(), "ABP-C001");
    assert_eq!(code.category(), "contract");
    let _info = ErrorInfo::new(code, "test");
    let _all = ErrorCatalog::all();
    assert!(!_all.is_empty());
}

#[test]
fn core_module_ext_accessible() {
    use abp_core::ext::{AgentEventExt, ReceiptExt, WorkOrderExt};
    let wo = abp_core::WorkOrderBuilder::new("fix code").build();
    assert!(wo.is_code_task());
    let _summary = wo.task_summary(10);

    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    assert!(receipt.is_success());
    assert!(!receipt.is_failure());
    assert_eq!(receipt.total_tool_calls(), 0);

    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    assert!(!event.is_tool_call());
    assert_eq!(event.text_content(), Some("hi"));
}

// ---------------------------------------------------------------------------
// abp-protocol: public types
// ---------------------------------------------------------------------------

#[test]
fn protocol_envelope_all_variants() {
    use abp_protocol::Envelope;

    let _hello = Envelope::hello(
        abp_core::BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    // Run, Event, Final, Fatal are struct variants accessible via construction
    let _fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
    };
}

#[test]
fn protocol_jsonl_codec_accessible() {
    use abp_protocol::{Envelope, JsonlCodec};
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "err".into(),
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Fatal { .. }));
}

#[test]
fn protocol_streaming_codec_accessible() {
    use abp_protocol::Envelope;
    use abp_protocol::codec::StreamingCodec;
    let envs = vec![
        Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
        },
        Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
        },
    ];
    let batch = StreamingCodec::encode_batch(&envs);
    assert_eq!(StreamingCodec::line_count(&batch), 2);
    let results = StreamingCodec::decode_batch(&batch);
    assert_eq!(results.len(), 2);
}

#[test]
fn protocol_version_type_accessible() {
    use abp_protocol::version::ProtocolVersion;
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
    let current = ProtocolVersion::current();
    assert!(current.is_compatible(&v));
}

#[test]
fn protocol_envelope_builder_accessible() {
    use abp_protocol::builder::EnvelopeBuilder;
    let env = EnvelopeBuilder::hello()
        .backend("test")
        .version("1.0")
        .build()
        .unwrap();
    assert!(matches!(env, abp_protocol::Envelope::Hello { .. }));

    let _fatal = EnvelopeBuilder::fatal("boom").build().unwrap();
    assert!(matches!(_fatal, abp_protocol::Envelope::Fatal { .. }));
}

#[test]
fn protocol_free_functions_accessible() {
    let parsed = abp_protocol::parse_version("abp/v0.1");
    assert_eq!(parsed, Some((0, 1)));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn protocol_error_type_accessible() {
    let _err = abp_protocol::ProtocolError::Violation("test".into());
}

// ---------------------------------------------------------------------------
// abp-glob: public types
// ---------------------------------------------------------------------------

#[test]
fn glob_include_exclude_accessible() {
    use abp_glob::{IncludeExcludeGlobs, MatchDecision};
    let globs = IncludeExcludeGlobs::new(&["src/**".into()], &["src/generated/**".into()]).unwrap();
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(
        globs.decide_str("src/generated/out.rs"),
        MatchDecision::DeniedByExclude
    );
    assert_eq!(
        globs.decide_str("README.md"),
        MatchDecision::DeniedByMissingInclude
    );
    assert!(MatchDecision::Allowed.is_allowed());
}

#[test]
fn glob_build_globset_accessible() {
    let result = abp_glob::build_globset(&[]).unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// abp-policy: public types
// ---------------------------------------------------------------------------

#[test]
fn policy_engine_accessible() {
    use abp_policy::PolicyEngine;
    let policy = abp_core::PolicyProfile::default();
    let engine = PolicyEngine::new(&policy).unwrap();
    let d = engine.can_use_tool("Bash");
    assert!(d.allowed);
}

#[test]
fn policy_decision_type_accessible() {
    use abp_policy::Decision;
    let allow = Decision::allow();
    assert!(allow.allowed);
    assert!(allow.reason.is_none());
    let deny = Decision::deny("nope");
    assert!(!deny.allowed);
    assert_eq!(deny.reason.as_deref(), Some("nope"));
}

#[test]
fn policy_auditor_and_decision_accessible() {
    use abp_policy::audit::{PolicyAuditor, PolicyDecision};
    let engine = abp_policy::PolicyEngine::new(&abp_core::PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    })
    .unwrap();
    let mut auditor = PolicyAuditor::new(engine);
    let decision = auditor.check_tool("Bash");
    assert!(matches!(decision, PolicyDecision::Deny { .. }));
    let decision2 = auditor.check_tool("Read");
    assert!(matches!(decision2, PolicyDecision::Allow));
    assert_eq!(auditor.denied_count(), 1);
    assert_eq!(auditor.allowed_count(), 1);
    let summary = auditor.summary();
    assert_eq!(summary.denied, 1);
    assert_eq!(summary.allowed, 1);
}

// ---------------------------------------------------------------------------
// Trait implementations: Clone, Debug, Serialize, Deserialize, PartialEq, Eq
// ---------------------------------------------------------------------------

fn assert_clone<T: Clone>() {}
fn assert_debug<T: std::fmt::Debug>() {}
fn assert_serialize<T: serde::Serialize>() {}
fn assert_deserialize<T: serde::de::DeserializeOwned>() {}
fn assert_partial_eq<T: PartialEq>() {}
fn assert_eq_trait<T: Eq>() {}

#[test]
fn core_types_implement_clone_debug_serde() {
    // Clone + Debug + Serialize + Deserialize
    assert_clone::<abp_core::WorkOrder>();
    assert_clone::<abp_core::Receipt>();
    assert_clone::<abp_core::AgentEvent>();
    assert_clone::<abp_core::AgentEventKind>();
    assert_clone::<abp_core::Capability>();
    assert_clone::<abp_core::ExecutionLane>();
    assert_clone::<abp_core::ExecutionMode>();
    assert_clone::<abp_core::WorkspaceMode>();
    assert_clone::<abp_core::Outcome>();

    assert_debug::<abp_core::WorkOrder>();
    assert_debug::<abp_core::Receipt>();
    assert_debug::<abp_core::AgentEvent>();
    assert_debug::<abp_core::WorkOrderBuilder>();
    assert_debug::<abp_core::ReceiptBuilder>();

    assert_serialize::<abp_core::WorkOrder>();
    assert_serialize::<abp_core::Receipt>();
    assert_serialize::<abp_core::AgentEvent>();
    assert_serialize::<abp_core::Capability>();
    assert_serialize::<abp_core::ExecutionMode>();

    assert_deserialize::<abp_core::WorkOrder>();
    assert_deserialize::<abp_core::Receipt>();
    assert_deserialize::<abp_core::AgentEvent>();
    assert_deserialize::<abp_core::Capability>();
    assert_deserialize::<abp_core::ExecutionMode>();
}

#[test]
fn core_types_implement_partial_eq_eq() {
    assert_partial_eq::<abp_core::Outcome>();
    assert_eq_trait::<abp_core::Outcome>();
    assert_partial_eq::<abp_core::ExecutionMode>();
    assert_eq_trait::<abp_core::ExecutionMode>();
    assert_partial_eq::<abp_core::Capability>();
    assert_eq_trait::<abp_core::Capability>();
}

#[test]
fn protocol_types_implement_clone_debug_serde() {
    assert_clone::<abp_protocol::Envelope>();
    assert_debug::<abp_protocol::Envelope>();
    assert_serialize::<abp_protocol::Envelope>();
    assert_deserialize::<abp_protocol::Envelope>();

    assert_clone::<abp_protocol::version::ProtocolVersion>();
    assert_debug::<abp_protocol::version::ProtocolVersion>();
    assert_serialize::<abp_protocol::version::ProtocolVersion>();
    assert_deserialize::<abp_protocol::version::ProtocolVersion>();
    assert_partial_eq::<abp_protocol::version::ProtocolVersion>();
    assert_eq_trait::<abp_protocol::version::ProtocolVersion>();
}

#[test]
fn glob_types_implement_clone_debug() {
    assert_clone::<abp_glob::IncludeExcludeGlobs>();
    assert_debug::<abp_glob::IncludeExcludeGlobs>();
    assert_clone::<abp_glob::MatchDecision>();
    assert_debug::<abp_glob::MatchDecision>();
    assert_partial_eq::<abp_glob::MatchDecision>();
    assert_eq_trait::<abp_glob::MatchDecision>();
}

#[test]
fn policy_types_implement_clone_debug() {
    assert_clone::<abp_policy::PolicyEngine>();
    assert_debug::<abp_policy::PolicyEngine>();
    assert_clone::<abp_policy::Decision>();
    assert_debug::<abp_policy::Decision>();
    assert_serialize::<abp_policy::Decision>();
    assert_deserialize::<abp_policy::Decision>();

    assert_clone::<abp_policy::audit::PolicyDecision>();
    assert_debug::<abp_policy::audit::PolicyDecision>();
    assert_serialize::<abp_policy::audit::PolicyDecision>();
    assert_deserialize::<abp_policy::audit::PolicyDecision>();
    assert_partial_eq::<abp_policy::audit::PolicyDecision>();
    assert_eq_trait::<abp_policy::audit::PolicyDecision>();
}

// ---------------------------------------------------------------------------
// Send + Sync: key types are safe for async usage
// ---------------------------------------------------------------------------

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn core_types_are_send_sync() {
    assert_send::<abp_core::WorkOrder>();
    assert_sync::<abp_core::WorkOrder>();
    assert_send::<abp_core::Receipt>();
    assert_sync::<abp_core::Receipt>();
    assert_send::<abp_core::AgentEvent>();
    assert_sync::<abp_core::AgentEvent>();
    assert_send::<abp_core::AgentEventKind>();
    assert_sync::<abp_core::AgentEventKind>();
    assert_send::<abp_core::Capability>();
    assert_sync::<abp_core::Capability>();
    assert_send::<abp_core::ExecutionMode>();
    assert_sync::<abp_core::ExecutionMode>();
    assert_send::<abp_core::Outcome>();
    assert_sync::<abp_core::Outcome>();
}

#[test]
fn protocol_types_are_send_sync() {
    assert_send::<abp_protocol::Envelope>();
    assert_sync::<abp_protocol::Envelope>();
    assert_send::<abp_protocol::version::ProtocolVersion>();
    assert_sync::<abp_protocol::version::ProtocolVersion>();
}

#[test]
fn policy_types_are_send_sync() {
    assert_send::<abp_policy::PolicyEngine>();
    assert_sync::<abp_policy::PolicyEngine>();
    assert_send::<abp_policy::Decision>();
    assert_sync::<abp_policy::Decision>();
    assert_send::<abp_policy::audit::PolicyDecision>();
    assert_sync::<abp_policy::audit::PolicyDecision>();
}
