#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Exhaustive tests for [`WorkOrder`] construction, validation, and
//! serialization covering builder patterns, config validation, serde
//! roundtrips, vendor config, and edge cases.

use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::ext::WorkOrderExt;
use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet,
    ExecutionLane, ExecutionMode, MinSupport, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use serde_json::json;
use std::collections::BTreeMap;

// =========================================================================
// Helpers
// =========================================================================

fn minimal() -> WorkOrder {
    WorkOrderBuilder::new("do something").build()
}

fn full() -> WorkOrder {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    vendor.insert("openai".into(), json!({"temperature": 0.7}));

    let mut env = BTreeMap::new();
    env.insert("RUST_LOG".into(), "debug".into());

    WorkOrderBuilder::new("Refactor the auth module")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use JWT tokens".into(),
            }],
        })
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec!["*.key".into()],
            deny_write: vec!["/etc/**".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["*.malware.io".into()],
            require_approval_for: vec!["delete".into()],
        })
        .requirements(CapabilityRequirements {
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
        })
        .config(RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor,
            env,
            max_budget_usd: Some(5.0),
            max_turns: Some(20),
        })
        .build()
}

fn validator() -> ConfigValidator {
    ConfigValidator::new()
}

fn has_error(warnings: &[abp_core::config::ConfigWarning], field: &str) -> bool {
    warnings
        .iter()
        .any(|w| w.field == field && w.severity == WarningSeverity::Error)
}

fn has_warning(warnings: &[abp_core::config::ConfigWarning], field: &str) -> bool {
    warnings
        .iter()
        .any(|w| w.field == field && w.severity == WarningSeverity::Warning)
}

// =========================================================================
// 1. WorkOrderBuilder
// =========================================================================

#[test]
fn builder_minimal_work_order() {
    let wo = minimal();
    assert_eq!(wo.task, "do something");
    assert!(!wo.id.is_nil());
}

#[test]
fn builder_task_preserved() {
    let wo = WorkOrderBuilder::new("Hello world").build();
    assert_eq!(wo.task, "Hello world");
}

#[test]
fn builder_default_lane_is_patch_first() {
    let wo = minimal();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn builder_default_root_is_dot() {
    let wo = minimal();
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn builder_default_workspace_mode_is_staged() {
    let wo = minimal();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn builder_default_include_empty() {
    let wo = minimal();
    assert!(wo.workspace.include.is_empty());
}

#[test]
fn builder_default_exclude_empty() {
    let wo = minimal();
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn builder_default_context_empty() {
    let wo = minimal();
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn builder_default_policy_empty() {
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
fn builder_default_requirements_empty() {
    let wo = minimal();
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn builder_default_config_model_none() {
    let wo = minimal();
    assert!(wo.config.model.is_none());
}

#[test]
fn builder_default_config_budget_none() {
    let wo = minimal();
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn builder_default_config_turns_none() {
    let wo = minimal();
    assert!(wo.config.max_turns.is_none());
}

#[test]
fn builder_default_vendor_empty() {
    let wo = minimal();
    assert!(wo.config.vendor.is_empty());
}

#[test]
fn builder_default_env_empty() {
    let wo = minimal();
    assert!(wo.config.env.is_empty());
}

#[test]
fn builder_full_work_order() {
    let wo = full();
    assert_eq!(wo.task, "Refactor the auth module");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.context.files, vec!["src/main.rs"]);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.policy.allowed_tools.len(), 2);
    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.max_turns, Some(20));
    assert_eq!(wo.config.vendor.len(), 2);
    assert_eq!(wo.config.env.len(), 1);
}

#[test]
fn builder_method_chaining_lane() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn builder_method_chaining_root() {
    let wo = WorkOrderBuilder::new("t").root("/my/root").build();
    assert_eq!(wo.workspace.root, "/my/root");
}

#[test]
fn builder_method_chaining_workspace_mode() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn builder_method_chaining_include_exclude() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into()])
        .exclude(vec!["*.log".into()])
        .build();
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["*.log"]);
}

#[test]
fn builder_method_chaining_context() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files, vec!["a.rs"]);
}

#[test]
fn builder_method_chaining_policy() {
    let pol = PolicyProfile {
        allowed_tools: vec!["x".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    assert_eq!(wo.policy.allowed_tools, vec!["x"]);
}

#[test]
fn builder_method_chaining_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn builder_method_chaining_config() {
    let cfg = RuntimeConfig {
        model: Some("claude-3".into()),
        max_turns: Some(5),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_turns, Some(5));
}

#[test]
fn builder_model_shorthand() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4o").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn builder_max_budget_usd() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(2.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(2.5));
}

#[test]
fn builder_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(42).build();
    assert_eq!(wo.config.max_turns, Some(42));
}

#[test]
fn builder_unique_ids() {
    let a = minimal();
    let b = minimal();
    assert_ne!(a.id, b.id);
}

#[test]
fn builder_full_chaining_all_methods() {
    let wo = WorkOrderBuilder::new("chain test")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/r")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["a".into()])
        .exclude(vec!["b".into()])
        .context(ContextPacket::default())
        .policy(PolicyProfile::default())
        .requirements(CapabilityRequirements::default())
        .model("m")
        .max_budget_usd(1.0)
        .max_turns(10)
        .build();
    assert_eq!(wo.task, "chain test");
    assert_eq!(wo.workspace.root, "/r");
    assert_eq!(wo.config.model.as_deref(), Some("m"));
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn builder_config_vendor_abp_metadata() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        json!({"mode": "passthrough", "request_id": "r1"}),
    );
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["abp"]["mode"], "passthrough");
    assert_eq!(wo.config.vendor["abp"]["request_id"], "r1");
}

#[test]
fn builder_capability_tool_read_native() {
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
fn builder_multiple_capabilities() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert!(wo.has_capability(&Capability::Streaming));
    assert!(wo.has_capability(&Capability::McpClient));
}

// =========================================================================
// 2. WorkOrder Validation
// =========================================================================

#[test]
fn validate_empty_task_rejected() {
    let wo = WorkOrderBuilder::new("").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "task"));
}

#[test]
fn validate_whitespace_only_task_rejected() {
    let wo = WorkOrderBuilder::new("   ").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "task"));
}

#[test]
fn validate_valid_task_accepted() {
    let wo = minimal();
    let warnings = validator().validate_work_order(&wo);
    assert!(!has_error(&warnings, "task"));
}

#[test]
fn validate_max_turns_zero_rejected() {
    let wo = WorkOrderBuilder::new("t").max_turns(0).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "config.max_turns"));
}

#[test]
fn validate_max_turns_positive_ok() {
    let wo = WorkOrderBuilder::new("t").max_turns(1).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(!has_error(&warnings, "config.max_turns"));
}

#[test]
fn validate_max_turns_none_ok() {
    let wo = minimal();
    let warnings = validator().validate_work_order(&wo);
    assert!(!has_error(&warnings, "config.max_turns"));
}

#[test]
fn validate_budget_zero_rejected() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.0).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "config.max_budget_usd"));
}

#[test]
fn validate_budget_negative_rejected() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(-1.0).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "config.max_budget_usd"));
}

#[test]
fn validate_budget_positive_ok() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.01).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(!has_error(&warnings, "config.max_budget_usd"));
}

#[test]
fn validate_budget_none_ok() {
    let wo = minimal();
    let warnings = validator().validate_work_order(&wo);
    assert!(!has_error(&warnings, "config.max_budget_usd"));
}

#[test]
fn validate_model_empty_string_rejected() {
    let wo = WorkOrderBuilder::new("t").model("").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "config.model"));
}

#[test]
fn validate_model_whitespace_rejected() {
    let wo = WorkOrderBuilder::new("t").model("   ").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "config.model"));
}

#[test]
fn validate_model_valid_ok() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
    let warnings = validator().validate_work_order(&wo);
    assert!(!has_error(&warnings, "config.model"));
}

#[test]
fn validate_duplicate_allowed_tools() {
    let pol = PolicyProfile {
        allowed_tools: vec!["bash".into(), "read".into(), "bash".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_warning(&warnings, "policy.allowed_tools"));
}

#[test]
fn validate_empty_glob_in_deny_read() {
    let pol = PolicyProfile {
        deny_read: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "policy.deny_read"));
}

#[test]
fn validate_empty_glob_in_deny_write() {
    let pol = PolicyProfile {
        deny_write: vec!["  ".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "policy.deny_write"));
}

#[test]
fn validate_empty_glob_in_allow_network() {
    let pol = PolicyProfile {
        allow_network: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "policy.allow_network"));
}

#[test]
fn validate_empty_glob_in_deny_network() {
    let pol = PolicyProfile {
        deny_network: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "policy.deny_network"));
}

#[test]
fn validate_empty_glob_in_disallowed_tools() {
    let pol = PolicyProfile {
        disallowed_tools: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "policy.disallowed_tools"));
}

#[test]
fn validate_empty_glob_in_require_approval_for() {
    let pol = PolicyProfile {
        require_approval_for: vec!["".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "policy.require_approval_for"));
}

#[test]
fn validate_empty_vendor_key_rejected() {
    let mut vendor = BTreeMap::new();
    vendor.insert("".into(), json!("v"));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let warnings = validator().validate_work_order(&wo);
    assert!(has_error(&warnings, "config.vendor"));
}

#[test]
fn validate_full_work_order_clean() {
    let wo = full();
    let warnings = validator().validate_work_order(&wo);
    let errors: Vec<_> = warnings
        .iter()
        .filter(|w| w.severity == WarningSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn validate_multiple_errors_accumulated() {
    let mut vendor = BTreeMap::new();
    vendor.insert("".into(), json!(1));
    let cfg = RuntimeConfig {
        model: Some("".into()),
        vendor,
        max_turns: Some(0),
        max_budget_usd: Some(-1.0),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("").config(cfg).build();
    let warnings = validator().validate_work_order(&wo);
    let error_count = warnings
        .iter()
        .filter(|w| w.severity == WarningSeverity::Error)
        .count();
    assert!(error_count >= 4, "expected >=4 errors, got {error_count}");
}

// =========================================================================
// 3. Serialization
// =========================================================================

#[test]
fn serde_json_roundtrip_minimal() {
    let wo = minimal();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, back.id);
    assert_eq!(wo.task, back.task);
}

#[test]
fn serde_json_roundtrip_full() {
    let wo = full();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, back.id);
    assert_eq!(wo.task, back.task);
    assert_eq!(wo.workspace.root, back.workspace.root);
    assert_eq!(wo.workspace.include, back.workspace.include);
    assert_eq!(wo.workspace.exclude, back.workspace.exclude);
    assert_eq!(wo.context.files, back.context.files);
    assert_eq!(wo.context.snippets.len(), back.context.snippets.len());
    assert_eq!(wo.policy.allowed_tools, back.policy.allowed_tools);
    assert_eq!(
        wo.requirements.required.len(),
        back.requirements.required.len()
    );
    assert_eq!(wo.config.model, back.config.model);
    assert_eq!(wo.config.max_turns, back.config.max_turns);
    assert_eq!(wo.config.vendor.len(), back.config.vendor.len());
    assert_eq!(wo.config.env, back.config.env);
}

#[test]
fn serde_roundtrip_preserves_lane_patch_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.lane, ExecutionLane::PatchFirst));
}

#[test]
fn serde_roundtrip_preserves_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn serde_roundtrip_preserves_workspace_mode_pass_through() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn serde_roundtrip_preserves_workspace_mode_staged() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn serde_deterministic_btreemap_vendor() {
    let mut vendor = BTreeMap::new();
    vendor.insert("z_last".into(), json!(1));
    vendor.insert("a_first".into(), json!(2));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let a_pos = json.find("a_first").unwrap();
    let z_pos = json.find("z_last").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should serialize keys in order");
}

#[test]
fn serde_deterministic_btreemap_env() {
    let mut env = BTreeMap::new();
    env.insert("Z_VAR".into(), "z".into());
    env.insert("A_VAR".into(), "a".into());
    let cfg = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let a_pos = json.find("A_VAR").unwrap();
    let z_pos = json.find("Z_VAR").unwrap();
    assert!(a_pos < z_pos, "BTreeMap env should serialize keys in order");
}

#[test]
fn serde_optional_model_none_missing() {
    let wo = minimal();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert!(v["config"]["model"].is_null());
}

#[test]
fn serde_optional_model_some_present() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["config"]["model"], "gpt-4");
}

#[test]
fn serde_optional_budget_none() {
    let wo = minimal();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert!(v["config"]["max_budget_usd"].is_null());
}

#[test]
fn serde_optional_budget_some() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(3.14).build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["config"]["max_budget_usd"], 3.14);
}

#[test]
fn serde_optional_turns_none() {
    let wo = minimal();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert!(v["config"]["max_turns"].is_null());
}

#[test]
fn serde_optional_turns_some() {
    let wo = WorkOrderBuilder::new("t").max_turns(7).build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["config"]["max_turns"], 7);
}

#[test]
fn serde_unknown_fields_in_vendor_preserved() {
    let wo = minimal();
    let mut v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    v["config"]["vendor"]["custom"] = json!({"secret": true});
    let json = serde_json::to_string(&v).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.config.vendor["custom"]["secret"], true);
}

#[test]
fn serde_canonical_json_deterministic() {
    let wo = full();
    let json1 = abp_core::canonical_json(&wo).unwrap();
    let json2 = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn serde_canonical_json_sorts_keys() {
    let wo = full();
    let canonical = abp_core::canonical_json(&wo).unwrap();
    // "config" should come before "context" alphabetically
    let config_pos = canonical.find("\"config\"").unwrap();
    let context_pos = canonical.find("\"context\"").unwrap();
    assert!(
        config_pos < context_pos,
        "canonical JSON should sort top-level keys"
    );
}

#[test]
fn serde_lane_rename_snake_case() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"patch_first\""));
}

#[test]
fn serde_lane_workspace_first_rename() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"workspace_first\""));
}

#[test]
fn serde_workspace_mode_pass_through_rename() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"pass_through\""));
}

#[test]
fn serde_workspace_mode_staged_rename() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"staged\""));
}

#[test]
fn serde_capability_rename_snake_case() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"tool_read\""));
    assert!(json.contains("\"native\""));
}

#[test]
fn serde_context_snippet_roundtrip() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into()],
        snippets: vec![
            ContextSnippet {
                name: "s1".into(),
                content: "content1".into(),
            },
            ContextSnippet {
                name: "s2".into(),
                content: "content2".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.context.files.len(), 2);
    assert_eq!(back.context.snippets.len(), 2);
    assert_eq!(back.context.snippets[0].name, "s1");
    assert_eq!(back.context.snippets[1].content, "content2");
}

#[test]
fn serde_policy_roundtrip() {
    let pol = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/sys/**".into()],
        allow_network: vec!["*.safe.io".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["rm".into()],
    };
    let wo = WorkOrderBuilder::new("t").policy(pol).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.policy.allowed_tools, vec!["read"]);
    assert_eq!(back.policy.disallowed_tools, vec!["bash"]);
    assert_eq!(back.policy.deny_read, vec!["*.secret"]);
    assert_eq!(back.policy.deny_write, vec!["/sys/**"]);
    assert_eq!(back.policy.allow_network, vec!["*.safe.io"]);
    assert_eq!(back.policy.deny_network, vec!["evil.com"]);
    assert_eq!(back.policy.require_approval_for, vec!["rm"]);
}

// =========================================================================
// 4. Config vendor section
// =========================================================================

#[test]
fn vendor_abp_mode_passthrough() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["abp"]["mode"], "passthrough");
}

#[test]
fn vendor_abp_mode_mapped() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "mapped"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["abp"]["mode"], "mapped");
}

#[test]
fn vendor_abp_request_metadata() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        json!({
            "mode": "passthrough",
            "request_id": "req-123",
            "trace_id": "trace-456"
        }),
    );
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["abp"]["request_id"], "req-123");
    assert_eq!(wo.config.vendor["abp"]["trace_id"], "trace-456");
}

#[test]
fn vendor_dotted_key_format() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp.mode".into(), json!("passthrough"));
    vendor.insert("abp.request_id".into(), json!("r1"));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["abp.mode"], json!("passthrough"));
    assert_eq!(wo.config.vendor["abp.request_id"], json!("r1"));
}

#[test]
fn vendor_nested_format() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        json!({
            "mode": "mapped",
            "config": {
                "retry": 3,
                "timeout_ms": 5000
            }
        }),
    );
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["abp"]["config"]["retry"], 3);
    assert_eq!(wo.config.vendor["abp"]["config"]["timeout_ms"], 5000);
}

#[test]
fn vendor_multiple_providers() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.8}));
    vendor.insert("anthropic".into(), json!({"max_tokens": 4096}));
    vendor.insert("abp".into(), json!({"mode": "mapped"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor.len(), 3);
    assert_eq!(wo.config.vendor["openai"]["temperature"], 0.8);
    assert_eq!(wo.config.vendor["anthropic"]["max_tokens"], 4096);
}

#[test]
fn vendor_config_roundtrip() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    vendor.insert("abp.extra".into(), json!("val"));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.config.vendor["abp"]["mode"], "passthrough");
    assert_eq!(back.config.vendor["abp.extra"], "val");
}

#[test]
fn vendor_config_ext_lookup() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.vendor_config("abp").unwrap()["mode"], "passthrough");
    assert!(wo.vendor_config("missing").is_none());
}

// =========================================================================
// 5. Edge Cases
// =========================================================================

#[test]
fn edge_very_long_task() {
    let long_task = "a".repeat(100_000);
    let wo = WorkOrderBuilder::new(&long_task).build();
    assert_eq!(wo.task.len(), 100_000);
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task.len(), 100_000);
}

#[test]
fn edge_unicode_task() {
    let wo = WorkOrderBuilder::new("修复认证模块 🔧").build();
    assert_eq!(wo.task, "修复认证模块 🔧");
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "修复认证模块 🔧");
}

#[test]
fn edge_unicode_emoji_heavy_task() {
    let wo = WorkOrderBuilder::new("🦀🔥💯 Rust is great!").build();
    assert_eq!(wo.task, "🦀🔥💯 Rust is great!");
}

#[test]
fn edge_unicode_rtl_task() {
    let wo = WorkOrderBuilder::new("إصلاح وحدة المصادقة").build();
    assert_eq!(wo.task, "إصلاح وحدة المصادقة");
}

#[test]
fn edge_task_with_newlines() {
    let wo = WorkOrderBuilder::new("line1\nline2\nline3").build();
    assert!(wo.task.contains('\n'));
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "line1\nline2\nline3");
}

#[test]
fn edge_task_with_special_json_chars() {
    let wo = WorkOrderBuilder::new(r#"fix "quotes" and \backslash"#).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, r#"fix "quotes" and \backslash"#);
}

#[test]
fn edge_empty_config_defaults() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn edge_deep_nesting_in_vendor() {
    let deep = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "level5": "deep_value"
                    }
                }
            }
        }
    });
    let mut vendor = BTreeMap::new();
    vendor.insert("nested".into(), deep);
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.config.vendor["nested"]["level1"]["level2"]["level3"]["level4"]["level5"],
        "deep_value"
    );
}

#[test]
fn edge_duplicate_capability_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    // Both entries preserved — dedup is not enforced at type level
    assert_eq!(wo.requirements.required.len(), 2);
}

#[test]
fn edge_many_capabilities() {
    let caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::CodeExecution,
    ];
    let reqs = CapabilityRequirements {
        required: caps
            .into_iter()
            .map(|c| CapabilityRequirement {
                capability: c,
                min_support: MinSupport::Emulated,
            })
            .collect(),
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 12);
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.requirements.required.len(), 12);
}

#[test]
fn edge_empty_include_exclude_globs() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec![])
        .exclude(vec![])
        .build();
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn edge_many_include_patterns() {
    let patterns: Vec<String> = (0..100).map(|i| format!("pattern_{i}/**")).collect();
    let wo = WorkOrderBuilder::new("t").include(patterns.clone()).build();
    assert_eq!(wo.workspace.include.len(), 100);
}

#[test]
fn edge_empty_vendor_object() {
    let mut vendor = BTreeMap::new();
    vendor.insert("empty".into(), json!({}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.config.vendor["empty"], json!({}));
}

#[test]
fn edge_vendor_array_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("list".into(), json!([1, 2, 3]));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.config.vendor["list"], json!([1, 2, 3]));
}

#[test]
fn edge_vendor_null_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("nothing".into(), json!(null));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(back.config.vendor["nothing"].is_null());
}

#[test]
fn edge_vendor_boolean_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("flag".into(), json!(true));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["flag"], json!(true));
}

#[test]
fn edge_vendor_numeric_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("count".into(), json!(42));
    vendor.insert("ratio".into(), json!(3.14));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert_eq!(wo.config.vendor["count"], json!(42));
    assert_eq!(wo.config.vendor["ratio"], json!(3.14));
}

// =========================================================================
// 6. Defaults (ConfigDefaults)
// =========================================================================

#[test]
fn defaults_max_turns() {
    assert_eq!(ConfigDefaults::default_max_turns(), 25);
}

#[test]
fn defaults_max_budget() {
    assert!((ConfigDefaults::default_max_budget() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn defaults_model() {
    assert_eq!(ConfigDefaults::default_model(), "gpt-4");
}

#[test]
fn defaults_apply_fills_missing() {
    let mut wo = minimal();
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(25));
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
}

#[test]
fn defaults_apply_does_not_overwrite() {
    let mut wo = WorkOrderBuilder::new("t")
        .model("claude-3")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

// =========================================================================
// 7. ExecutionMode
// =========================================================================

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_serde() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, "\"passthrough\"");
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_mapped_serde() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, "\"mapped\"");
    let back: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ExecutionMode::Mapped);
}

// =========================================================================
// 8. Extension traits
// =========================================================================

#[test]
fn ext_has_capability_present() {
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
fn ext_has_capability_absent() {
    let wo = minimal();
    assert!(!wo.has_capability(&Capability::Streaming));
}

#[test]
fn ext_tool_budget_remaining_some() {
    let wo = WorkOrderBuilder::new("t").max_turns(10).build();
    assert_eq!(wo.tool_budget_remaining(), Some(10));
}

#[test]
fn ext_tool_budget_remaining_none() {
    let wo = minimal();
    assert!(wo.tool_budget_remaining().is_none());
}

#[test]
fn ext_is_code_task_true() {
    let wo = WorkOrderBuilder::new("fix the login bug").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_false() {
    let wo = WorkOrderBuilder::new("write documentation").build();
    assert!(!wo.is_code_task());
}

#[test]
fn ext_task_summary_truncation() {
    let wo = WorkOrderBuilder::new("a".repeat(200)).build();
    let summary = wo.task_summary(50);
    // task_summary appends "…" (3 bytes) when truncating, so result is up to max_len + 3
    assert!(summary.len() <= 50 + "…".len());
    assert!(summary.ends_with('…'));
}

#[test]
fn ext_task_summary_short_unchanged() {
    let wo = WorkOrderBuilder::new("short").build();
    let summary = wo.task_summary(50);
    assert_eq!(summary, "short");
}

#[test]
fn ext_vendor_config_lookup() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.5}));
    let cfg = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).build();
    assert!(wo.vendor_config("openai").is_some());
    assert!(wo.vendor_config("anthropic").is_none());
}

// =========================================================================
// 9. WorkspaceSpec
// =========================================================================

#[test]
fn workspace_spec_serde_roundtrip() {
    let spec = WorkspaceSpec {
        root: "/my/root".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["*.rs".into()],
        exclude: vec!["target/**".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.root, "/my/root");
    assert!(matches!(back.mode, WorkspaceMode::Staged));
    assert_eq!(back.include, vec!["*.rs"]);
    assert_eq!(back.exclude, vec!["target/**"]);
}

// =========================================================================
// 10. RuntimeConfig
// =========================================================================

#[test]
fn runtime_config_default() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn runtime_config_serde_roundtrip() {
    let mut vendor = BTreeMap::new();
    vendor.insert("key".into(), json!("val"));
    let mut env = BTreeMap::new();
    env.insert("K".into(), "V".into());
    let cfg = RuntimeConfig {
        model: Some("m".into()),
        vendor,
        env,
        max_budget_usd: Some(2.0),
        max_turns: Some(8),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model.as_deref(), Some("m"));
    assert_eq!(back.vendor["key"], "val");
    assert_eq!(back.env["K"], "V");
    assert_eq!(back.max_budget_usd, Some(2.0));
    assert_eq!(back.max_turns, Some(8));
}

// =========================================================================
// 11. ContextPacket and ContextSnippet
// =========================================================================

#[test]
fn context_packet_default() {
    let cp = ContextPacket::default();
    assert!(cp.files.is_empty());
    assert!(cp.snippets.is_empty());
}

#[test]
fn context_snippet_serde_roundtrip() {
    let s = ContextSnippet {
        name: "test".into(),
        content: "fn main() {}".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: ContextSnippet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "test");
    assert_eq!(back.content, "fn main() {}");
}

// =========================================================================
// 12. PolicyProfile
// =========================================================================

#[test]
fn policy_profile_default() {
    let p = PolicyProfile::default();
    assert!(p.allowed_tools.is_empty());
    assert!(p.disallowed_tools.is_empty());
    assert!(p.deny_read.is_empty());
    assert!(p.deny_write.is_empty());
    assert!(p.allow_network.is_empty());
    assert!(p.deny_network.is_empty());
    assert!(p.require_approval_for.is_empty());
}

#[test]
fn policy_profile_serde_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: vec!["a".into()],
        disallowed_tools: vec!["b".into()],
        deny_read: vec!["c".into()],
        deny_write: vec!["d".into()],
        allow_network: vec!["e".into()],
        deny_network: vec!["f".into()],
        require_approval_for: vec!["g".into()],
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(back.allowed_tools, vec!["a"]);
    assert_eq!(back.disallowed_tools, vec!["b"]);
    assert_eq!(back.deny_read, vec!["c"]);
    assert_eq!(back.deny_write, vec!["d"]);
    assert_eq!(back.allow_network, vec!["e"]);
    assert_eq!(back.deny_network, vec!["f"]);
    assert_eq!(back.require_approval_for, vec!["g"]);
}

// =========================================================================
// 13. CapabilityRequirements
// =========================================================================

#[test]
fn capability_requirements_default() {
    let cr = CapabilityRequirements::default();
    assert!(cr.required.is_empty());
}

#[test]
fn capability_requirements_serde_roundtrip() {
    let cr = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let json = serde_json::to_string(&cr).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 2);
}

// =========================================================================
// 14. Deserialization from raw JSON
// =========================================================================

#[test]
fn deserialize_minimal_json() {
    let raw = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "hello",
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
    });
    let wo: WorkOrder = serde_json::from_value(raw).unwrap();
    assert_eq!(wo.task, "hello");
}

#[test]
fn deserialize_rejects_invalid_lane() {
    let raw = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "hello",
        "lane": "invalid_lane",
        "workspace": {"root": ".", "mode": "staged", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": []},
        "requirements": {"required": []},
        "config": {"model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null}
    });
    assert!(serde_json::from_value::<WorkOrder>(raw).is_err());
}

#[test]
fn deserialize_rejects_missing_task() {
    let raw = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "lane": "patch_first",
        "workspace": {"root": ".", "mode": "staged", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": []},
        "requirements": {"required": []},
        "config": {"model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null}
    });
    assert!(serde_json::from_value::<WorkOrder>(raw).is_err());
}

#[test]
fn deserialize_rejects_invalid_workspace_mode() {
    let raw = json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "t",
        "lane": "patch_first",
        "workspace": {"root": ".", "mode": "bogus", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": []},
        "requirements": {"required": []},
        "config": {"model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null}
    });
    assert!(serde_json::from_value::<WorkOrder>(raw).is_err());
}

// =========================================================================
// 15. Hashing-related (canonical JSON)
// =========================================================================

#[test]
fn canonical_json_same_data_same_output() {
    let wo = full();
    let a = abp_core::canonical_json(&wo).unwrap();
    let b = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_different_data_different_output() {
    let a = WorkOrderBuilder::new("task A").build();
    let b = WorkOrderBuilder::new("task B").build();
    let ja = abp_core::canonical_json(&a).unwrap();
    let jb = abp_core::canonical_json(&b).unwrap();
    assert_ne!(ja, jb);
}

#[test]
fn sha256_hex_stable() {
    let h1 = abp_core::sha256_hex(b"hello world");
    let h2 = abp_core::sha256_hex(b"hello world");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn sha256_hex_different_input() {
    let h1 = abp_core::sha256_hex(b"a");
    let h2 = abp_core::sha256_hex(b"b");
    assert_ne!(h1, h2);
}
