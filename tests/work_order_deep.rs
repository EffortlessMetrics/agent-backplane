// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for [`WorkOrder`] construction, serialization,
//! validation, and edge cases.

use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::ext::WorkOrderExt;
use abp_core::{
    CONTRACT_VERSION, Capability, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, PolicyProfile, RuntimeConfig,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use serde_json::json;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal work order through the builder.
fn minimal_wo() -> WorkOrder {
    WorkOrderBuilder::new("do something").build()
}

/// Build a maximal work order with every field populated.
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
// 1. WorkOrder construction with all fields
// ===========================================================================

#[test]
fn construct_minimal_work_order() {
    let wo = minimal_wo();
    assert_eq!(wo.task, "do something");
    assert!(!wo.id.is_nil());
}

#[test]
fn construct_maximal_work_order() {
    let wo = maximal_wo();
    assert_eq!(wo.task, "Refactor the auth module completely");
    assert_eq!(wo.workspace.root, "/tmp/workspace");
    assert_eq!(wo.config.model.as_deref(), Some("claude-3-opus"));
    assert_eq!(wo.config.max_turns, Some(50));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
    assert_eq!(wo.policy.allowed_tools.len(), 2);
    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn builder_defaults() {
    let wo = WorkOrderBuilder::new("test").build();
    // Default lane is PatchFirst
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    // Default workspace root is "."
    assert_eq!(wo.workspace.root, ".");
    // Default workspace mode is Staged
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    // No include/exclude by default
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    // No context by default
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    // Empty policy
    assert!(wo.policy.allowed_tools.is_empty());
    // No config
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.vendor.is_empty());
    assert!(wo.config.env.is_empty());
    // No requirements
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn builder_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn builder_lane_patch_first() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::PatchFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn builder_workspace_pass_through() {
    let wo = WorkOrderBuilder::new("task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
}

#[test]
fn builder_include_exclude_globs() {
    let wo = WorkOrderBuilder::new("task")
        .include(vec!["src/**/*.rs".into(), "Cargo.toml".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(wo.workspace.include.len(), 2);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
}

#[test]
fn each_work_order_gets_unique_id() {
    let a = minimal_wo();
    let b = minimal_wo();
    assert_ne!(a.id, b.id);
}

// ===========================================================================
// 2. Serde roundtrip (JSON serialization/deserialization)
// ===========================================================================

#[test]
fn serde_roundtrip_minimal() {
    let wo = minimal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
}

#[test]
fn serde_roundtrip_maximal() {
    let wo = maximal_wo();
    let json = serde_json::to_string_pretty(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.workspace.root, wo2.workspace.root);
    assert_eq!(wo.config.model, wo2.config.model);
    assert_eq!(wo.config.max_turns, wo2.config.max_turns);
    assert_eq!(wo.config.max_budget_usd, wo2.config.max_budget_usd);
    assert_eq!(wo.policy.allowed_tools, wo2.policy.allowed_tools);
    assert_eq!(
        wo.requirements.required.len(),
        wo2.requirements.required.len()
    );
}

#[test]
fn serde_roundtrip_preserves_vendor_values() {
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
fn serde_roundtrip_preserves_context_snippets() {
    let wo = maximal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.snippets[0].name, "hint");
    assert_eq!(wo2.context.snippets[0].content, "Use JWT tokens");
}

#[test]
fn deserialize_from_known_json() {
    let raw = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "task": "hello world",
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
    }"#;
    let wo: WorkOrder = serde_json::from_str(raw).unwrap();
    assert_eq!(wo.task, "hello world");
    assert_eq!(wo.id.to_string(), "00000000-0000-0000-0000-000000000001");
}

#[test]
fn serialized_json_contains_expected_keys() {
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

// ===========================================================================
// 3. WorkOrder with various config.vendor settings
// ===========================================================================

#[test]
fn vendor_config_empty() {
    let wo = minimal_wo();
    assert!(wo.config.vendor.is_empty());
}

#[test]
fn vendor_config_single_key() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.5}));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor.len(), 1);
    assert_eq!(wo.config.vendor["openai"]["temperature"], 0.5);
}

#[test]
fn vendor_config_multiple_keys() {
    let mut vendor = BTreeMap::new();
    vendor.insert("anthropic".into(), json!({"max_tokens": 4096}));
    vendor.insert("openai".into(), json!({"temperature": 0.7}));
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor.len(), 3);
}

#[test]
fn vendor_config_nested_objects() {
    let mut vendor = BTreeMap::new();
    vendor.insert("deep".into(), json!({"a": {"b": {"c": {"d": 42}}}}));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["deep"]["a"]["b"]["c"]["d"], 42);
}

#[test]
fn vendor_config_array_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("stops".into(), json!([".", "!", "?"]));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["stops"].as_array().unwrap().len(), 3);
}

#[test]
fn vendor_config_null_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("key".into(), serde_json::Value::Null);
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(wo2.config.vendor["key"].is_null());
}

// ===========================================================================
// 4. ABP mode settings (passthrough vs mapped)
// ===========================================================================

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_passthrough_serde() {
    let mode = ExecutionMode::Passthrough;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""passthrough""#);
    let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ExecutionMode::Passthrough);
}

#[test]
fn execution_mode_mapped_serde() {
    let mode = ExecutionMode::Mapped;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""mapped""#);
    let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ExecutionMode::Mapped);
}

#[test]
fn vendor_abp_mode_passthrough_in_work_order() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor["abp"]["mode"], "passthrough");
}

#[test]
fn vendor_abp_mode_mapped_in_work_order() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "mapped"}));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor["abp"]["mode"], "mapped");
}

// ===========================================================================
// 5. WorkOrder minimal vs maximal
// ===========================================================================

#[test]
fn minimal_json_is_compact() {
    let wo = minimal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    // Minimal should still serialize all fields
    assert!(json.contains("\"task\":\"do something\""));
    assert!(json.contains("\"vendor\":{}"));
}

#[test]
fn maximal_json_is_larger() {
    let min_json = serde_json::to_string(&minimal_wo()).unwrap();
    let max_json = serde_json::to_string(&maximal_wo()).unwrap();
    assert!(max_json.len() > min_json.len());
}

#[test]
fn minimal_has_empty_collections() {
    let wo = minimal_wo();
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.config.vendor.is_empty());
    assert!(wo.config.env.is_empty());
}

#[test]
fn maximal_has_populated_collections() {
    let wo = maximal_wo();
    assert!(!wo.workspace.include.is_empty());
    assert!(!wo.workspace.exclude.is_empty());
    assert!(!wo.context.files.is_empty());
    assert!(!wo.context.snippets.is_empty());
    assert!(!wo.policy.allowed_tools.is_empty());
    assert!(!wo.policy.disallowed_tools.is_empty());
    assert!(!wo.requirements.required.is_empty());
    assert!(!wo.config.vendor.is_empty());
    assert!(!wo.config.env.is_empty());
}

// ===========================================================================
// 6. Task field variations
// ===========================================================================

#[test]
fn task_simple_string() {
    let wo = WorkOrderBuilder::new("hello").build();
    assert_eq!(wo.task, "hello");
}

#[test]
fn task_with_newlines() {
    let wo = WorkOrderBuilder::new("line1\nline2\nline3").build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "line1\nline2\nline3");
}

#[test]
fn task_with_special_characters() {
    let wo = WorkOrderBuilder::new(r#"Fix the "bug" in <module> & test"#).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, r#"Fix the "bug" in <module> & test"#);
}

#[test]
fn task_with_json_inside() {
    let task = r#"Parse this: {"key": "value"}"#;
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

// ===========================================================================
// 7. Config field validation (ConfigValidator)
// ===========================================================================

#[test]
fn validator_accepts_valid_work_order() {
    let wo = WorkOrderBuilder::new("valid task").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_rejects_empty_task() {
    let mut wo = minimal_wo();
    wo.task = String::new();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task"));
}

#[test]
fn validator_rejects_whitespace_only_task() {
    let mut wo = minimal_wo();
    wo.task = "   \t\n  ".into();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "task"));
}

#[test]
fn validator_rejects_zero_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(0).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "config.max_turns" && matches!(w.severity, WarningSeverity::Error))
    );
}

#[test]
fn validator_accepts_positive_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(10).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_rejects_zero_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.0).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validator_rejects_negative_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(-1.0).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validator_accepts_positive_budget() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(1.5).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_warns_duplicate_tools() {
    let mut wo = minimal_wo();
    wo.policy.allowed_tools = vec!["read".into(), "read".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(
        |w| w.field == "policy.allowed_tools" && matches!(w.severity, WarningSeverity::Warning)
    ));
}

#[test]
fn validator_rejects_empty_model_name() {
    let wo = WorkOrderBuilder::new("task").model("  ").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.model"));
}

#[test]
fn validator_rejects_empty_vendor_key() {
    let mut wo = minimal_wo();
    wo.config.vendor.insert("".into(), json!("val"));
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.vendor"));
}

#[test]
fn validator_rejects_empty_glob_in_deny_read() {
    let mut wo = minimal_wo();
    wo.policy.deny_read = vec!["".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_read"));
}

#[test]
fn validator_rejects_empty_glob_in_deny_write() {
    let mut wo = minimal_wo();
    wo.policy.deny_write = vec!["  ".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_write"));
}

// ===========================================================================
// 8. BTreeMap ordering in vendor config
// ===========================================================================

#[test]
fn vendor_config_keys_are_sorted() {
    let mut vendor = BTreeMap::new();
    vendor.insert("zebra".into(), json!(1));
    vendor.insert("apple".into(), json!(2));
    vendor.insert("mango".into(), json!(3));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let keys: Vec<&String> = wo.config.vendor.keys().collect();
    assert_eq!(keys, vec!["apple", "mango", "zebra"]);
}

#[test]
fn env_keys_are_sorted() {
    let mut env = BTreeMap::new();
    env.insert("Z_VAR".into(), "z".into());
    env.insert("A_VAR".into(), "a".into());
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            env,
            ..Default::default()
        })
        .build();
    let keys: Vec<&String> = wo.config.env.keys().collect();
    assert_eq!(keys, vec!["A_VAR", "Z_VAR"]);
}

#[test]
fn vendor_config_sorted_in_serialized_json() {
    let mut vendor = BTreeMap::new();
    vendor.insert("z_backend".into(), json!("last"));
    vendor.insert("a_backend".into(), json!("first"));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let a_pos = json.find("a_backend").unwrap();
    let z_pos = json.find("z_backend").unwrap();
    assert!(a_pos < z_pos, "BTreeMap should serialize in sorted order");
}

#[test]
fn canonical_json_is_deterministic() {
    let wo = maximal_wo();
    let json1 = abp_core::canonical_json(&wo).unwrap();
    let json2 = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(json1, json2);
}

// ===========================================================================
// 9. WorkOrder schema conformance
// ===========================================================================

#[test]
fn json_schema_can_be_generated() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_json = serde_json::to_value(&schema).unwrap();
    assert!(schema_json.is_object());
    assert!(schema_json.get("$schema").is_some() || schema_json.get("title").is_some());
}

#[test]
fn minimal_wo_validates_against_schema() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let wo_value = serde_json::to_value(minimal_wo()).unwrap();
    assert!(validator.validate(&wo_value).is_ok());
}

#[test]
fn maximal_wo_validates_against_schema() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let wo_value = serde_json::to_value(maximal_wo()).unwrap();
    assert!(validator.validate(&wo_value).is_ok());
}

#[test]
fn invalid_json_fails_schema_validation() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    // Missing required fields
    let bad = json!({"task": "hello"});
    assert!(validator.validate(&bad).is_err());
}

// ===========================================================================
// 10. Edge cases: empty task, very long task, unicode in task
// ===========================================================================

#[test]
fn empty_task_roundtrips() {
    let mut wo = minimal_wo();
    wo.task = String::new();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, "");
}

#[test]
fn very_long_task_roundtrips() {
    let long_task = "x".repeat(100_000);
    let wo = WorkOrderBuilder::new(long_task.clone()).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, long_task);
}

#[test]
fn unicode_task_roundtrips() {
    let task = "修复认证模块 🔧 تصحيح الخطأ";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn emoji_heavy_task() {
    let task = "🚀🔥💻🎯🛠️ Fix all the things! 🧪✅";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn task_with_null_bytes_in_json() {
    // Null bytes are valid in Rust strings but tricky in JSON
    let task = "before\0after";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn task_with_backslashes() {
    let task = r"C:\Users\test\path";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn task_with_mixed_whitespace() {
    let task = "  \ttabs and spaces\n\nnewlines  ";
    let wo = WorkOrderBuilder::new(task).build();
    assert_eq!(wo.task, task);
}

// ===========================================================================
// 11. WorkOrder cloning and equality
// ===========================================================================

#[test]
fn clone_preserves_all_fields() {
    let wo = maximal_wo();
    let cloned = wo.clone();
    assert_eq!(wo.id, cloned.id);
    assert_eq!(wo.task, cloned.task);
    assert_eq!(wo.workspace.root, cloned.workspace.root);
    assert_eq!(wo.config.model, cloned.config.model);
    assert_eq!(wo.config.max_turns, cloned.config.max_turns);
    assert_eq!(wo.config.max_budget_usd, cloned.config.max_budget_usd);
    assert_eq!(wo.config.vendor, cloned.config.vendor);
    assert_eq!(wo.config.env, cloned.config.env);
    assert_eq!(wo.policy.allowed_tools, cloned.policy.allowed_tools);
    assert_eq!(wo.policy.disallowed_tools, cloned.policy.disallowed_tools);
    assert_eq!(wo.policy.deny_read, cloned.policy.deny_read);
    assert_eq!(wo.policy.deny_write, cloned.policy.deny_write);
    assert_eq!(wo.context.files, cloned.context.files);
    assert_eq!(
        wo.requirements.required.len(),
        cloned.requirements.required.len()
    );
}

#[test]
fn clone_is_independent() {
    let wo = maximal_wo();
    let mut cloned = wo.clone();
    cloned.task = "modified".into();
    assert_ne!(wo.task, cloned.task);
    assert_eq!(wo.task, "Refactor the auth module completely");
}

#[test]
fn clone_minimal_work_order() {
    let wo = minimal_wo();
    let cloned = wo.clone();
    assert_eq!(wo.id, cloned.id);
    assert_eq!(wo.task, cloned.task);
}

#[test]
fn serialized_clone_matches_original() {
    let wo = maximal_wo();
    let cloned = wo.clone();
    let json_orig = serde_json::to_string(&wo).unwrap();
    let json_clone = serde_json::to_string(&cloned).unwrap();
    assert_eq!(json_orig, json_clone);
}

// ===========================================================================
// 12. WorkOrder with capabilities
// ===========================================================================

#[test]
fn work_order_no_capabilities() {
    let wo = minimal_wo();
    assert!(!wo.has_capability(&Capability::ToolRead));
    assert!(!wo.has_capability(&Capability::Streaming));
}

#[test]
fn work_order_with_single_capability() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    assert!(wo.has_capability(&Capability::ToolRead));
    assert!(!wo.has_capability(&Capability::Streaming));
}

#[test]
fn work_order_with_all_tool_capabilities() {
    let caps = vec![
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ];
    let wo = WorkOrderBuilder::new("task")
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
        assert!(wo.has_capability(cap));
    }
}

#[test]
fn capability_native_vs_emulated() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    assert!(json.contains("\"native\""));
    assert!(json.contains("\"emulated\""));
}

#[test]
fn capability_serde_roundtrip() {
    let cap = Capability::ExtendedThinking;
    let json = serde_json::to_string(&cap).unwrap();
    assert_eq!(json, r#""extended_thinking""#);
    let rt: Capability = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, cap);
}

#[test]
fn all_capabilities_serialize_as_snake_case() {
    let caps = vec![
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
    for (cap, expected) in caps {
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, format!("\"{expected}\""), "mismatch for {cap:?}");
    }
}

// ===========================================================================
// 13. WorkOrder debug/display formatting
// ===========================================================================

#[test]
fn debug_format_contains_task() {
    let wo = WorkOrderBuilder::new("debug test task").build();
    let debug = format!("{wo:?}");
    assert!(debug.contains("debug test task"));
}

#[test]
fn debug_format_contains_struct_name() {
    let wo = minimal_wo();
    let debug = format!("{wo:?}");
    assert!(debug.contains("WorkOrder"));
}

#[test]
fn debug_format_contains_id() {
    let wo = minimal_wo();
    let debug = format!("{wo:?}");
    assert!(debug.contains(&wo.id.to_string()));
}

#[test]
fn debug_format_of_lane() {
    let debug_pf = format!("{:?}", ExecutionLane::PatchFirst);
    let debug_wf = format!("{:?}", ExecutionLane::WorkspaceFirst);
    assert_eq!(debug_pf, "PatchFirst");
    assert_eq!(debug_wf, "WorkspaceFirst");
}

#[test]
fn debug_format_of_workspace_mode() {
    let debug_pt = format!("{:?}", WorkspaceMode::PassThrough);
    let debug_st = format!("{:?}", WorkspaceMode::Staged);
    assert_eq!(debug_pt, "PassThrough");
    assert_eq!(debug_st, "Staged");
}

// ===========================================================================
// 14. Extension trait tests
// ===========================================================================

#[test]
fn ext_is_code_task_positive() {
    let wo = WorkOrderBuilder::new("Fix the login code").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_negative() {
    let wo = WorkOrderBuilder::new("Write documentation").build();
    assert!(!wo.is_code_task());
}

#[test]
fn ext_is_code_task_refactor() {
    let wo = WorkOrderBuilder::new("Refactor the auth module").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_task_summary_short() {
    let wo = WorkOrderBuilder::new("short").build();
    assert_eq!(wo.task_summary(100), "short");
}

#[test]
fn ext_task_summary_truncated() {
    let wo = WorkOrderBuilder::new("a]very long task description here").build();
    let summary = wo.task_summary(10);
    assert!(summary.len() <= 14); // 10 chars + "…" (up to 3 bytes)
    assert!(summary.ends_with('…'));
}

#[test]
fn ext_task_summary_unicode_boundary() {
    let wo = WorkOrderBuilder::new("héllo wörld").build();
    let summary = wo.task_summary(5);
    // Should not panic on multi-byte boundary
    assert!(summary.ends_with('…'));
}

#[test]
fn ext_tool_budget_remaining_none() {
    let wo = minimal_wo();
    assert_eq!(wo.tool_budget_remaining(), None);
}

#[test]
fn ext_tool_budget_remaining_some() {
    let wo = WorkOrderBuilder::new("task").max_turns(42).build();
    assert_eq!(wo.tool_budget_remaining(), Some(42));
}

#[test]
fn ext_vendor_config_lookup() {
    let wo = maximal_wo();
    assert!(wo.vendor_config("abp").is_some());
    assert!(wo.vendor_config("nonexistent").is_none());
}

#[test]
fn ext_required_capabilities_inferred_edit() {
    let wo = WorkOrderBuilder::new("Edit the main file").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolEdit));
}

#[test]
fn ext_required_capabilities_inferred_bash() {
    let wo = WorkOrderBuilder::new("Run a shell command").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolBash));
}

#[test]
fn ext_required_capabilities_explicit_plus_inferred() {
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
// 15. ConfigDefaults
// ===========================================================================

#[test]
fn config_defaults_apply() {
    let mut wo = minimal_wo();
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.model.is_none());

    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.max_turns, Some(25));
    assert_eq!(wo.config.max_budget_usd, Some(1.0));
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn config_defaults_do_not_overwrite_existing() {
    let mut wo = WorkOrderBuilder::new("task")
        .model("claude-3")
        .max_turns(100)
        .max_budget_usd(10.0)
        .build();

    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
    assert_eq!(wo.config.max_turns, Some(100));
    assert_eq!(wo.config.max_budget_usd, Some(10.0));
}

// ===========================================================================
// 16. Serde edge cases & contract version
// ===========================================================================

#[test]
fn contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn execution_lane_serde_snake_case() {
    let pf = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(pf, r#""patch_first""#);
    let wf = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(wf, r#""workspace_first""#);
}

#[test]
fn workspace_mode_serde_snake_case() {
    let pt = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(pt, r#""pass_through""#);
    let st = serde_json::to_string(&WorkspaceMode::Staged).unwrap();
    assert_eq!(st, r#""staged""#);
}

#[test]
fn deserialize_rejects_unknown_lane() {
    let bad = r#""unknown_lane""#;
    let result = serde_json::from_str::<ExecutionLane>(bad);
    assert!(result.is_err());
}

#[test]
fn deserialize_rejects_unknown_workspace_mode() {
    let bad = r#""unknown_mode""#;
    let result = serde_json::from_str::<WorkspaceMode>(bad);
    assert!(result.is_err());
}

#[test]
fn min_support_serde_snake_case() {
    let native = serde_json::to_string(&MinSupport::Native).unwrap();
    assert_eq!(native, r#""native""#);
    let emulated = serde_json::to_string(&MinSupport::Emulated).unwrap();
    assert_eq!(emulated, r#""emulated""#);
}

#[test]
fn work_order_from_value_roundtrip() {
    let wo = maximal_wo();
    let value = serde_json::to_value(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_value(value).unwrap();
    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
}

// ===========================================================================
// 17. Policy profile tests
// ===========================================================================

#[test]
fn empty_policy_is_default() {
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
fn policy_serde_roundtrip() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
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
    assert_eq!(rt.disallowed_tools, policy.disallowed_tools);
    assert_eq!(rt.deny_read, policy.deny_read);
    assert_eq!(rt.deny_write, policy.deny_write);
}

// ===========================================================================
// 18. WorkspaceSpec tests
// ===========================================================================

#[test]
fn workspace_spec_serde_roundtrip() {
    let spec = WorkspaceSpec {
        root: "/home/user/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["node_modules/**".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let rt: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.root, spec.root);
    assert_eq!(rt.include, spec.include);
    assert_eq!(rt.exclude, spec.exclude);
}

#[test]
fn workspace_pass_through_mode_roundtrip() {
    let spec = WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::PassThrough,
        include: vec![],
        exclude: vec![],
    };
    let json = serde_json::to_string(&spec).unwrap();
    assert!(json.contains("pass_through"));
    let rt: WorkspaceSpec = serde_json::from_str(&json).unwrap();
    assert!(matches!(rt.mode, WorkspaceMode::PassThrough));
}

// ===========================================================================
// 19. Construction — additional coverage
// ===========================================================================

#[test]
fn builder_preserves_leading_trailing_spaces_in_task() {
    let wo = WorkOrderBuilder::new("  padded task  ").build();
    assert_eq!(wo.task, "  padded task  ");
}

#[test]
fn multiple_builders_are_independent() {
    let b1 = WorkOrderBuilder::new("task one");
    let b2 = WorkOrderBuilder::new("task two");
    let wo1 = b1.lane(ExecutionLane::WorkspaceFirst).build();
    let wo2 = b2.lane(ExecutionLane::PatchFirst).build();
    assert_eq!(wo1.task, "task one");
    assert_eq!(wo2.task, "task two");
    assert!(matches!(wo1.lane, ExecutionLane::WorkspaceFirst));
    assert!(matches!(wo2.lane, ExecutionLane::PatchFirst));
}

#[test]
fn builder_with_all_methods_chained() {
    let wo = WorkOrderBuilder::new("full chain")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .context(ContextPacket {
            files: vec!["f.rs".into()],
            snippets: vec![],
        })
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            ..Default::default()
        })
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        })
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(2.0)
        .build();
    assert_eq!(wo.task, "full chain");
    assert_eq!(wo.workspace.root, "/workspace");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.context.files, vec!["f.rs"]);
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.requirements.required.len(), 1);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(5));
    assert_eq!(wo.config.max_budget_usd, Some(2.0));
}

#[test]
fn builder_empty_include_populated_exclude() {
    let wo = WorkOrderBuilder::new("task")
        .include(vec![])
        .exclude(vec!["*.log".into(), "tmp/**".into()])
        .build();
    assert!(wo.workspace.include.is_empty());
    assert_eq!(wo.workspace.exclude.len(), 2);
}

#[test]
fn builder_context_multiple_files_and_snippets() {
    let wo = WorkOrderBuilder::new("task")
        .context(ContextPacket {
            files: vec!["a.rs".into(), "b.rs".into(), "c.rs".into()],
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
        })
        .build();
    assert_eq!(wo.context.files.len(), 3);
    assert_eq!(wo.context.snippets.len(), 2);
    assert_eq!(wo.context.snippets[1].name, "s2");
}

#[test]
fn uuid_is_v4_format() {
    let wo = minimal_wo();
    let id_str = wo.id.to_string();
    // UUID v4 has the form xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
    assert_eq!(id_str.len(), 36);
    assert_eq!(&id_str[14..15], "4");
}

#[test]
fn builder_config_override_replaces_full_config() {
    let wo = WorkOrderBuilder::new("task")
        .model("initial-model")
        .config(RuntimeConfig {
            model: Some("replaced-model".into()),
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("replaced-model"));
}

#[test]
fn work_order_id_is_not_nil() {
    for _ in 0..10 {
        let wo = minimal_wo();
        assert!(!wo.id.is_nil());
    }
}

#[test]
fn builder_default_model_is_none() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.config.model.is_none());
}

// ===========================================================================
// 20. Serialization roundtrip — additional coverage
// ===========================================================================

#[test]
fn pretty_and_compact_both_deserialize_identically() {
    let wo = maximal_wo();
    let compact = serde_json::to_string(&wo).unwrap();
    let pretty = serde_json::to_string_pretty(&wo).unwrap();
    let from_compact: WorkOrder = serde_json::from_str(&compact).unwrap();
    let from_pretty: WorkOrder = serde_json::from_str(&pretty).unwrap();
    assert_eq!(from_compact.id, from_pretty.id);
    assert_eq!(from_compact.task, from_pretty.task);
    assert_eq!(from_compact.config.model, from_pretty.config.model);
    assert_eq!(from_compact.config.max_turns, from_pretty.config.max_turns);
}

#[test]
fn deserialization_rejects_missing_required_field_task() {
    let raw = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "lane": "patch_first",
        "workspace": {"root": ".", "mode": "staged", "include": [], "exclude": []},
        "context": {"files": [], "snippets": []},
        "policy": {"allowed_tools": [], "disallowed_tools": [], "deny_read": [], "deny_write": [], "allow_network": [], "deny_network": [], "require_approval_for": []},
        "requirements": {"required": []},
        "config": {"model": null, "vendor": {}, "env": {}, "max_budget_usd": null, "max_turns": null}
    }"#;
    assert!(serde_json::from_str::<WorkOrder>(raw).is_err());
}

#[test]
fn large_vendor_config_roundtrip() {
    let mut vendor = BTreeMap::new();
    for i in 0..100 {
        vendor.insert(
            format!("key_{i:04}"),
            json!({"index": i, "data": "x".repeat(100)}),
        );
    }
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor.len(), 100);
    assert_eq!(wo2.config.vendor["key_0050"]["index"], 50);
}

#[test]
fn multiple_sequential_roundtrips_are_stable() {
    let wo = maximal_wo();
    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    let wo3: WorkOrder = serde_json::from_str(&json2).unwrap();
    let json3 = serde_json::to_string(&wo3).unwrap();
    assert_eq!(json1, json2);
    assert_eq!(json2, json3);
}

#[test]
fn serialized_length_is_deterministic() {
    let wo = maximal_wo();
    let len1 = serde_json::to_string(&wo).unwrap().len();
    let len2 = serde_json::to_string(&wo).unwrap().len();
    assert_eq!(len1, len2);
}

#[test]
fn unicode_in_vendor_config_roundtrip() {
    let mut vendor = BTreeMap::new();
    vendor.insert("label".into(), json!("日本語テスト 🎌"));
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["label"], "日本語テスト 🎌");
}

#[test]
fn env_with_special_values_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin:/usr/local/bin".into());
    env.insert("QUOTED".into(), r#"value with "quotes""#.into());
    env.insert("NEWLINE".into(), "line1\nline2".into());
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            env,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.env["PATH"], "/usr/bin:/usr/local/bin");
    assert_eq!(wo2.config.env["QUOTED"], r#"value with "quotes""#);
    assert_eq!(wo2.config.env["NEWLINE"], "line1\nline2");
}

#[test]
fn boolean_and_numeric_vendor_values_roundtrip() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "flags".into(),
        json!({"enabled": true, "count": 42, "rate": 3.15}),
    );
    let wo = WorkOrderBuilder::new("task")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["flags"]["enabled"], true);
    assert_eq!(wo2.config.vendor["flags"]["count"], 42);
}

#[test]
fn serialization_to_value_and_back() {
    let wo = maximal_wo();
    let value = serde_json::to_value(&wo).unwrap();
    assert!(value.is_object());
    let obj = value.as_object().unwrap();
    assert_eq!(obj["task"].as_str().unwrap(), wo.task);
    let wo2: WorkOrder = serde_json::from_value(value).unwrap();
    assert_eq!(wo.id, wo2.id);
}

// ===========================================================================
// 21. Validation — additional coverage
// ===========================================================================

#[test]
fn validator_accepts_maximal_work_order() {
    let wo = maximal_wo();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_rejects_empty_allow_network_glob() {
    let mut wo = minimal_wo();
    wo.policy.allow_network = vec!["".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.allow_network"));
}

#[test]
fn validator_rejects_empty_deny_network_glob() {
    let mut wo = minimal_wo();
    wo.policy.deny_network = vec!["  ".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_network"));
}

#[test]
fn validator_rejects_empty_disallowed_tools_entry() {
    let mut wo = minimal_wo();
    wo.policy.disallowed_tools = vec!["".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.disallowed_tools")
    );
}

#[test]
fn validator_rejects_empty_require_approval_for_entry() {
    let mut wo = minimal_wo();
    wo.policy.require_approval_for = vec!["".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.require_approval_for")
    );
}

#[test]
fn validator_accepts_very_large_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(u32::MAX).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_accepts_very_large_budget() {
    let wo = WorkOrderBuilder::new("task")
        .max_budget_usd(1_000_000.0)
        .build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_multiple_errors_at_once() {
    let mut wo = minimal_wo();
    wo.task = String::new();
    wo.config.max_turns = Some(0);
    wo.config.max_budget_usd = Some(-1.0);
    wo.config.model = Some("  ".into());
    wo.config.vendor.insert("".into(), json!("bad"));
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.len() >= 4);
    let fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
    assert!(fields.contains(&"task"));
    assert!(fields.contains(&"config.max_turns"));
    assert!(fields.contains(&"config.max_budget_usd"));
    assert!(fields.contains(&"config.model"));
    assert!(fields.contains(&"config.vendor"));
}

#[test]
fn validator_duplicate_in_disallowed_tools() {
    let mut wo = minimal_wo();
    wo.policy.disallowed_tools = vec!["bash".into(), "bash".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    // disallowed_tools are checked for empty globs, not duplicates per se.
    // Ensure no panic occurs at minimum.
    let _ = warnings;
}

#[test]
fn validator_conflicting_allow_and_deny_tools() {
    let mut wo = minimal_wo();
    wo.policy.allowed_tools = vec!["bash".into()];
    wo.policy.disallowed_tools = vec!["bash".into()];
    // Validator does not currently detect conflicts, but should not panic.
    let _warnings = ConfigValidator::new().validate_work_order(&wo);
}

// ===========================================================================
// 22. Policies and capabilities — additional coverage
// ===========================================================================

#[test]
fn policy_with_all_fields_populated_roundtrip() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into(), "edit".into()],
        disallowed_tools: vec!["bash".into(), "deploy".into()],
        deny_read: vec!["*.key".into(), "*.pem".into()],
        deny_write: vec!["/etc/**".into(), "/root/**".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.internal".into(), "10.0.0.0/8".into()],
        require_approval_for: vec!["deploy".into(), "delete".into()],
    };
    let wo = WorkOrderBuilder::new("secured task").policy(policy).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.allowed_tools.len(), 3);
    assert_eq!(wo2.policy.disallowed_tools.len(), 2);
    assert_eq!(wo2.policy.deny_read.len(), 2);
    assert_eq!(wo2.policy.deny_write.len(), 2);
    assert_eq!(wo2.policy.allow_network.len(), 1);
    assert_eq!(wo2.policy.deny_network.len(), 2);
    assert_eq!(wo2.policy.require_approval_for.len(), 2);
}

#[test]
fn capabilities_mixed_support_levels_roundtrip() {
    let wo = WorkOrderBuilder::new("task")
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
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Native,
                },
            ],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.requirements.required.len(), 3);
    assert!(matches!(
        wo2.requirements.required[0].min_support,
        MinSupport::Native
    ));
    assert!(matches!(
        wo2.requirements.required[1].min_support,
        MinSupport::Emulated
    ));
}

#[test]
fn combined_policy_capability_config_roundtrip() {
    let wo = WorkOrderBuilder::new("complex task")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            deny_write: vec!["/secret/**".into()],
            ..Default::default()
        })
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        })
        .model("claude-3-opus")
        .max_turns(20)
        .max_budget_usd(3.0)
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo2.policy.deny_write, vec!["/secret/**"]);
    assert_eq!(wo2.requirements.required.len(), 1);
    assert_eq!(wo2.config.model.as_deref(), Some("claude-3-opus"));
    assert_eq!(wo2.config.max_turns, Some(20));
    assert_eq!(wo2.config.max_budget_usd, Some(3.0));
}

#[test]
fn policy_deny_read_multiple_patterns_preserved() {
    let patterns = vec![
        "*.key".into(),
        "*.pem".into(),
        "*.env".into(),
        ".git/**".into(),
        "secrets/**".into(),
    ];
    let wo = WorkOrderBuilder::new("task")
        .policy(PolicyProfile {
            deny_read: patterns.clone(),
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.deny_read, patterns);
}

#[test]
fn all_capability_variants_roundtrip_in_requirements() {
    let caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::CodeExecution,
    ];
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: caps
                .iter()
                .map(|c| CapabilityRequirement {
                    capability: c.clone(),
                    min_support: MinSupport::Emulated,
                })
                .collect(),
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.requirements.required.len(), caps.len());
    for (i, cap) in caps.iter().enumerate() {
        assert_eq!(&wo2.requirements.required[i].capability, cap);
    }
}

// ===========================================================================
// 23. Context packet edge cases
// ===========================================================================

#[test]
fn context_empty_file_path_roundtrip() {
    let wo = WorkOrderBuilder::new("task")
        .context(ContextPacket {
            files: vec!["".into()],
            snippets: vec![],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.files, vec![""]);
}

#[test]
fn context_snippet_with_large_content() {
    let content = "x".repeat(50_000);
    let wo = WorkOrderBuilder::new("task")
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "big".into(),
                content: content.clone(),
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.snippets[0].content.len(), 50_000);
}

#[test]
fn context_snippet_unicode_name_and_content() {
    let wo = WorkOrderBuilder::new("task")
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "提示".into(),
                content: "使用 JWT 令牌 🔑".into(),
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.snippets[0].name, "提示");
    assert_eq!(wo2.context.snippets[0].content, "使用 JWT 令牌 🔑");
}

// ===========================================================================
// 24. Canonical JSON and hashing
// ===========================================================================

#[test]
fn canonical_json_minimal_is_compact() {
    let wo = minimal_wo();
    let cj = abp_core::canonical_json(&wo).unwrap();
    // Canonical JSON has no pretty-printing whitespace
    assert!(!cj.contains('\n'));
}

#[test]
fn canonical_json_maximal_sorted_keys() {
    let wo = maximal_wo();
    let cj = abp_core::canonical_json(&wo).unwrap();
    // Config key "env" should appear before "max_budget_usd" (alphabetical)
    let env_pos = cj.find("\"env\"").unwrap();
    let budget_pos = cj.find("\"max_budget_usd\"").unwrap();
    assert!(env_pos < budget_pos);
}

#[test]
fn sha256_hex_of_work_order_json_is_64_chars() {
    let wo = maximal_wo();
    let json = serde_json::to_string(&wo).unwrap();
    let hash = abp_core::sha256_hex(json.as_bytes());
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

// ===========================================================================
// 25. RuntimeConfig edge cases
// ===========================================================================

#[test]
fn runtime_config_default_all_none_empty() {
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
    vendor.insert("key".into(), json!([1, 2, 3]));
    let mut env = BTreeMap::new();
    env.insert("HOME".into(), "/home/user".into());
    let cfg = RuntimeConfig {
        model: Some("test-model".into()),
        vendor,
        env,
        max_budget_usd: Some(9.99),
        max_turns: Some(100),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let cfg2: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg2.model, cfg.model);
    assert_eq!(cfg2.max_turns, cfg.max_turns);
    assert_eq!(cfg2.max_budget_usd, cfg.max_budget_usd);
    assert_eq!(cfg2.vendor.len(), 1);
    assert_eq!(cfg2.env.len(), 1);
}

#[test]
fn budget_with_fractional_cents() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(0.001).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!((wo2.config.max_budget_usd.unwrap() - 0.001).abs() < f64::EPSILON);
}
