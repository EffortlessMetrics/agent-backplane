// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for [`WorkOrder`] and [`WorkOrderBuilder`].
//!
//! Categories:
//! 1. WorkOrderBuilder fluent API (all methods)
//! 2. WorkOrder serialization roundtrip
//! 3. Config vendor settings
//! 4. Capabilities requirements
//! 5. Context packet construction
//! 6. Execution lane settings
//! 7. Validation (required fields, invalid values)
//! 8. Edge cases

use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::ext::WorkOrderExt;
use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet,
    ExecutionLane, MinSupport, PolicyProfile, RuntimeConfig, WorkOrder, WorkOrderBuilder,
    WorkspaceMode,
};
use serde_json::json;
use std::collections::BTreeMap;

// ===========================================================================
// Helpers
// ===========================================================================

fn minimal() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn full_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["bash".into()],
    }
}

fn streaming_requirement() -> CapabilityRequirements {
    CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    }
}

fn full_context() -> ContextPacket {
    ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![ContextSnippet {
            name: "readme".into(),
            content: "# Hello".into(),
        }],
    }
}

fn full_runtime_config() -> RuntimeConfig {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    let mut env = BTreeMap::new();
    env.insert("RUST_LOG".into(), "debug".into());
    RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        env,
        max_budget_usd: Some(5.0),
        max_turns: Some(20),
    }
}

// ===========================================================================
// 1. WorkOrderBuilder fluent API
// ===========================================================================

#[test]
fn builder_new_sets_task() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn builder_new_accepts_string() {
    let wo = WorkOrderBuilder::new(String::from("owned")).build();
    assert_eq!(wo.task, "owned");
}

#[test]
fn builder_new_accepts_str_ref() {
    let s = "borrowed";
    let wo = WorkOrderBuilder::new(s).build();
    assert_eq!(wo.task, s);
}

#[test]
fn builder_default_lane_is_patch_first() {
    let wo = minimal();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn builder_default_workspace_root_is_dot() {
    let wo = minimal();
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn builder_default_workspace_mode_is_staged() {
    let wo = minimal();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn builder_default_include_is_empty() {
    let wo = minimal();
    assert!(wo.workspace.include.is_empty());
}

#[test]
fn builder_default_exclude_is_empty() {
    let wo = minimal();
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn builder_default_context_is_empty() {
    let wo = minimal();
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn builder_default_policy_is_empty() {
    let wo = minimal();
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
    assert!(wo.policy.allow_network.is_empty());
    assert!(wo.policy.deny_network.is_empty());
    assert!(wo.policy.require_approval_for.is_empty());
}

#[test]
fn builder_default_requirements_is_empty() {
    let wo = minimal();
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn builder_default_config_model_is_none() {
    let wo = minimal();
    assert!(wo.config.model.is_none());
}

#[test]
fn builder_default_config_vendor_is_empty() {
    let wo = minimal();
    assert!(wo.config.vendor.is_empty());
}

#[test]
fn builder_default_config_env_is_empty() {
    let wo = minimal();
    assert!(wo.config.env.is_empty());
}

#[test]
fn builder_default_budget_is_none() {
    let wo = minimal();
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn builder_default_max_turns_is_none() {
    let wo = minimal();
    assert!(wo.config.max_turns.is_none());
}

#[test]
fn builder_id_is_unique() {
    let a = minimal();
    let b = minimal();
    assert_ne!(a.id, b.id);
}

#[test]
fn builder_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn builder_lane_patch_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn builder_root() {
    let wo = WorkOrderBuilder::new("t").root("/workspace").build();
    assert_eq!(wo.workspace.root, "/workspace");
}

#[test]
fn builder_root_accepts_string() {
    let wo = WorkOrderBuilder::new("t")
        .root(String::from("/tmp"))
        .build();
    assert_eq!(wo.workspace.root, "/tmp");
}

#[test]
fn builder_workspace_mode_pass_through() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn builder_workspace_mode_staged() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn builder_include_patterns() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into(), "*.toml".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs", "*.toml"]);
}

#[test]
fn builder_exclude_patterns() {
    let wo = WorkOrderBuilder::new("t")
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn builder_context() {
    let ctx = full_context();
    let wo = WorkOrderBuilder::new("t").context(ctx.clone()).build();
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "readme");
}

#[test]
fn builder_policy() {
    let p = full_policy();
    let wo = WorkOrderBuilder::new("t").policy(p).build();
    assert_eq!(wo.policy.allowed_tools, vec!["read", "write"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["rm"]);
    assert_eq!(wo.policy.deny_read, vec!["*.secret"]);
    assert_eq!(wo.policy.deny_write, vec!["/etc/**"]);
    assert_eq!(wo.policy.allow_network, vec!["api.example.com"]);
    assert_eq!(wo.policy.deny_network, vec!["evil.com"]);
    assert_eq!(wo.policy.require_approval_for, vec!["bash"]);
}

#[test]
fn builder_requirements() {
    let reqs = streaming_requirement();
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
    assert_eq!(
        wo.requirements.required[0].capability,
        Capability::Streaming
    );
}

#[test]
fn builder_config() {
    let cfg = full_runtime_config();
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(20));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert!(wo.config.vendor.contains_key("abp"));
    assert!(wo.config.env.contains_key("RUST_LOG"));
}

#[test]
fn builder_model() {
    let wo = WorkOrderBuilder::new("t").model("claude-3").build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
}

#[test]
fn builder_model_accepts_string() {
    let wo = WorkOrderBuilder::new("t")
        .model(String::from("gpt-4o"))
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn builder_max_budget_usd() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(10.0).build();
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

#[test]
fn builder_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(42).build();
    assert_eq!(wo.config.max_turns, Some(42));
}

#[test]
fn builder_full_chain() {
    let wo = WorkOrderBuilder::new("full chain task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .context(full_context())
        .policy(full_policy())
        .requirements(streaming_requirement())
        .model("gpt-4")
        .max_budget_usd(5.0)
        .max_turns(15)
        .build();

    assert_eq!(wo.task, "full chain task");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/workspace");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.policy.allowed_tools.len(), 2);
    assert_eq!(wo.requirements.required.len(), 1);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(15));
}

#[test]
fn builder_model_overrides_config_model() {
    let cfg = RuntimeConfig {
        model: Some("old-model".into()),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t")
        .config(cfg)
        .model("new-model")
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("new-model"));
}

#[test]
fn builder_config_after_model_overrides() {
    let cfg = RuntimeConfig {
        model: Some("config-model".into()),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t")
        .model("builder-model")
        .config(cfg)
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("config-model"));
}

#[test]
fn builder_last_lane_wins() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .lane(ExecutionLane::PatchFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn builder_last_root_wins() {
    let wo = WorkOrderBuilder::new("t")
        .root("/first")
        .root("/second")
        .build();
    assert_eq!(wo.workspace.root, "/second");
}

#[test]
fn builder_last_include_wins() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["a".into()])
        .include(vec!["b".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["b"]);
}

#[test]
fn builder_last_exclude_wins() {
    let wo = WorkOrderBuilder::new("t")
        .exclude(vec!["a".into()])
        .exclude(vec!["b".into()])
        .build();
    assert_eq!(wo.workspace.exclude, vec!["b"]);
}

// ===========================================================================
// 2. WorkOrder serialization roundtrip
// ===========================================================================

#[test]
fn serde_roundtrip_minimal() {
    let wo = minimal();
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.id, wo.id);
    assert_eq!(de.task, wo.task);
}

#[test]
fn serde_roundtrip_full() {
    let wo = WorkOrderBuilder::new("full roundtrip")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/ws")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .context(full_context())
        .policy(full_policy())
        .requirements(streaming_requirement())
        .model("claude-3")
        .max_budget_usd(3.5)
        .max_turns(10)
        .build();

    let json = serde_json::to_string_pretty(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();

    assert_eq!(de.id, wo.id);
    assert_eq!(de.task, wo.task);
    assert_eq!(de.workspace.root, wo.workspace.root);
    assert_eq!(de.workspace.include, wo.workspace.include);
    assert_eq!(de.workspace.exclude, wo.workspace.exclude);
    assert_eq!(de.context.files, wo.context.files);
    assert_eq!(de.context.snippets.len(), wo.context.snippets.len());
    assert_eq!(de.policy.allowed_tools, wo.policy.allowed_tools);
    assert_eq!(
        de.requirements.required.len(),
        wo.requirements.required.len()
    );
    assert_eq!(de.config.model, wo.config.model);
    assert_eq!(de.config.max_turns, wo.config.max_turns);
    assert_eq!(de.config.max_budget_usd, wo.config.max_budget_usd);
}

#[test]
fn serde_roundtrip_preserves_lane_patch_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("patch_first"));
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.lane, ExecutionLane::PatchFirst));
}

#[test]
fn serde_roundtrip_preserves_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("workspace_first"));
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn serde_roundtrip_preserves_workspace_mode_staged() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("staged"));
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn serde_roundtrip_preserves_workspace_mode_pass_through() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("pass_through"));
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(de.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn serde_json_value_roundtrip() {
    let wo = minimal();
    let v = serde_json::to_value(&wo).unwrap();
    let de: WorkOrder = serde_json::from_value(v).unwrap();
    assert_eq!(de.id, wo.id);
}

#[test]
fn serde_deterministic_for_same_input() {
    let cfg = full_runtime_config();
    let wo1 = WorkOrderBuilder::new("det test")
        .config(cfg.clone())
        .build();
    // Serialize twice from same work order
    let j1 = serde_json::to_string(&wo1).unwrap();
    let j2 = serde_json::to_string(&wo1).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn serde_vendor_btreemap_sorted_keys() {
    let mut vendor = BTreeMap::new();
    vendor.insert("z_key".into(), json!(1));
    vendor.insert("a_key".into(), json!(2));
    vendor.insert("m_key".into(), json!(3));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

// ===========================================================================
// 3. Config vendor settings
// ===========================================================================

#[test]
fn vendor_config_abp_mode_passthrough() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let abp = wo.vendor_config("abp").unwrap();
    assert_eq!(abp["mode"], "passthrough");
}

#[test]
fn vendor_config_abp_mode_mapped() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "mapped"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let abp = wo.vendor_config("abp").unwrap();
    assert_eq!(abp["mode"], "mapped");
}

#[test]
fn vendor_config_abp_request() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        json!({"mode": "passthrough", "request": {"temperature": 0.5}}),
    );
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let abp = wo.vendor_config("abp").unwrap();
    assert_eq!(abp["request"]["temperature"], 0.5);
}

#[test]
fn vendor_config_missing_key_returns_none() {
    let wo = minimal();
    assert!(wo.vendor_config("nonexistent").is_none());
}

#[test]
fn vendor_config_multiple_vendors() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.7}));
    vendor.insert("anthropic".into(), json!({"max_tokens": 4096}));
    vendor.insert("abp".into(), json!({"mode": "mapped"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert!(wo.vendor_config("openai").is_some());
    assert!(wo.vendor_config("anthropic").is_some());
    assert!(wo.vendor_config("abp").is_some());
}

#[test]
fn vendor_config_nested_objects() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "deep".into(),
        json!({"level1": {"level2": {"level3": true}}}),
    );
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let deep = wo.vendor_config("deep").unwrap();
    assert_eq!(deep["level1"]["level2"]["level3"], true);
}

#[test]
fn vendor_config_array_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("v".into(), json!({"stop": ["END", "STOP"]}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let v = wo.vendor_config("v").unwrap();
    assert_eq!(v["stop"][0], "END");
    assert_eq!(v["stop"][1], "STOP");
}

#[test]
fn vendor_config_null_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("v".into(), json!(null));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert!(wo.vendor_config("v").unwrap().is_null());
}

#[test]
fn env_variables_set_via_config() {
    let mut env = BTreeMap::new();
    env.insert("MY_VAR".into(), "my_value".into());
    let cfg = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.env.get("MY_VAR").unwrap(), "my_value");
}

#[test]
fn env_variables_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("A".into(), "1".into());
    env.insert("B".into(), "2".into());
    let cfg = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.config.env.len(), 2);
    assert_eq!(de.config.env["A"], "1");
    assert_eq!(de.config.env["B"], "2");
}

// ===========================================================================
// 4. Capabilities requirements
// ===========================================================================

#[test]
fn capabilities_empty_by_default() {
    let wo = minimal();
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn capabilities_single_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert!(wo.has_capability(&Capability::ToolRead));
    assert!(!wo.has_capability(&Capability::ToolWrite));
}

#[test]
fn capabilities_single_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert!(wo.has_capability(&Capability::Streaming));
}

#[test]
fn capabilities_multiple() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert!(wo.has_capability(&Capability::ToolRead));
    assert!(wo.has_capability(&Capability::ToolWrite));
    assert!(wo.has_capability(&Capability::ToolBash));
    assert!(!wo.has_capability(&Capability::McpClient));
}

#[test]
fn capabilities_all_variants_serialize() {
    let all = vec![
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
    ];
    let reqs = CapabilityRequirements {
        required: all
            .into_iter()
            .map(|c| CapabilityRequirement {
                capability: c,
                min_support: MinSupport::Emulated,
            })
            .collect(),
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.requirements.required.len(), 26);
}

#[test]
fn capabilities_roundtrip_min_support_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"native\""));
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        de.requirements.required[0].min_support,
        MinSupport::Native
    ));
}

#[test]
fn capabilities_roundtrip_min_support_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"emulated\""));
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        de.requirements.required[0].min_support,
        MinSupport::Emulated
    ));
}

// ===========================================================================
// 5. Context packet construction
// ===========================================================================

#[test]
fn context_default_is_empty() {
    let ctx = ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

#[test]
fn context_with_files_only() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 2);
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn context_with_snippets_only() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "some hint".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert!(wo.context.files.is_empty());
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "hint");
    assert_eq!(wo.context.snippets[0].content, "some hint");
}

#[test]
fn context_with_files_and_snippets() {
    let wo = WorkOrderBuilder::new("t").context(full_context()).build();
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn context_multiple_snippets() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![
            ContextSnippet {
                name: "a".into(),
                content: "alpha".into(),
            },
            ContextSnippet {
                name: "b".into(),
                content: "beta".into(),
            },
            ContextSnippet {
                name: "c".into(),
                content: "gamma".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.snippets.len(), 3);
}

#[test]
fn context_roundtrip() {
    let ctx = full_context();
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.context.files, wo.context.files);
    assert_eq!(de.context.snippets.len(), wo.context.snippets.len());
    assert_eq!(de.context.snippets[0].name, wo.context.snippets[0].name);
    assert_eq!(
        de.context.snippets[0].content,
        wo.context.snippets[0].content
    );
}

// ===========================================================================
// 6. Execution lane settings
// ===========================================================================

#[test]
fn lane_serde_patch_first() {
    let json = serde_json::to_value(ExecutionLane::PatchFirst).unwrap();
    assert_eq!(json, json!("patch_first"));
}

#[test]
fn lane_serde_workspace_first() {
    let json = serde_json::to_value(ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(json, json!("workspace_first"));
}

#[test]
fn lane_deserialize_patch_first() {
    let lane: ExecutionLane = serde_json::from_value(json!("patch_first")).unwrap();
    assert!(matches!(lane, ExecutionLane::PatchFirst));
}

#[test]
fn lane_deserialize_workspace_first() {
    let lane: ExecutionLane = serde_json::from_value(json!("workspace_first")).unwrap();
    assert!(matches!(lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn lane_invalid_value_fails() {
    let result = serde_json::from_value::<ExecutionLane>(json!("invalid_lane"));
    assert!(result.is_err());
}

#[test]
fn workspace_mode_serde_staged() {
    let v = serde_json::to_value(WorkspaceMode::Staged).unwrap();
    assert_eq!(v, json!("staged"));
}

#[test]
fn workspace_mode_serde_pass_through() {
    let v = serde_json::to_value(WorkspaceMode::PassThrough).unwrap();
    assert_eq!(v, json!("pass_through"));
}

#[test]
fn workspace_mode_invalid_fails() {
    let result = serde_json::from_value::<WorkspaceMode>(json!("bad"));
    assert!(result.is_err());
}

// ===========================================================================
// 7. Validation (ConfigValidator + ConfigDefaults)
// ===========================================================================

#[test]
fn validation_minimal_has_no_errors() {
    let wo = minimal();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    let errors: Vec<_> = warnings
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .collect();
    assert!(errors.is_empty());
}

#[test]
fn validation_empty_task_is_error() {
    let wo = WorkOrderBuilder::new("").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "task" && matches!(w.severity, WarningSeverity::Error))
    );
}

#[test]
fn validation_whitespace_task_is_error() {
    let wo = WorkOrderBuilder::new("   ").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task"));
}

#[test]
fn validation_zero_max_turns_is_error() {
    let wo = WorkOrderBuilder::new("t").max_turns(0).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.max_turns" && matches!(w.severity, WarningSeverity::Error))
    );
}

#[test]
fn validation_positive_max_turns_ok() {
    let wo = WorkOrderBuilder::new("t").max_turns(1).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "config.max_turns"));
}

#[test]
fn validation_zero_budget_is_error() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.0).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(
        |w| w.field == "config.max_budget_usd" && matches!(w.severity, WarningSeverity::Error)
    ));
}

#[test]
fn validation_negative_budget_is_error() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(-1.0).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validation_positive_budget_ok() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.01).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validation_duplicate_tools_warning() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "read".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(
        |w| w.field == "policy.allowed_tools" && matches!(w.severity, WarningSeverity::Warning)
    ));
}

#[test]
fn validation_no_duplicate_tools_no_warning() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(!warnings.iter().any(|w| w.field == "policy.allowed_tools"));
}

#[test]
fn validation_empty_model_is_error() {
    let wo = WorkOrderBuilder::new("t").model("").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.model"));
}

#[test]
fn validation_whitespace_model_is_error() {
    let wo = WorkOrderBuilder::new("t").model("   ").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.model"));
}

#[test]
fn validation_empty_glob_in_deny_read_is_error() {
    let policy = PolicyProfile {
        deny_read: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_read"));
}

#[test]
fn validation_empty_glob_in_deny_write_is_error() {
    let policy = PolicyProfile {
        deny_write: vec!["  ".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_write"));
}

#[test]
fn validation_empty_glob_in_allow_network_is_error() {
    let policy = PolicyProfile {
        allow_network: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.allow_network"));
}

#[test]
fn validation_empty_glob_in_deny_network_is_error() {
    let policy = PolicyProfile {
        deny_network: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_network"));
}

#[test]
fn validation_empty_glob_in_disallowed_tools_is_error() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.disallowed_tools")
    );
}

#[test]
fn validation_empty_glob_in_require_approval_for_is_error() {
    let policy = PolicyProfile {
        require_approval_for: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.require_approval_for")
    );
}

#[test]
fn validation_empty_vendor_key_is_error() {
    let mut vendor = BTreeMap::new();
    vendor.insert("".into(), json!(1));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.vendor"));
}

#[test]
fn validation_whitespace_vendor_key_is_error() {
    let mut vendor = BTreeMap::new();
    vendor.insert("  ".into(), json!(1));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.vendor"));
}

#[test]
fn validation_full_valid_work_order_no_errors() {
    let wo = WorkOrderBuilder::new("fix the bug")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/ws")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .policy(full_policy())
        .requirements(streaming_requirement())
        .context(full_context())
        .build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    let errors: Vec<_> = warnings
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .collect();
    assert!(errors.is_empty());
}

// ConfigDefaults tests

#[test]
fn config_defaults_apply_max_turns() {
    let mut wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.max_turns.is_none());
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.max_turns,
        Some(ConfigDefaults::default_max_turns())
    );
}

#[test]
fn config_defaults_apply_budget() {
    let mut wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.max_budget_usd.is_none());
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.max_budget_usd,
        Some(ConfigDefaults::default_max_budget())
    );
}

#[test]
fn config_defaults_apply_model() {
    let mut wo = WorkOrderBuilder::new("t").build();
    assert!(wo.config.model.is_none());
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.model.as_deref(),
        Some(ConfigDefaults::default_model())
    );
}

#[test]
fn config_defaults_do_not_override_existing() {
    let mut wo = WorkOrderBuilder::new("t")
        .model("custom")
        .max_turns(5)
        .max_budget_usd(1.5)
        .build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.model.as_deref(), Some("custom"));
    assert_eq!(wo.config.max_turns, Some(5));
    assert_eq!(wo.config.max_budget_usd, Some(1.5));
}

#[test]
fn config_defaults_values() {
    assert_eq!(ConfigDefaults::default_max_turns(), 25);
    assert!((ConfigDefaults::default_max_budget() - 1.0).abs() < f64::EPSILON);
    assert_eq!(ConfigDefaults::default_model(), "gpt-4");
}

// ===========================================================================
// 8. Edge cases
// ===========================================================================

#[test]
fn edge_empty_task() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn edge_very_long_task() {
    let long_task = "a".repeat(10_000);
    let wo = WorkOrderBuilder::new(long_task.clone()).build();
    assert_eq!(wo.task.len(), 10_000);
    // Roundtrip
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.task, long_task);
}

#[test]
fn edge_unicode_task_basic() {
    let wo = WorkOrderBuilder::new("修复登录模块的bug").build();
    assert_eq!(wo.task, "修复登录模块的bug");
}

#[test]
fn edge_unicode_task_emoji() {
    let wo = WorkOrderBuilder::new("Fix 🐛 in auth 🔐").build();
    assert_eq!(wo.task, "Fix 🐛 in auth 🔐");
}

#[test]
fn edge_unicode_task_rtl() {
    let wo = WorkOrderBuilder::new("إصلاح الخطأ").build();
    assert_eq!(wo.task, "إصلاح الخطأ");
}

#[test]
fn edge_unicode_task_mixed_scripts() {
    let wo = WorkOrderBuilder::new("Fix バグ в коде 코드에서").build();
    assert_eq!(wo.task, "Fix バグ в коде 코드에서");
}

#[test]
fn edge_unicode_roundtrip() {
    let task = "日本語テスト 🎉 données café";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.task, task);
}

#[test]
fn edge_special_chars_in_task() {
    let wo = WorkOrderBuilder::new("Fix \"quotes\" and \\backslashes\\").build();
    assert_eq!(wo.task, "Fix \"quotes\" and \\backslashes\\");
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.task, wo.task);
}

#[test]
fn edge_newlines_in_task() {
    let wo = WorkOrderBuilder::new("line1\nline2\nline3").build();
    assert!(wo.task.contains('\n'));
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.task, wo.task);
}

#[test]
fn edge_tabs_in_task() {
    let wo = WorkOrderBuilder::new("col1\tcol2").build();
    assert!(wo.task.contains('\t'));
}

#[test]
fn edge_empty_include_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec![])
        .exclude(vec![])
        .build();
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn edge_large_number_of_include_patterns() {
    let patterns: Vec<String> = (0..100).map(|i| format!("pattern_{i}/**")).collect();
    let wo = WorkOrderBuilder::new("t").include(patterns.clone()).build();
    assert_eq!(wo.workspace.include.len(), 100);
}

#[test]
fn edge_large_number_of_context_files() {
    let files: Vec<String> = (0..200).map(|i| format!("file_{i}.rs")).collect();
    let ctx = ContextPacket {
        files,
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 200);
}

#[test]
fn edge_many_snippets() {
    let snippets: Vec<ContextSnippet> = (0..50)
        .map(|i| ContextSnippet {
            name: format!("snippet_{i}"),
            content: format!("content of snippet {i}"),
        })
        .collect();
    let ctx = ContextPacket {
        files: vec![],
        snippets,
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.snippets.len(), 50);
}

#[test]
fn edge_empty_snippet_content() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "empty".into(),
            content: "".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.snippets[0].content, "");
}

#[test]
fn edge_large_budget() {
    let wo = WorkOrderBuilder::new("t")
        .max_budget_usd(999_999.99)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(999_999.99));
}

#[test]
fn edge_max_u32_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(u32::MAX).build();
    assert_eq!(wo.config.max_turns, Some(u32::MAX));
}

#[test]
fn edge_max_turns_one() {
    let wo = WorkOrderBuilder::new("t").max_turns(1).build();
    assert_eq!(wo.config.max_turns, Some(1));
}

#[test]
fn edge_fractional_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.001).build();
    assert_eq!(wo.config.max_budget_usd, Some(0.001));
}

// WorkOrderExt tests

#[test]
fn ext_is_code_task_true() {
    let wo = WorkOrderBuilder::new("implement the feature").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_fix() {
    let wo = WorkOrderBuilder::new("Fix the login bug").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_refactor() {
    let wo = WorkOrderBuilder::new("Refactor auth module").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_code() {
    let wo = WorkOrderBuilder::new("Write code for parser").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_false() {
    let wo = WorkOrderBuilder::new("Send an email").build();
    assert!(!wo.is_code_task());
}

#[test]
fn ext_task_summary_short() {
    let wo = WorkOrderBuilder::new("short").build();
    assert_eq!(wo.task_summary(100), "short");
}

#[test]
fn ext_task_summary_truncation() {
    let wo = WorkOrderBuilder::new("this is a longer task description").build();
    let summary = wo.task_summary(10);
    assert!(summary.len() <= 14); // 10 + "…" (3 bytes)
    assert!(summary.ends_with('…'));
}

#[test]
fn ext_task_summary_unicode_boundary() {
    let wo = WorkOrderBuilder::new("日本語テスト").build();
    let summary = wo.task_summary(6);
    // Should not panic on unicode boundary
    assert!(summary.ends_with('…'));
}

#[test]
fn ext_tool_budget_remaining() {
    let wo = WorkOrderBuilder::new("t").max_turns(10).build();
    assert_eq!(wo.tool_budget_remaining(), Some(10));
}

#[test]
fn ext_tool_budget_remaining_none() {
    let wo = minimal();
    assert_eq!(wo.tool_budget_remaining(), None);
}

#[test]
fn ext_required_capabilities_infers_edit() {
    let wo = WorkOrderBuilder::new("edit the config file").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolEdit));
}

#[test]
fn ext_required_capabilities_infers_grep() {
    let wo = WorkOrderBuilder::new("search for the pattern").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolGrep));
}

#[test]
fn ext_required_capabilities_infers_bash() {
    let wo = WorkOrderBuilder::new("run shell command").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolBash));
}

#[test]
fn ext_required_capabilities_merges_explicit_and_inferred() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("edit and refactor")
        .requirements(reqs)
        .build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::Streaming));
    assert!(caps.contains(&Capability::ToolEdit));
}

#[test]
fn ext_vendor_config_lookup() {
    let mut vendor = BTreeMap::new();
    vendor.insert("key".into(), json!("value"));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.vendor_config("key").unwrap(), &json!("value"));
}

// ===========================================================================
// Additional: JSON deserialization from raw JSON
// ===========================================================================

#[test]
fn deserialize_from_raw_json() {
    let raw = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "raw json task",
        "lane": "patch_first",
        "workspace": {
            "root": ".",
            "mode": "staged",
            "include": [],
            "exclude": []
        },
        "context": {"files": [], "snippets": []},
        "policy": {
            "allowed_tools": [],
            "disallowed_tools": [],
            "deny_read": [],
            "deny_write": [],
            "allow_network": [],
            "deny_network": [],
            "require_approval_for": []
        },
        "requirements": {"required": []},
        "config": {
            "model": null,
            "vendor": {},
            "env": {},
            "max_budget_usd": null,
            "max_turns": null
        }
    }"#;
    let wo: WorkOrder = serde_json::from_str(raw).unwrap();
    assert_eq!(wo.task, "raw json task");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn deserialize_partial_config_with_model() {
    let raw = r#"{
        "id": "00000000-0000-0000-0000-000000000002",
        "task": "partial config",
        "lane": "workspace_first",
        "workspace": {"root": "/ws", "mode": "pass_through", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": []},
        "requirements": {"required": []},
        "config": {"model": "gpt-4o", "vendor": {}, "env": {}, "max_budget_usd": 2.5, "max_turns": 15}
    }"#;
    let wo: WorkOrder = serde_json::from_str(raw).unwrap();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    assert_eq!(wo.config.max_budget_usd, Some(2.5));
    assert_eq!(wo.config.max_turns, Some(15));
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

// ===========================================================================
// Policy roundtrip
// ===========================================================================

#[test]
fn policy_roundtrip_all_fields() {
    let wo = WorkOrderBuilder::new("t").policy(full_policy()).build();
    let json = serde_json::to_string(&wo).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.policy.allowed_tools, wo.policy.allowed_tools);
    assert_eq!(de.policy.disallowed_tools, wo.policy.disallowed_tools);
    assert_eq!(de.policy.deny_read, wo.policy.deny_read);
    assert_eq!(de.policy.deny_write, wo.policy.deny_write);
    assert_eq!(de.policy.allow_network, wo.policy.allow_network);
    assert_eq!(de.policy.deny_network, wo.policy.deny_network);
    assert_eq!(
        de.policy.require_approval_for,
        wo.policy.require_approval_for
    );
}

#[test]
fn policy_many_tools() {
    let tools: Vec<String> = (0..100).map(|i| format!("tool_{i}")).collect();
    let policy = PolicyProfile {
        allowed_tools: tools.clone(),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools.len(), 100);
}

// ===========================================================================
// Multiple validation errors
// ===========================================================================

#[test]
fn validation_accumulates_multiple_errors() {
    let policy = PolicyProfile {
        deny_read: vec!["".into()],
        deny_write: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("")
        .model("")
        .max_turns(0)
        .max_budget_usd(-1.0)
        .policy(policy)
        .build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    let error_count = warnings
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .count();
    // task, model, max_turns, max_budget_usd, deny_read, deny_write = 6
    assert!(
        error_count >= 5,
        "expected at least 5 errors, got {error_count}"
    );
}

// ===========================================================================
// Clone and Debug
// ===========================================================================

#[test]
fn work_order_is_clone() {
    let wo = minimal();
    let cloned = wo.clone();
    assert_eq!(cloned.id, wo.id);
    assert_eq!(cloned.task, wo.task);
}

#[test]
fn work_order_is_debug() {
    let wo = minimal();
    let debug = format!("{wo:?}");
    assert!(debug.contains("WorkOrder"));
    assert!(debug.contains("test task"));
}

#[test]
fn builder_is_debug() {
    let builder = WorkOrderBuilder::new("debug test");
    let debug = format!("{builder:?}");
    assert!(debug.contains("WorkOrderBuilder"));
}
