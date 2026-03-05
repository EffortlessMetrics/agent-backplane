#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for [`WorkOrder`] construction, validation, and
//! serialization — covering builder ergonomics, field edge cases, serde
//! roundtrips, canonical JSON determinism, schema conformance, and more.

use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::ext::WorkOrderExt;
use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet,
    ExecutionLane, ExecutionMode, MinSupport, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use serde_json::json;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn minimal_wo() -> WorkOrder {
    WorkOrderBuilder::new("do something").build()
}

fn maximal_wo() -> WorkOrder {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    vendor.insert("openai".into(), json!({"temperature": 0.7}));

    let mut env = BTreeMap::new();
    env.insert("RUST_LOG".into(), "debug".into());

    WorkOrderBuilder::new("Refactor the auth module completely")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/workspace")
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
            deny_network: vec!["*.internal".into()],
            require_approval_for: vec!["deploy".into()],
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
            model: Some("claude-3-opus".into()),
            vendor,
            env,
            max_budget_usd: Some(5.0),
            max_turns: Some(50),
        })
        .build()
}

// ===========================================================================
// 1. WorkOrder default construction
// ===========================================================================

#[test]
fn default_construction_sets_non_nil_id() {
    let wo = minimal_wo();
    assert!(!wo.id.is_nil());
}

#[test]
fn default_construction_stores_task() {
    let wo = WorkOrderBuilder::new("my task").build();
    assert_eq!(wo.task, "my task");
}

#[test]
fn default_lane_is_patch_first() {
    let wo = minimal_wo();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn default_workspace_root_is_dot() {
    let wo = minimal_wo();
    assert_eq!(wo.workspace.root, ".");
}

#[test]
fn default_workspace_mode_is_staged() {
    let wo = minimal_wo();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
}

#[test]
fn default_include_is_empty() {
    let wo = minimal_wo();
    assert!(wo.workspace.include.is_empty());
}

#[test]
fn default_exclude_is_empty() {
    let wo = minimal_wo();
    assert!(wo.workspace.exclude.is_empty());
}

#[test]
fn default_context_files_empty() {
    let wo = minimal_wo();
    assert!(wo.context.files.is_empty());
}

#[test]
fn default_context_snippets_empty() {
    let wo = minimal_wo();
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn default_policy_is_permissive() {
    let wo = minimal_wo();
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
    assert!(wo.policy.allow_network.is_empty());
    assert!(wo.policy.deny_network.is_empty());
    assert!(wo.policy.require_approval_for.is_empty());
}

#[test]
fn default_requirements_empty() {
    let wo = minimal_wo();
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn default_model_is_none() {
    let wo = minimal_wo();
    assert!(wo.config.model.is_none());
}

#[test]
fn default_max_turns_is_none() {
    let wo = minimal_wo();
    assert!(wo.config.max_turns.is_none());
}

#[test]
fn default_max_budget_is_none() {
    let wo = minimal_wo();
    assert!(wo.config.max_budget_usd.is_none());
}

#[test]
fn default_vendor_map_empty() {
    let wo = minimal_wo();
    assert!(wo.config.vendor.is_empty());
}

#[test]
fn default_env_map_empty() {
    let wo = minimal_wo();
    assert!(wo.config.env.is_empty());
}

// ===========================================================================
// 2. Builder pattern usage
// ===========================================================================

#[test]
fn builder_chaining_all_methods() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/project")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .context(ContextPacket::default())
        .policy(PolicyProfile::default())
        .requirements(CapabilityRequirements::default())
        .config(RuntimeConfig::default())
        .build();
    assert_eq!(wo.workspace.root, "/project");
}

#[test]
fn builder_model_shorthand() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn builder_max_turns_shorthand() {
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    assert_eq!(wo.config.max_turns, Some(10));
}

#[test]
fn builder_max_budget_shorthand() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(2.5).build();
    assert_eq!(wo.config.max_budget_usd, Some(2.5));
}

#[test]
fn builder_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn builder_workspace_pass_through_mode() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn builder_include_multiple_globs() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["*.rs".into(), "*.toml".into(), "*.md".into()])
        .build();
    assert_eq!(wo.workspace.include.len(), 3);
}

#[test]
fn builder_exclude_multiple_globs() {
    let wo = WorkOrderBuilder::new("t")
        .exclude(vec!["target/**".into(), "node_modules/**".into()])
        .build();
    assert_eq!(wo.workspace.exclude.len(), 2);
}

#[test]
fn builder_context_with_files_and_snippets() {
    let wo = WorkOrderBuilder::new("t")
        .context(ContextPacket {
            files: vec!["a.rs".into(), "b.rs".into()],
            snippets: vec![ContextSnippet {
                name: "s".into(),
                content: "c".into(),
            }],
        })
        .build();
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn builder_full_runtime_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("key".into(), json!("val"));
    let mut env = BTreeMap::new();
    env.insert("FOO".into(), "bar".into());
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            model: Some("m".into()),
            vendor,
            env,
            max_budget_usd: Some(1.0),
            max_turns: Some(5),
        })
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("m"));
    assert_eq!(wo.config.vendor.len(), 1);
    assert_eq!(wo.config.env.len(), 1);
}

#[test]
fn builder_model_overrides_config_model() {
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            model: Some("original".into()),
            ..Default::default()
        })
        .model("override")
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("override"));
}

// ===========================================================================
// 3. Required field validation (ConfigValidator)
// ===========================================================================

#[test]
fn validator_passes_valid_minimal() {
    let wo = minimal_wo();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_passes_valid_maximal() {
    let wo = maximal_wo();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_rejects_empty_task() {
    let mut wo = minimal_wo();
    wo.task = String::new();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w
        .iter()
        .any(|w| w.field == "task" && matches!(w.severity, WarningSeverity::Error)));
}

#[test]
fn validator_rejects_whitespace_task() {
    let mut wo = minimal_wo();
    wo.task = "   \t\n".into();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "task"));
}

#[test]
fn validator_rejects_zero_max_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(0).build();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w
        .iter()
        .any(|w| w.field == "config.max_turns" && matches!(w.severity, WarningSeverity::Error)));
}

#[test]
fn validator_accepts_one_turn() {
    let wo = WorkOrderBuilder::new("t").max_turns(1).build();
    assert!(ConfigValidator::new().validate_work_order(&wo).is_empty());
}

#[test]
fn validator_accepts_large_turns() {
    let wo = WorkOrderBuilder::new("t").max_turns(u32::MAX).build();
    assert!(ConfigValidator::new().validate_work_order(&wo).is_empty());
}

#[test]
fn validator_rejects_zero_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.0).build();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validator_rejects_negative_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(-0.01).build();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validator_accepts_small_positive_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.001).build();
    assert!(ConfigValidator::new().validate_work_order(&wo).is_empty());
}

#[test]
fn validator_rejects_empty_model() {
    let wo = WorkOrderBuilder::new("t").model("").build();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "config.model"));
}

#[test]
fn validator_rejects_whitespace_model() {
    let wo = WorkOrderBuilder::new("t").model("   ").build();
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "config.model"));
}

#[test]
fn validator_warns_duplicate_allowed_tools() {
    let mut wo = minimal_wo();
    wo.policy.allowed_tools = vec!["read".into(), "write".into(), "read".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(
        |w| w.field == "policy.allowed_tools" && matches!(w.severity, WarningSeverity::Warning)
    ));
}

#[test]
fn validator_accepts_unique_tools() {
    let mut wo = minimal_wo();
    wo.policy.allowed_tools = vec!["read".into(), "write".into(), "bash".into()];
    assert!(ConfigValidator::new().validate_work_order(&wo).is_empty());
}

#[test]
fn validator_rejects_empty_vendor_key() {
    let mut wo = minimal_wo();
    wo.config.vendor.insert("".into(), json!("v"));
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "config.vendor"));
}

#[test]
fn validator_rejects_whitespace_vendor_key() {
    let mut wo = minimal_wo();
    wo.config.vendor.insert("  ".into(), json!("v"));
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "config.vendor"));
}

#[test]
fn validator_rejects_empty_deny_read_glob() {
    let mut wo = minimal_wo();
    wo.policy.deny_read = vec!["".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "policy.deny_read"));
}

#[test]
fn validator_rejects_empty_deny_write_glob() {
    let mut wo = minimal_wo();
    wo.policy.deny_write = vec!["  ".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "policy.deny_write"));
}

#[test]
fn validator_rejects_empty_allow_network() {
    let mut wo = minimal_wo();
    wo.policy.allow_network = vec!["".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "policy.allow_network"));
}

#[test]
fn validator_rejects_empty_deny_network() {
    let mut wo = minimal_wo();
    wo.policy.deny_network = vec!["".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "policy.deny_network"));
}

#[test]
fn validator_rejects_empty_disallowed_tool() {
    let mut wo = minimal_wo();
    wo.policy.disallowed_tools = vec!["".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "policy.disallowed_tools"));
}

#[test]
fn validator_rejects_empty_require_approval() {
    let mut wo = minimal_wo();
    wo.policy.require_approval_for = vec!["".into()];
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.iter().any(|w| w.field == "policy.require_approval_for"));
}

#[test]
fn validator_accumulates_multiple_errors() {
    let mut wo = minimal_wo();
    wo.task = "".into();
    wo.config.max_turns = Some(0);
    wo.config.max_budget_usd = Some(-1.0);
    wo.config.model = Some("".into());
    let w = ConfigValidator::new().validate_work_order(&wo);
    assert!(w.len() >= 4);
}

// ===========================================================================
// 4. Optional field defaults
// ===========================================================================

#[test]
fn config_defaults_fills_missing_turns() {
    let mut wo = minimal_wo();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.max_turns,
        Some(ConfigDefaults::default_max_turns())
    );
}

#[test]
fn config_defaults_fills_missing_budget() {
    let mut wo = minimal_wo();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.max_budget_usd,
        Some(ConfigDefaults::default_max_budget())
    );
}

#[test]
fn config_defaults_fills_missing_model() {
    let mut wo = minimal_wo();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(
        wo.config.model.as_deref(),
        Some(ConfigDefaults::default_model())
    );
}

#[test]
fn config_defaults_preserves_existing_turns() {
    let mut wo = WorkOrderBuilder::new("t").max_turns(99).build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.max_turns, Some(99));
}

#[test]
fn config_defaults_preserves_existing_budget() {
    let mut wo = WorkOrderBuilder::new("t").max_budget_usd(42.0).build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.max_budget_usd, Some(42.0));
}

#[test]
fn config_defaults_preserves_existing_model() {
    let mut wo = WorkOrderBuilder::new("t").model("custom").build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.model.as_deref(), Some("custom"));
}

#[test]
fn config_defaults_values_are_sane() {
    assert!(ConfigDefaults::default_max_turns() > 0);
    assert!(ConfigDefaults::default_max_budget() > 0.0);
    assert!(!ConfigDefaults::default_model().is_empty());
}

// ===========================================================================
// 5. Task string edge cases
// ===========================================================================

#[test]
fn task_empty_string() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn task_single_char() {
    let wo = WorkOrderBuilder::new("x").build();
    assert_eq!(wo.task, "x");
}

#[test]
fn task_unicode_cjk() {
    let task = "修复认证模块";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_unicode_arabic() {
    let task = "تصحيح الخطأ في الوحدة";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_emoji() {
    let task = "🚀 Fix the 🐛 in auth 🔒";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_very_long_10k() {
    let task = "a".repeat(10_000);
    let wo = WorkOrderBuilder::new(task.clone()).build();
    assert_eq!(wo.task.len(), 10_000);
}

#[test]
fn task_very_long_100k() {
    let task = "x".repeat(100_000);
    let wo = WorkOrderBuilder::new(task.clone()).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_newlines() {
    let task = "line1\nline2\nline3";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_carriage_return() {
    let task = "line1\r\nline2\r\n";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_tabs() {
    let task = "\ttabbed\tcontent\t";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_null_byte() {
    let task = "before\0after";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_backslashes() {
    let task = r"C:\Users\test\path";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_quotes() {
    let task = r#"Fix "this" and 'that'"#;
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_html_entities() {
    let task = "Fix <div> &amp; &lt;module&gt;";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_with_json_content() {
    let task = r#"Parse {"key": "value", "num": 42}"#;
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

#[test]
fn task_only_whitespace() {
    let wo = WorkOrderBuilder::new("   ").build();
    assert_eq!(wo.task, "   ");
}

// ===========================================================================
// 6. Backend hint validation (model field)
// ===========================================================================

#[test]
fn model_none_by_default() {
    let wo = minimal_wo();
    assert!(wo.config.model.is_none());
}

#[test]
fn model_set_via_builder() {
    let wo = WorkOrderBuilder::new("t").model("gpt-4-turbo").build();
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn model_with_vendor_prefix() {
    let wo = WorkOrderBuilder::new("t")
        .model("anthropic/claude-3-opus")
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("anthropic/claude-3-opus"));
}

#[test]
fn model_with_special_chars() {
    let wo = WorkOrderBuilder::new("t").model("model-v2.1_beta").build();
    assert_eq!(wo.config.model.as_deref(), Some("model-v2.1_beta"));
}

#[test]
fn model_empty_string_allowed_by_builder() {
    // Builder allows it, but validator will reject it
    let wo = WorkOrderBuilder::new("t").model("").build();
    assert_eq!(wo.config.model.as_deref(), Some(""));
}

#[test]
fn model_roundtrips_through_json() {
    let wo = WorkOrderBuilder::new("t")
        .model("claude-3.5-sonnet")
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.model, wo.config.model);
}

// ===========================================================================
// 7. Config vendor metadata access
// ===========================================================================

#[test]
fn vendor_empty_by_default() {
    let wo = minimal_wo();
    assert!(wo.vendor_config("anything").is_none());
}

#[test]
fn vendor_single_key_access() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.5}));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let v = wo.vendor_config("openai").unwrap();
    assert_eq!(v["temperature"], 0.5);
}

#[test]
fn vendor_nested_abp_metadata() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".into(),
        json!({"mode": "passthrough", "version": "0.1"}),
    );
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let abp = wo.vendor_config("abp").unwrap();
    assert_eq!(abp["mode"], "passthrough");
    assert_eq!(abp["version"], "0.1");
}

#[test]
fn vendor_deeply_nested_access() {
    let mut vendor = BTreeMap::new();
    vendor.insert("deep".into(), json!({"a": {"b": {"c": 99}}}));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let v = wo.vendor_config("deep").unwrap();
    assert_eq!(v["a"]["b"]["c"], 99);
}

#[test]
fn vendor_multiple_keys_all_accessible() {
    let mut vendor = BTreeMap::new();
    vendor.insert("a".into(), json!(1));
    vendor.insert("b".into(), json!(2));
    vendor.insert("c".into(), json!(3));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(*wo.vendor_config("a").unwrap(), json!(1));
    assert_eq!(*wo.vendor_config("b").unwrap(), json!(2));
    assert_eq!(*wo.vendor_config("c").unwrap(), json!(3));
}

#[test]
fn vendor_null_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("k".into(), serde_json::Value::Null);
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert!(wo.vendor_config("k").unwrap().is_null());
}

#[test]
fn vendor_array_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("stops".into(), json!([".", "!", "?"]));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let arr = wo.vendor_config("stops").unwrap().as_array().unwrap();
    assert_eq!(arr.len(), 3);
}

#[test]
fn vendor_boolean_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("streaming".into(), json!(true));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(*wo.vendor_config("streaming").unwrap(), json!(true));
}

#[test]
fn vendor_numeric_types() {
    let mut vendor = BTreeMap::new();
    vendor.insert("int_val".into(), json!(42));
    vendor.insert("float_val".into(), json!(2.72));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.vendor_config("int_val").unwrap().as_i64(), Some(42));
    assert!((wo.vendor_config("float_val").unwrap().as_f64().unwrap() - 2.72).abs() < f64::EPSILON);
}

// ===========================================================================
// 8. Capabilities list construction
// ===========================================================================

#[test]
fn no_capabilities_by_default() {
    let wo = minimal_wo();
    assert!(wo.requirements.required.is_empty());
    assert!(!wo.has_capability(&Capability::ToolRead));
}

#[test]
fn single_capability_native() {
    let wo = WorkOrderBuilder::new("t")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    assert!(wo.has_capability(&Capability::ToolRead));
}

#[test]
fn single_capability_emulated() {
    let wo = WorkOrderBuilder::new("t")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        })
        .build();
    assert!(wo.has_capability(&Capability::Streaming));
}

#[test]
fn multiple_capabilities() {
    let caps = vec![
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::Streaming,
    ];
    let wo = WorkOrderBuilder::new("t")
        .requirements(CapabilityRequirements {
            required: caps
                .iter()
                .map(|c| CapabilityRequirement {
                    capability: c.clone(),
                    min_support: MinSupport::Native,
                })
                .collect(),
        })
        .build();
    for cap in &caps {
        assert!(wo.has_capability(cap), "missing: {cap:?}");
    }
}

#[test]
fn has_capability_false_for_unset() {
    let wo = WorkOrderBuilder::new("t")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    assert!(!wo.has_capability(&Capability::ToolWrite));
    assert!(!wo.has_capability(&Capability::McpClient));
}

#[test]
fn all_tool_capabilities() {
    let tool_caps = vec![
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
    ];
    let wo = WorkOrderBuilder::new("t")
        .requirements(CapabilityRequirements {
            required: tool_caps
                .iter()
                .map(|c| CapabilityRequirement {
                    capability: c.clone(),
                    min_support: MinSupport::Native,
                })
                .collect(),
        })
        .build();
    assert_eq!(wo.requirements.required.len(), 9);
}

// ===========================================================================
// 9. Policy attachment
// ===========================================================================

#[test]
fn default_policy_is_empty() {
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
fn policy_with_allowed_tools() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.allowed_tools, vec!["read", "write"]);
}

#[test]
fn policy_with_disallowed_tools() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            disallowed_tools: vec!["bash".into(), "exec".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.disallowed_tools.len(), 2);
}

#[test]
fn policy_with_deny_read_globs() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            deny_read: vec!["*.key".into(), "*.pem".into(), ".env*".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.deny_read.len(), 3);
}

#[test]
fn policy_with_deny_write_globs() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            deny_write: vec!["/etc/**".into(), "/usr/**".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.deny_write.len(), 2);
}

#[test]
fn policy_with_network_rules() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["*.evil.com".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.allow_network.len(), 1);
    assert_eq!(wo.policy.deny_network.len(), 1);
}

#[test]
fn policy_with_approval_required() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            require_approval_for: vec!["deploy".into(), "delete".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.require_approval_for.len(), 2);
}

#[test]
fn policy_serde_roundtrip() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/root/**".into()],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    };
    let json = serde_json::to_string(&policy).unwrap();
    let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.allowed_tools, policy.allowed_tools);
    assert_eq!(rt.deny_read, policy.deny_read);
    assert_eq!(rt.deny_network, policy.deny_network);
}

// ===========================================================================
// 10. WorkOrder serde roundtrip
// ===========================================================================

#[test]
fn serde_roundtrip_minimal_json() {
    let wo = minimal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
}

#[test]
fn serde_roundtrip_maximal_json() {
    let wo = maximal_wo();
    let json = serde_json::to_string_pretty(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.workspace.root, wo2.workspace.root);
    assert_eq!(wo.config.model, wo2.config.model);
    assert_eq!(wo.config.max_turns, wo2.config.max_turns);
    assert_eq!(wo.config.max_budget_usd, wo2.config.max_budget_usd);
    assert_eq!(wo.config.vendor, wo2.config.vendor);
    assert_eq!(wo.config.env, wo2.config.env);
    assert_eq!(wo.policy.allowed_tools, wo2.policy.allowed_tools);
}

#[test]
fn serde_roundtrip_preserves_vendor() {
    let wo = maximal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.config.vendor, wo2.config.vendor);
}

#[test]
fn serde_roundtrip_preserves_env() {
    let wo = maximal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.config.env, wo2.config.env);
}

#[test]
fn serde_roundtrip_preserves_context() {
    let wo = maximal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.files, vec!["src/main.rs"]);
    assert_eq!(wo2.context.snippets[0].name, "hint");
    assert_eq!(wo2.context.snippets[0].content, "Use JWT tokens");
}

#[test]
fn serde_value_roundtrip() {
    let wo = maximal_wo();
    let value = serde_json::to_value(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_value(value).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
}

#[test]
fn serde_unicode_task_roundtrip() {
    let wo = WorkOrderBuilder::new("修复认证模块 🔧 αβγ").build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "修复认证模块 🔧 αβγ");
}

#[test]
fn serde_long_task_roundtrip() {
    let task = "x".repeat(50_000);
    let wo = WorkOrderBuilder::new(task.clone()).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn serde_null_byte_roundtrip() {
    let wo = WorkOrderBuilder::new("a\0b").build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "a\0b");
}

#[test]
fn deserialize_known_json() {
    let raw = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "hello",
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
        "config": { "model": null, "vendor": {}, "env": {},
                     "max_budget_usd": null, "max_turns": null }
    }"#;
    let wo: WorkOrder = serde_json::from_str(raw).unwrap();
    assert_eq!(wo.task, "hello");
    assert_eq!(wo.id.to_string(), "00000000-0000-0000-0000-000000000001");
}

#[test]
fn deserialize_rejects_missing_task() {
    let raw = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
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
        "config": { "model": null, "vendor": {}, "env": {},
                     "max_budget_usd": null, "max_turns": null }
    }"#;
    assert!(serde_json::from_str::<WorkOrder>(raw).is_err());
}

#[test]
fn deserialize_rejects_missing_id() {
    let raw = r#"{
        "task": "hello",
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
        "config": { "model": null, "vendor": {}, "env": {},
                     "max_budget_usd": null, "max_turns": null }
    }"#;
    assert!(serde_json::from_str::<WorkOrder>(raw).is_err());
}

#[test]
fn deserialize_rejects_invalid_lane() {
    let raw = r#""nonexistent_lane""#;
    assert!(serde_json::from_str::<ExecutionLane>(raw).is_err());
}

#[test]
fn deserialize_rejects_invalid_workspace_mode() {
    let raw = r#""bad_mode""#;
    assert!(serde_json::from_str::<WorkspaceMode>(raw).is_err());
}

// ===========================================================================
// 11. WorkOrder canonical JSON (deterministic field ordering)
// ===========================================================================

#[test]
fn canonical_json_deterministic() {
    let wo = maximal_wo();
    let a = abp_core::canonical_json(&wo).unwrap();
    let b = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(a, b);
}

#[test]
fn canonical_json_deterministic_repeated() {
    let wo = maximal_wo();
    let jsons: Vec<String> = (0..10)
        .map(|_| abp_core::canonical_json(&wo).unwrap())
        .collect();
    for j in &jsons {
        assert_eq!(j, &jsons[0]);
    }
}

#[test]
fn canonical_json_vendor_keys_sorted() {
    let mut vendor = BTreeMap::new();
    vendor.insert("zebra".into(), json!(1));
    vendor.insert("apple".into(), json!(2));
    vendor.insert("mango".into(), json!(3));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = abp_core::canonical_json(&wo).unwrap();
    let a_pos = json.find("\"apple\"").unwrap();
    let m_pos = json.find("\"mango\"").unwrap();
    let z_pos = json.find("\"zebra\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn canonical_json_env_keys_sorted() {
    let mut env = BTreeMap::new();
    env.insert("Z_VAR".into(), "z".into());
    env.insert("A_VAR".into(), "a".into());
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            env,
            ..Default::default()
        })
        .build();
    let json = abp_core::canonical_json(&wo).unwrap();
    let a_pos = json.find("\"A_VAR\"").unwrap();
    let z_pos = json.find("\"Z_VAR\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn serialized_json_contains_all_top_level_keys() {
    let wo = minimal_wo();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
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

#[test]
fn serialized_json_config_keys() {
    let wo = minimal_wo();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let config = v["config"].as_object().unwrap();
    for key in &["model", "vendor", "env", "max_budget_usd", "max_turns"] {
        assert!(config.contains_key(*key), "missing config key: {key}");
    }
}

#[test]
fn serialized_json_workspace_keys() {
    let wo = minimal_wo();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let ws = v["workspace"].as_object().unwrap();
    for key in &["root", "mode", "include", "exclude"] {
        assert!(ws.contains_key(*key), "missing workspace key: {key}");
    }
}

// ===========================================================================
// 12. WorkOrder with all fields populated
// ===========================================================================

#[test]
fn maximal_wo_has_task() {
    let wo = maximal_wo();
    assert_eq!(wo.task, "Refactor the auth module completely");
}

#[test]
fn maximal_wo_has_workspace() {
    let wo = maximal_wo();
    assert_eq!(wo.workspace.root, "/tmp/workspace");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn maximal_wo_has_context() {
    let wo = maximal_wo();
    assert_eq!(wo.context.files, vec!["src/main.rs"]);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn maximal_wo_has_policy() {
    let wo = maximal_wo();
    assert_eq!(wo.policy.allowed_tools.len(), 2);
    assert_eq!(wo.policy.disallowed_tools.len(), 1);
    assert_eq!(wo.policy.deny_read.len(), 1);
    assert_eq!(wo.policy.deny_write.len(), 1);
    assert_eq!(wo.policy.allow_network.len(), 1);
    assert_eq!(wo.policy.deny_network.len(), 1);
    assert_eq!(wo.policy.require_approval_for.len(), 1);
}

#[test]
fn maximal_wo_has_requirements() {
    let wo = maximal_wo();
    assert_eq!(wo.requirements.required.len(), 2);
}

#[test]
fn maximal_wo_has_config() {
    let wo = maximal_wo();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3-opus"));
    assert_eq!(wo.config.max_turns, Some(50));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.config.vendor.len(), 2);
    assert_eq!(wo.config.env.len(), 1);
}

#[test]
fn maximal_wo_serialization_size() {
    let min_json = serde_json::to_string(&minimal_wo()).unwrap();
    let max_json = serde_json::to_string(&maximal_wo()).unwrap();
    assert!(max_json.len() > min_json.len());
}

// ===========================================================================
// 13. WorkOrder cloning/equality
// ===========================================================================

#[test]
fn clone_preserves_id() {
    let wo = maximal_wo();
    let c = wo.clone();
    assert_eq!(wo.id, c.id);
}

#[test]
fn clone_preserves_task() {
    let wo = maximal_wo();
    let c = wo.clone();
    assert_eq!(wo.task, c.task);
}

#[test]
fn clone_preserves_config() {
    let wo = maximal_wo();
    let c = wo.clone();
    assert_eq!(wo.config.model, c.config.model);
    assert_eq!(wo.config.max_turns, c.config.max_turns);
    assert_eq!(wo.config.max_budget_usd, c.config.max_budget_usd);
    assert_eq!(wo.config.vendor, c.config.vendor);
    assert_eq!(wo.config.env, c.config.env);
}

#[test]
fn clone_preserves_policy() {
    let wo = maximal_wo();
    let c = wo.clone();
    assert_eq!(wo.policy.allowed_tools, c.policy.allowed_tools);
    assert_eq!(wo.policy.deny_read, c.policy.deny_read);
}

#[test]
fn clone_preserves_workspace() {
    let wo = maximal_wo();
    let c = wo.clone();
    assert_eq!(wo.workspace.root, c.workspace.root);
    assert_eq!(wo.workspace.include, c.workspace.include);
    assert_eq!(wo.workspace.exclude, c.workspace.exclude);
}

#[test]
fn clone_is_independent_mutation() {
    let wo = maximal_wo();
    let mut c = wo.clone();
    c.task = "modified".into();
    assert_ne!(wo.task, c.task);
    assert_eq!(wo.task, "Refactor the auth module completely");
}

#[test]
fn clone_serializes_identically() {
    let wo = maximal_wo();
    let c = wo.clone();
    let j1 = serde_json::to_string(&wo).unwrap();
    let j2 = serde_json::to_string(&c).unwrap();
    assert_eq!(j1, j2);
}

// ===========================================================================
// 14. WorkOrder ID uniqueness
// ===========================================================================

#[test]
fn two_builders_produce_different_ids() {
    let a = WorkOrderBuilder::new("same").build();
    let b = WorkOrderBuilder::new("same").build();
    assert_ne!(a.id, b.id);
}

#[test]
fn ten_work_orders_all_unique() {
    let ids: Vec<_> = (0..10).map(|_| minimal_wo().id).collect();
    let mut dedup = ids.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(ids.len(), dedup.len());
}

#[test]
fn hundred_work_orders_all_unique() {
    let ids: Vec<_> = (0..100).map(|_| minimal_wo().id).collect();
    let set: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(set.len(), 100);
}

#[test]
fn id_is_v4_uuid() {
    let wo = minimal_wo();
    assert_eq!(wo.id.get_version_num(), 4);
}

#[test]
fn id_is_not_nil() {
    let wo = minimal_wo();
    assert!(!wo.id.is_nil());
}

#[test]
fn id_string_format() {
    let wo = minimal_wo();
    let s = wo.id.to_string();
    // UUID v4 format: 8-4-4-4-12
    assert_eq!(s.len(), 36);
    assert_eq!(s.chars().filter(|c| *c == '-').count(), 4);
}

// ===========================================================================
// 15. WorkOrder to JSON schema conformance
// ===========================================================================

#[test]
fn schema_can_be_generated() {
    let schema = schemars::schema_for!(WorkOrder);
    let v = serde_json::to_value(&schema).unwrap();
    assert!(v.is_object());
}

#[test]
fn minimal_wo_passes_schema() {
    let schema = schemars::schema_for!(WorkOrder);
    let sv = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&sv).unwrap();
    let wo_val = serde_json::to_value(minimal_wo()).unwrap();
    assert!(validator.validate(&wo_val).is_ok());
}

#[test]
fn maximal_wo_passes_schema() {
    let schema = schemars::schema_for!(WorkOrder);
    let sv = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&sv).unwrap();
    let wo_val = serde_json::to_value(maximal_wo()).unwrap();
    assert!(validator.validate(&wo_val).is_ok());
}

#[test]
fn empty_object_fails_schema() {
    let schema = schemars::schema_for!(WorkOrder);
    let sv = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&sv).unwrap();
    assert!(validator.validate(&json!({})).is_err());
}

#[test]
fn partial_object_fails_schema() {
    let schema = schemars::schema_for!(WorkOrder);
    let sv = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&sv).unwrap();
    assert!(validator.validate(&json!({"task": "hello"})).is_err());
}

#[test]
fn schema_includes_capability_enum() {
    let schema = schemars::schema_for!(WorkOrder);
    let json_str = serde_json::to_string(&schema).unwrap();
    assert!(json_str.contains("tool_read"));
    assert!(json_str.contains("streaming"));
}

#[test]
fn schema_includes_execution_lane() {
    let schema = schemars::schema_for!(WorkOrder);
    let json_str = serde_json::to_string(&schema).unwrap();
    assert!(json_str.contains("patch_first"));
    assert!(json_str.contains("workspace_first"));
}

// ===========================================================================
// Bonus: Serde enum serialization
// ===========================================================================

#[test]
fn execution_lane_serde_snake_case() {
    assert_eq!(
        serde_json::to_string(&ExecutionLane::PatchFirst).unwrap(),
        r#""patch_first""#
    );
    assert_eq!(
        serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap(),
        r#""workspace_first""#
    );
}

#[test]
fn workspace_mode_serde_snake_case() {
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::PassThrough).unwrap(),
        r#""pass_through""#
    );
    assert_eq!(
        serde_json::to_string(&WorkspaceMode::Staged).unwrap(),
        r#""staged""#
    );
}

#[test]
fn min_support_serde_snake_case() {
    assert_eq!(
        serde_json::to_string(&MinSupport::Native).unwrap(),
        r#""native""#
    );
    assert_eq!(
        serde_json::to_string(&MinSupport::Emulated).unwrap(),
        r#""emulated""#
    );
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_serde() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, r#""passthrough""#);
    let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ExecutionMode::Passthrough);
}

#[test]
fn all_capabilities_serialize_snake_case() {
    let pairs = vec![
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
    for (cap, expected) in pairs {
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, format!("\"{expected}\""), "mismatch for {cap:?}");
    }
}

// ===========================================================================
// Bonus: Extension trait coverage
// ===========================================================================

#[test]
fn ext_is_code_task_positive_keywords() {
    for kw in &["code", "fix", "implement", "refactor"] {
        let wo = WorkOrderBuilder::new(format!("Please {kw} it")).build();
        assert!(wo.is_code_task(), "expected code task for keyword '{kw}'");
    }
}

#[test]
fn ext_is_code_task_negative() {
    let wo = WorkOrderBuilder::new("Write documentation").build();
    assert!(!wo.is_code_task());
}

#[test]
fn ext_task_summary_short_text() {
    let wo = WorkOrderBuilder::new("short").build();
    assert_eq!(wo.task_summary(100), "short");
}

#[test]
fn ext_task_summary_truncation() {
    let wo = WorkOrderBuilder::new("a very long task description").build();
    let s = wo.task_summary(10);
    assert!(s.ends_with('…'));
    assert!(s.len() <= 14); // 10 + up to 3 bytes for '…'
}

#[test]
fn ext_task_summary_unicode_safe() {
    let wo = WorkOrderBuilder::new("héllo wörld").build();
    let s = wo.task_summary(5);
    assert!(s.ends_with('…'));
}

#[test]
fn ext_tool_budget_none_by_default() {
    assert_eq!(minimal_wo().tool_budget_remaining(), None);
}

#[test]
fn ext_tool_budget_some() {
    let wo = WorkOrderBuilder::new("t").max_turns(7).build();
    assert_eq!(wo.tool_budget_remaining(), Some(7));
}

#[test]
fn ext_vendor_config_found() {
    let wo = maximal_wo();
    assert!(wo.vendor_config("abp").is_some());
    assert!(wo.vendor_config("openai").is_some());
}

#[test]
fn ext_vendor_config_missing() {
    let wo = maximal_wo();
    assert!(wo.vendor_config("nonexistent").is_none());
}

#[test]
fn ext_required_caps_inferred_edit() {
    let wo = WorkOrderBuilder::new("Edit the file").build();
    assert!(wo.required_capabilities().contains(&Capability::ToolEdit));
}

#[test]
fn ext_required_caps_inferred_grep() {
    let wo = WorkOrderBuilder::new("Search for the pattern").build();
    assert!(wo.required_capabilities().contains(&Capability::ToolGrep));
}

#[test]
fn ext_required_caps_inferred_bash() {
    let wo = WorkOrderBuilder::new("Run a shell command").build();
    assert!(wo.required_capabilities().contains(&Capability::ToolBash));
}

#[test]
fn ext_required_caps_explicit_plus_inferred() {
    let wo = WorkOrderBuilder::new("Search and edit files")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolRead));
    assert!(caps.contains(&Capability::ToolGrep));
    assert!(caps.contains(&Capability::ToolEdit));
}

// ===========================================================================
// Bonus: CONTRACT_VERSION and Debug
// ===========================================================================

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn debug_format_contains_task() {
    let wo = WorkOrderBuilder::new("debug test").build();
    let debug = format!("{wo:?}");
    assert!(debug.contains("debug test"));
}

#[test]
fn debug_format_contains_struct_name() {
    let debug = format!("{:?}", minimal_wo());
    assert!(debug.contains("WorkOrder"));
}

#[test]
fn debug_format_contains_id() {
    let wo = minimal_wo();
    let debug = format!("{wo:?}");
    assert!(debug.contains(&wo.id.to_string()));
}
