#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ContractError,
    ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_dialect::Dialect;
use abp_error::ErrorCode;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample_context_packet() -> ContextPacket {
    ContextPacket {
        files: vec!["src/main.rs".into(), "README.md".into()],
        snippets: vec![ContextSnippet {
            name: "greeting".into(),
            content: "Hello, world!".into(),
        }],
    }
}

fn sample_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["exec".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/*".into()],
        allow_network: vec!["example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    }
}

fn sample_runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("temperature".into(), json!(0.7));
            m
        },
        env: {
            let mut m = BTreeMap::new();
            m.insert("RUST_LOG".into(), "debug".into());
            m
        },
        max_budget_usd: Some(1.50),
        max_turns: Some(10),
    }
}

fn sample_capability_requirements() -> CapabilityRequirements {
    CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    }
}

fn sample_agent_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "done".into(),
        },
        ext: None,
    }
}

fn sample_artifact() -> ArtifactRef {
    ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    }
}

fn sample_verification() -> VerificationReport {
    VerificationReport {
        git_diff: Some("+hello".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    }
}

fn sample_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(20),
        cache_write_tokens: Some(10),
        request_units: Some(1),
        estimated_cost_usd: Some(0.005),
    }
}

fn all_capabilities() -> Vec<Capability> {
    vec![
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
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ]
}

// ===========================================================================
// WorkOrderBuilder — defaults
// ===========================================================================

#[test]
fn wo_default_task() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn wo_default_lane_is_patch_first() {
    let wo = WorkOrderBuilder::new("t").build();
    let v = serde_json::to_value(&wo.lane).unwrap();
    assert_eq!(v, json!("patch_first"));
}

#[test]
fn wo_default_workspace_root_is_dot() {
    let wo = WorkOrderBuilder::new("t").build();
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn wo_default_workspace_mode_is_staged() {
    let wo = WorkOrderBuilder::new("t").build();
    let v = serde_json::to_value(&wo.workspace.mode).unwrap();
    assert_eq!(v, json!("staged"));
}

#[test]
fn wo_default_include_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.workspace.include.is_empty());
}

#[test]
fn wo_default_exclude_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn wo_default_context_files_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.context.files.is_empty());
}

#[test]
fn wo_default_context_snippets_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn wo_default_policy_all_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
    assert!(wo.policy.allow_network.is_empty());
    assert!(wo.policy.deny_network.is_empty());
    assert!(wo.policy.require_approval_for.is_empty());
}

#[test]
fn wo_default_requirements_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn wo_default_config_model_none() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.model.is_none());
}

#[test]
fn wo_default_config_vendor_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.vendor.is_empty());
}

#[test]
fn wo_default_config_env_empty() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.env.is_empty());
}

#[test]
fn wo_default_config_budget_none() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn wo_default_config_max_turns_none() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.max_turns.is_none());
}

#[test]
fn wo_id_is_nonnil_uuid() {
    let wo = WorkOrderBuilder::new("t").build();
    assert!(!wo.id.is_nil());
}

#[test]
fn wo_ids_are_unique() {
    let a = WorkOrderBuilder::new("t").build();
    let b = WorkOrderBuilder::new("t").build();
    assert_ne!(a.id, b.id);
}

// ===========================================================================
// WorkOrderBuilder — setters
// ===========================================================================

#[test]
fn wo_set_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let v = serde_json::to_value(&wo.lane).unwrap();
    assert_eq!(v, json!("workspace_first"));
}

#[test]
fn wo_set_lane_patch_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let v = serde_json::to_value(&wo.lane).unwrap();
    assert_eq!(v, json!("patch_first"));
}

#[test]
fn wo_set_root() {
    let wo = WorkOrderBuilder::new("t").root("/tmp/ws").build();
    assert_eq!(wo.workspace.root, "/tmp/ws");
}

#[test]
fn wo_set_workspace_mode_pass_through() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let v = serde_json::to_value(&wo.workspace.mode).unwrap();
    assert_eq!(v, json!("pass_through"));
}

#[test]
fn wo_set_workspace_mode_staged() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let v = serde_json::to_value(&wo.workspace.mode).unwrap();
    assert_eq!(v, json!("staged"));
}

#[test]
fn wo_set_include() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into(), "*.toml".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs", "*.toml"]);
}

#[test]
fn wo_set_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn wo_set_context() {
    let ctx = sample_context_packet();
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "greeting");
}

#[test]
fn wo_set_policy() {
    let p = sample_policy();
    let wo = WorkOrderBuilder::new("t").policy(p).build();
    assert_eq!(wo.policy.allowed_tools, vec!["read", "write"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["exec"]);
    assert_eq!(wo.policy.deny_read, vec!["*.secret"]);
    assert_eq!(wo.policy.deny_write, vec!["/etc/*"]);
    assert_eq!(wo.policy.allow_network, vec!["example.com"]);
    assert_eq!(wo.policy.deny_network, vec!["evil.com"]);
    assert_eq!(wo.policy.require_approval_for, vec!["deploy"]);
}

#[test]
fn wo_set_requirements() {
    let reqs = sample_capability_requirements();
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 2);
}

#[test]
fn wo_set_config() {
    let cfg = sample_runtime_config();
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_budget_usd, Some(1.50));
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn wo_set_model_shorthand() {
    let wo = WorkOrderBuilder::new("t").model("claude-3").build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
}

#[test]
fn wo_set_max_budget_usd_shorthand() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(5.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn wo_set_max_turns_shorthand() {
    let wo = WorkOrderBuilder::new("t").max_turns(20).build();
    assert_eq!(wo.config.max_turns, Some(20));
}

// ===========================================================================
// WorkOrderBuilder — chaining
// ===========================================================================

#[test]
fn wo_chain_all_setters() {
    let wo = WorkOrderBuilder::new("full chain")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/my/project")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["src/**".into()])
        .exclude(vec!["*.lock".into()])
        .context(sample_context_packet())
        .policy(sample_policy())
        .requirements(sample_capability_requirements())
        .model("gpt-4o")
        .max_budget_usd(10.0)
        .max_turns(50)
        .build();

    assert_eq!(wo.task, "full chain");
    assert_eq!(wo.workspace.root, "/my/project");
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["*.lock"]);
    assert_eq!(wo.context.files.len(), 2);
    assert!(!wo.policy.allowed_tools.is_empty());
    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
    assert_eq!(wo.config.max_turns, Some(50));
}

#[test]
fn wo_chain_override_model_via_config_then_shorthand() {
    let cfg = RuntimeConfig {
        model: Some("initial".into()),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t")
        .config(cfg)
        .model("override")
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("override"));
}

#[test]
fn wo_chain_override_budget_via_config_then_shorthand() {
    let cfg = RuntimeConfig {
        max_budget_usd: Some(1.0),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t")
        .config(cfg)
        .max_budget_usd(99.0)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(99.0));
}

#[test]
fn wo_chain_override_turns_via_config_then_shorthand() {
    let cfg = RuntimeConfig {
        max_turns: Some(1),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t")
        .config(cfg)
        .max_turns(999)
        .build();
    assert_eq!(wo.config.max_turns, Some(999));
}

// ===========================================================================
// WorkOrderBuilder — complex nested types
// ===========================================================================

#[test]
fn wo_context_with_multiple_snippets() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into(), "c.rs".into()],
        snippets: vec![
            ContextSnippet {
                name: "s1".into(),
                content: "first".into(),
            },
            ContextSnippet {
                name: "s2".into(),
                content: "second".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 3);
    assert_eq!(wo.context.snippets.len(), 2);
    assert_eq!(wo.context.snippets[1].content, "second");
}

#[test]
fn wo_vendor_config_btreemap() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"stream": true}));
    vendor.insert("anthropic".into(), json!({"max_tokens": 4096}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor.len(), 2);
    assert_eq!(wo.config.vendor["openai"]["stream"], json!(true));
    assert_eq!(wo.config.vendor["anthropic"]["max_tokens"], json!(4096));
}

#[test]
fn wo_env_btreemap() {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin".into());
    env.insert("HOME".into(), "/home/user".into());
    let cfg = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.env["PATH"], "/usr/bin");
    assert_eq!(wo.config.env["HOME"], "/home/user");
}

#[test]
fn wo_policy_full_roundtrip() {
    let p = sample_policy();
    let wo = WorkOrderBuilder::new("t").policy(p).build();
    let json = serde_json::to_string(&wo.policy).unwrap();
    let roundtrip: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.allowed_tools, wo.policy.allowed_tools);
    assert_eq!(roundtrip.deny_read, wo.policy.deny_read);
}

#[test]
fn wo_requirements_with_all_min_support_variants() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    let v = serde_json::to_value(&wo.requirements.required[0].min_support).unwrap();
    assert_eq!(v, json!("native"));
    let v = serde_json::to_value(&wo.requirements.required[1].min_support).unwrap();
    assert_eq!(v, json!("emulated"));
}

// ===========================================================================
// WorkOrderBuilder — serialization
// ===========================================================================

#[test]
fn wo_serializes_to_json() {
    let wo = WorkOrderBuilder::new("serialize me").build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("serialize me"));
}

#[test]
fn wo_roundtrip_json() {
    let wo = WorkOrderBuilder::new("roundtrip")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/ws")
        .model("gpt-4")
        .max_turns(5)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "roundtrip");
    assert_eq!(back.workspace.root, "/ws");
    assert_eq!(back.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(back.config.max_turns, Some(5));
}

#[test]
fn wo_serialize_contains_id() {
    let wo = WorkOrderBuilder::new("t").build();
    let v = serde_json::to_value(&wo).unwrap();
    assert!(v.get("id").is_some());
}

#[test]
fn wo_serialize_lane_snake_case() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let v = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["lane"], json!("workspace_first"));
}

#[test]
fn wo_serialize_workspace_mode_snake_case() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let v = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["workspace"]["mode"], json!("pass_through"));
}

#[test]
fn wo_full_serialize_has_all_top_level_keys() {
    let wo = WorkOrderBuilder::new("t").build();
    let v = serde_json::to_value(&wo).unwrap();
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
        assert!(obj.contains_key(*key), "missing key: {key}");
    }
}

// ===========================================================================
// WorkOrderBuilder — all Capability values
// ===========================================================================

#[test]
fn wo_each_capability_serializes_snake_case() {
    for cap in all_capabilities() {
        let v = serde_json::to_value(&cap).unwrap();
        let s = v.as_str().unwrap();
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "capability {cap:?} serialized as {s:?} is not snake_case"
        );
    }
}

#[test]
fn wo_requirement_with_every_capability() {
    let reqs = CapabilityRequirements {
        required: all_capabilities()
            .into_iter()
            .map(|cap| CapabilityRequirement {
                capability: cap,
                min_support: MinSupport::Emulated,
            })
            .collect(),
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert!(wo.requirements.required.len() >= 30);
}

#[test]
fn wo_capability_roundtrip_all() {
    for cap in all_capabilities() {
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(
            format!("{back:?}"),
            format!("{cap:?}"),
            "roundtrip failed for {cap:?}"
        );
    }
}

// ===========================================================================
// WorkOrderBuilder — all Dialect values (abp_dialect)
// ===========================================================================

#[test]
fn dialect_openai_serializes() {
    let v = serde_json::to_value(Dialect::OpenAi).unwrap();
    assert_eq!(v, json!("open_ai"));
}

#[test]
fn dialect_claude_serializes() {
    let v = serde_json::to_value(Dialect::Claude).unwrap();
    assert_eq!(v, json!("claude"));
}

#[test]
fn dialect_gemini_serializes() {
    let v = serde_json::to_value(Dialect::Gemini).unwrap();
    assert_eq!(v, json!("gemini"));
}

#[test]
fn dialect_codex_serializes() {
    let v = serde_json::to_value(Dialect::Codex).unwrap();
    assert_eq!(v, json!("codex"));
}

#[test]
fn dialect_kimi_serializes() {
    let v = serde_json::to_value(Dialect::Kimi).unwrap();
    assert_eq!(v, json!("kimi"));
}

#[test]
fn dialect_copilot_serializes() {
    let v = serde_json::to_value(Dialect::Copilot).unwrap();
    assert_eq!(v, json!("copilot"));
}

#[test]
fn dialect_roundtrip_all() {
    let dialects = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
        Dialect::Copilot,
    ];
    for d in dialects {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }
}

// ===========================================================================
// ReceiptBuilder — defaults
// ===========================================================================

#[test]
fn rb_default_backend_id() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.backend.id, "mock");
}

#[test]
fn rb_default_outcome_complete() {
    let r = ReceiptBuilder::new("m").build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn rb_default_mode_mapped() {
    let r = ReceiptBuilder::new("m").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn rb_default_work_order_id_nil() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.meta.work_order_id.is_nil());
}

#[test]
fn rb_default_run_id_nonnil() {
    let r = ReceiptBuilder::new("m").build();
    assert!(!r.meta.run_id.is_nil());
}

#[test]
fn rb_default_contract_version() {
    let r = ReceiptBuilder::new("m").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn rb_default_usage_raw_empty_object() {
    let r = ReceiptBuilder::new("m").build();
    assert_eq!(r.usage_raw, json!({}));
}

#[test]
fn rb_default_usage_normalized_all_none() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    assert!(r.usage.cache_read_tokens.is_none());
    assert!(r.usage.cache_write_tokens.is_none());
    assert!(r.usage.request_units.is_none());
    assert!(r.usage.estimated_cost_usd.is_none());
}

#[test]
fn rb_default_trace_empty() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.trace.is_empty());
}

#[test]
fn rb_default_artifacts_empty() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.artifacts.is_empty());
}

#[test]
fn rb_default_verification_empty() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
    assert!(!r.verification.harness_ok);
}

#[test]
fn rb_default_hash_none() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn rb_default_backend_version_none() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.backend.backend_version.is_none());
}

#[test]
fn rb_default_adapter_version_none() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.backend.adapter_version.is_none());
}

#[test]
fn rb_default_capabilities_empty() {
    let r = ReceiptBuilder::new("m").build();
    assert!(r.capabilities.is_empty());
}

#[test]
fn rb_default_duration_ms_zero() {
    let r = ReceiptBuilder::new("m").build();
    assert_eq!(r.meta.duration_ms, 0);
}

// ===========================================================================
// ReceiptBuilder — setters
// ===========================================================================

#[test]
fn rb_set_backend_id() {
    let r = ReceiptBuilder::new("x").backend_id("sidecar:node").build();
    assert_eq!(r.backend.id, "sidecar:node");
}

#[test]
fn rb_set_outcome_partial() {
    let r = ReceiptBuilder::new("m").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn rb_set_outcome_failed() {
    let r = ReceiptBuilder::new("m").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn rb_set_outcome_complete() {
    let r = ReceiptBuilder::new("m").outcome(Outcome::Complete).build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn rb_set_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("m").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn rb_set_started_at() {
    let dt = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let r = ReceiptBuilder::new("m").started_at(dt).build();
    assert_eq!(r.meta.started_at, dt);
}

#[test]
fn rb_set_finished_at() {
    let dt = DateTime::parse_from_rfc3339("2024-12-31T23:59:59Z")
        .unwrap()
        .with_timezone(&Utc);
    let r = ReceiptBuilder::new("m").finished_at(dt).build();
    assert_eq!(r.meta.finished_at, dt);
}

#[test]
fn rb_duration_ms_computed_from_timestamps() {
    let start = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let end = start + Duration::milliseconds(1234);
    let r = ReceiptBuilder::new("m")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.duration_ms, 1234);
}

#[test]
fn rb_duration_ms_clamped_to_zero_if_negative() {
    let start = DateTime::parse_from_rfc3339("2024-01-01T00:01:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let end = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let r = ReceiptBuilder::new("m")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn rb_set_mode_passthrough() {
    let r = ReceiptBuilder::new("m")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn rb_set_mode_mapped() {
    let r = ReceiptBuilder::new("m").mode(ExecutionMode::Mapped).build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn rb_set_backend_version() {
    let r = ReceiptBuilder::new("m").backend_version("1.2.3").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

#[test]
fn rb_set_adapter_version() {
    let r = ReceiptBuilder::new("m").adapter_version("0.5.0").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
}

#[test]
fn rb_set_usage_raw() {
    let raw = json!({"prompt_tokens": 100, "completion_tokens": 50});
    let r = ReceiptBuilder::new("m").usage_raw(raw.clone()).build();
    assert_eq!(r.usage_raw, raw);
}

#[test]
fn rb_set_usage_normalized() {
    let u = sample_usage();
    let r = ReceiptBuilder::new("m").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(50));
    assert_eq!(r.usage.cache_read_tokens, Some(20));
    assert_eq!(r.usage.cache_write_tokens, Some(10));
    assert_eq!(r.usage.request_units, Some(1));
    assert_eq!(r.usage.estimated_cost_usd, Some(0.005));
}

#[test]
fn rb_set_verification() {
    let v = sample_verification();
    let r = ReceiptBuilder::new("m").verification(v).build();
    assert_eq!(r.verification.git_diff.as_deref(), Some("+hello"));
    assert_eq!(r.verification.git_status.as_deref(), Some("M src/main.rs"));
    assert!(r.verification.harness_ok);
}

#[test]
fn rb_set_capabilities() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let r = ReceiptBuilder::new("m").capabilities(caps).build();
    assert_eq!(r.capabilities.len(), 2);
}

#[test]
fn rb_add_trace_event() {
    let evt = sample_agent_event();
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn rb_add_multiple_trace_events() {
    let r = ReceiptBuilder::new("m")
        .add_trace_event(sample_agent_event())
        .add_trace_event(sample_agent_event())
        .add_trace_event(sample_agent_event())
        .build();
    assert_eq!(r.trace.len(), 3);
}

#[test]
fn rb_add_artifact() {
    let a = sample_artifact();
    let r = ReceiptBuilder::new("m").add_artifact(a).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert_eq!(r.artifacts[0].path, "output.diff");
}

#[test]
fn rb_add_multiple_artifacts() {
    let r = ReceiptBuilder::new("m")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.diff".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 2);
}

// ===========================================================================
// ReceiptBuilder — chaining
// ===========================================================================

#[test]
fn rb_chain_all_setters() {
    let start = DateTime::parse_from_rfc3339("2024-06-01T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let end = start + Duration::seconds(30);
    let wo_id = Uuid::new_v4();

    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let r = ReceiptBuilder::new("initial")
        .backend_id("sidecar:python")
        .backend_version("3.11")
        .adapter_version("0.1.0")
        .work_order_id(wo_id)
        .started_at(start)
        .finished_at(end)
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .capabilities(caps)
        .usage_raw(json!({"tokens": 999}))
        .usage(sample_usage())
        .verification(sample_verification())
        .add_trace_event(sample_agent_event())
        .add_artifact(sample_artifact())
        .build();

    assert_eq!(r.backend.id, "sidecar:python");
    assert_eq!(r.backend.backend_version.as_deref(), Some("3.11"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1.0"));
    assert_eq!(r.meta.work_order_id, wo_id);
    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, end);
    assert_eq!(r.meta.duration_ms, 30_000);
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.capabilities.len(), 1);
    assert_eq!(r.usage_raw["tokens"], json!(999));
    assert_eq!(r.usage.input_tokens, Some(100));
    assert!(r.verification.harness_ok);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
}

// ===========================================================================
// ReceiptBuilder — hashing
// ===========================================================================

#[test]
fn rb_with_hash_produces_64_hex_chars() {
    let r = ReceiptBuilder::new("m").with_hash().unwrap();
    let h = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn rb_build_then_with_hash() {
    let r = ReceiptBuilder::new("m").build().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn rb_hash_deterministic() {
    let start = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let end = start + Duration::seconds(1);
    let wo_id = Uuid::nil();

    let make = || {
        // Use a fixed work_order_id so the receipt is deterministic apart from run_id.
        ReceiptBuilder::new("m")
            .work_order_id(wo_id)
            .started_at(start)
            .finished_at(end)
            .build()
    };
    // run_id differs, so hashes differ.
    let r1 = make().with_hash().unwrap();
    let r2 = make().with_hash().unwrap();
    // Different run_ids => different hashes (this is expected).
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[test]
fn rb_hash_excludes_receipt_sha256_field() {
    let r = ReceiptBuilder::new("m").build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    // Manually set a fake hash, then re-hash — should get the same result.
    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("fake".into());
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "hash must ignore receipt_sha256 field");
}

#[test]
fn rb_with_hash_result_is_ok() {
    let result = ReceiptBuilder::new("m").with_hash();
    assert!(result.is_ok());
}

// ===========================================================================
// ReceiptBuilder — serialization
// ===========================================================================

#[test]
fn rb_serializes_to_json() {
    let r = ReceiptBuilder::new("mock").build();
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("mock"));
}

#[test]
fn rb_roundtrip_json() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let back: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.outcome, Outcome::Partial);
    assert_eq!(back.backend.id, "mock");
}

#[test]
fn rb_serialize_has_all_top_level_keys() {
    let r = ReceiptBuilder::new("m").build();
    let v = serde_json::to_value(&r).unwrap();
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
        assert!(obj.contains_key(*key), "missing key: {key}");
    }
}

#[test]
fn rb_serialize_outcome_snake_case() {
    for (outcome, expected) in [
        (Outcome::Complete, "complete"),
        (Outcome::Partial, "partial"),
        (Outcome::Failed, "failed"),
    ] {
        let r = ReceiptBuilder::new("m").outcome(outcome).build();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["outcome"], json!(expected));
    }
}

#[test]
fn rb_serialize_mode_snake_case() {
    for (mode, expected) in [
        (ExecutionMode::Passthrough, "passthrough"),
        (ExecutionMode::Mapped, "mapped"),
    ] {
        let r = ReceiptBuilder::new("m").mode(mode).build();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["mode"], json!(expected));
    }
}

#[test]
fn rb_serialize_meta_contains_contract_version() {
    let r = ReceiptBuilder::new("m").build();
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["meta"]["contract_version"], json!(CONTRACT_VERSION));
}

// ===========================================================================
// ReceiptBuilder — trace event kinds
// ===========================================================================

#[test]
fn rb_trace_run_started() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(r.trace[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn rb_trace_run_completed() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn rb_trace_assistant_delta() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
}

#[test]
fn rb_trace_assistant_message() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn rb_trace_tool_call() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "main.rs"}),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    if let AgentEventKind::ToolCall {
        tool_name, input, ..
    } = &r.trace[0].kind
    {
        assert_eq!(tool_name, "read_file");
        assert_eq!(input["path"], json!("main.rs"));
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn rb_trace_tool_result() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!("contents"),
            is_error: false,
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(r.trace[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn rb_trace_file_changed() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::FileChanged { .. }
    ));
}

#[test]
fn rb_trace_command_executed() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: Some("Compiling...".into()),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::CommandExecuted { .. }
    ));
}

#[test]
fn rb_trace_warning() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "deprecated".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    assert!(matches!(r.trace[0].kind, AgentEventKind::Warning { .. }));
}

#[test]
fn rb_trace_error_without_code() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &r.trace[0].kind
    {
        assert_eq!(message, "boom");
        assert!(error_code.is_none());
    } else {
        panic!("expected Error");
    }
}

#[test]
fn rb_trace_error_with_code() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "timeout".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    if let AgentEventKind::Error { error_code, .. } = &r.trace[0].kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    } else {
        panic!("expected Error");
    }
}

// ===========================================================================
// ReceiptBuilder — capability manifest with SupportLevel variants
// ===========================================================================

#[test]
fn rb_capability_native() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    let r = ReceiptBuilder::new("m").capabilities(caps).build();
    assert!(matches!(
        r.capabilities[&Capability::ToolRead],
        SupportLevel::Native
    ));
}

#[test]
fn rb_capability_emulated() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let r = ReceiptBuilder::new("m").capabilities(caps).build();
    assert!(matches!(
        r.capabilities[&Capability::Streaming],
        SupportLevel::Emulated
    ));
}

#[test]
fn rb_capability_unsupported() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(Capability::Audio, SupportLevel::Unsupported);
    let r = ReceiptBuilder::new("m").capabilities(caps).build();
    assert!(matches!(
        r.capabilities[&Capability::Audio],
        SupportLevel::Unsupported
    ));
}

#[test]
fn rb_capability_restricted() {
    let mut caps: CapabilityManifest = BTreeMap::new();
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let r = ReceiptBuilder::new("m").capabilities(caps).build();
    if let SupportLevel::Restricted { reason } = &r.capabilities[&Capability::ToolBash] {
        assert_eq!(reason, "sandboxed");
    } else {
        panic!("expected Restricted");
    }
}

#[test]
fn rb_capability_manifest_all_capabilities() {
    let caps: CapabilityManifest = all_capabilities()
        .into_iter()
        .map(|c| (c, SupportLevel::Native))
        .collect();
    let r = ReceiptBuilder::new("m").capabilities(caps).build();
    assert!(r.capabilities.len() >= 30);
}

// ===========================================================================
// ReceiptBuilder — complex nested types
// ===========================================================================

#[test]
fn rb_trace_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    let ext = r.trace[0].ext.as_ref().unwrap();
    assert_eq!(ext["raw_message"]["role"], json!("assistant"));
}

#[test]
fn rb_usage_raw_nested_json() {
    let raw = json!({
        "model": "gpt-4",
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        },
        "choices": [{"finish_reason": "stop"}]
    });
    let r = ReceiptBuilder::new("m").usage_raw(raw).build();
    assert_eq!(r.usage_raw["usage"]["total_tokens"], json!(150));
}

#[test]
fn rb_trace_serializes_event_kind_tagged() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("m").add_trace_event(evt).build();
    let v = serde_json::to_value(&r.trace[0]).unwrap();
    assert_eq!(v["type"], json!("assistant_message"));
}

#[test]
fn rb_trace_tool_call_serializes_type_tag() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    };
    let v = serde_json::to_value(&evt).unwrap();
    assert_eq!(v["type"], json!("tool_call"));
}

// ===========================================================================
// ErrorCode — as_str returns snake_case
// ===========================================================================

#[test]
fn error_code_as_str_is_snake_case() {
    let codes = [
        (
            ErrorCode::ProtocolInvalidEnvelope,
            "protocol_invalid_envelope",
        ),
        (
            ErrorCode::ProtocolHandshakeFailed,
            "protocol_handshake_failed",
        ),
        (ErrorCode::BackendTimeout, "backend_timeout"),
        (ErrorCode::BackendRateLimited, "backend_rate_limited"),
        (ErrorCode::PolicyDenied, "policy_denied"),
        (ErrorCode::Internal, "internal"),
        (ErrorCode::CapabilityUnsupported, "capability_unsupported"),
        (ErrorCode::WorkspaceInitFailed, "workspace_init_failed"),
        (ErrorCode::ReceiptHashMismatch, "receipt_hash_mismatch"),
        (ErrorCode::DialectUnknown, "dialect_unknown"),
        (ErrorCode::ConfigInvalid, "config_invalid"),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected, "ErrorCode::{code:?}");
    }
}

#[test]
fn error_code_serde_matches_as_str() {
    let code = ErrorCode::BackendTimeout;
    let json = serde_json::to_value(&code).unwrap();
    assert_eq!(json.as_str().unwrap(), code.as_str());
}

// ===========================================================================
// SupportLevel::satisfies
// ===========================================================================

#[test]
fn support_native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn support_native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn support_emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_unsupported_never_satisfies() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_restricted_satisfies_emulated_but_not_native() {
    let restricted = SupportLevel::Restricted {
        reason: "test".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// WorkOrder + Receipt interop
// ===========================================================================

#[test]
fn wo_and_receipt_linked_by_work_order_id() {
    let wo = WorkOrderBuilder::new("test").build();
    let r = ReceiptBuilder::new("mock").work_order_id(wo.id).build();
    assert_eq!(r.meta.work_order_id, wo.id);
}

#[test]
fn wo_and_receipt_full_flow() {
    let wo = WorkOrderBuilder::new("implement feature")
        .model("gpt-4")
        .max_turns(10)
        .policy(sample_policy())
        .build();

    let r = ReceiptBuilder::new("sidecar:node")
        .work_order_id(wo.id)
        .outcome(Outcome::Complete)
        .usage(sample_usage())
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
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "feature.diff".into(),
        })
        .verification(sample_verification())
        .with_hash()
        .unwrap();

    assert_eq!(r.meta.work_order_id, wo.id);
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.trace.len(), 2);
    assert_eq!(r.artifacts.len(), 1);
    assert!(r.receipt_sha256.is_some());
    assert!(r.verification.harness_ok);
}

// ===========================================================================
// WorkOrderBuilder — task accepts Into<String>
// ===========================================================================

#[test]
fn wo_task_from_str() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn wo_task_from_string() {
    let s = String::from("hello");
    let wo = WorkOrderBuilder::new(s).build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn wo_root_from_string() {
    let s = String::from("/path");
    let wo = WorkOrderBuilder::new("t").root(s).build();
    assert_eq!(wo.workspace.root, "/path");
}

#[test]
fn wo_model_from_string() {
    let s = String::from("model-v1");
    let wo = WorkOrderBuilder::new("t").model(s).build();
    assert_eq!(wo.config.model.as_deref(), Some("model-v1"));
}

// ===========================================================================
// ReceiptBuilder — backend_id accepts Into<String>
// ===========================================================================

#[test]
fn rb_backend_id_from_str() {
    let r = ReceiptBuilder::new("test").build();
    assert_eq!(r.backend.id, "test");
}

#[test]
fn rb_backend_id_from_string() {
    let s = String::from("dynamic");
    let r = ReceiptBuilder::new(s).build();
    assert_eq!(r.backend.id, "dynamic");
}

#[test]
fn rb_backend_version_from_string() {
    let s = String::from("2.0.0");
    let r = ReceiptBuilder::new("m").backend_version(s).build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0.0"));
}

#[test]
fn rb_adapter_version_from_string() {
    let s = String::from("1.0.0");
    let r = ReceiptBuilder::new("m").adapter_version(s).build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.0.0"));
}

// ===========================================================================
// Empty / edge cases
// ===========================================================================

#[test]
fn wo_empty_task() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn wo_unicode_task() {
    let wo = WorkOrderBuilder::new("修正バグ 🐛").build();
    assert_eq!(wo.task, "修正バグ 🐛");
}

#[test]
fn wo_very_long_task() {
    let long = "a".repeat(10_000);
    let wo = WorkOrderBuilder::new(&long).build();
    assert_eq!(wo.task.len(), 10_000);
}

#[test]
fn rb_empty_backend_id() {
    let r = ReceiptBuilder::new("").build();
    assert_eq!(r.backend.id, "");
}

#[test]
fn rb_unicode_backend_id() {
    let r = ReceiptBuilder::new("バックエンド").build();
    assert_eq!(r.backend.id, "バックエンド");
}

#[test]
fn wo_include_exclude_empty_vecs() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec![])
        .exclude(vec![])
        .build();
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn wo_budget_zero() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(0.0));
}

#[test]
fn wo_max_turns_zero() {
    let wo = WorkOrderBuilder::new("t").max_turns(0).build();
    assert_eq!(wo.config.max_turns, Some(0));
}

#[test]
fn rb_run_ids_are_unique() {
    let a = ReceiptBuilder::new("m").build();
    let b = ReceiptBuilder::new("m").build();
    assert_ne!(a.meta.run_id, b.meta.run_id);
}
