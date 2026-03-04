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
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Comprehensive tests for [`WorkOrder`] construction via [`WorkOrderBuilder`]
//! and validation edge cases. Covers builder chaining, serde stability,
//! deterministic serialization, schema compliance, unicode handling, and more.

use abp_core::config::{ConfigDefaults, ConfigValidator, WarningSeverity};
use abp_core::ext::WorkOrderExt;
use abp_core::{
    CONTRACT_VERSION, Capability, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, PolicyProfile, RuntimeConfig,
    WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use serde_json::json;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn builder_minimal() -> WorkOrderBuilder {
    WorkOrderBuilder::new("minimal task")
}

fn full_vendor_map() -> BTreeMap<String, serde_json::Value> {
    let mut m = BTreeMap::new();
    m.insert("abp".into(), json!({"mode": "passthrough", "request": {}}));
    m.insert("openai".into(), json!({"temperature": 0.7, "top_p": 0.9}));
    m.insert(
        "anthropic".into(),
        json!({"max_tokens": 8192, "stop_sequences": ["END"]}),
    );
    m
}

fn full_env_map() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("RUST_LOG".into(), "debug".into());
    m.insert("HOME".into(), "/home/user".into());
    m
}

fn many_tools(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("tool_{i}")).collect()
}

fn many_capabilities(n: usize) -> CapabilityRequirements {
    let all_caps = [
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
    CapabilityRequirements {
        required: all_caps
            .iter()
            .take(n.min(all_caps.len()))
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

// ===========================================================================
// 1. Builder chaining — every setter returns Self
// ===========================================================================

#[test]
fn builder_chain_all_setters() {
    let wo = WorkOrderBuilder::new("chain test")
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
        .requirements(many_capabilities(3))
        .model("gpt-4o")
        .max_turns(20)
        .max_budget_usd(2.5)
        .build();

    assert_eq!(wo.task, "chain test");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/workspace");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.context.files, vec!["f.rs"]);
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.requirements.required.len(), 3);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    assert_eq!(wo.config.max_turns, Some(20));
    assert_eq!(wo.config.max_budget_usd, Some(2.5));
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
fn builder_config_then_model_keeps_other_config_fields() {
    let cfg = RuntimeConfig {
        max_turns: Some(50),
        max_budget_usd: Some(3.0),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).model("m").build();
    assert_eq!(wo.config.max_turns, Some(50));
    assert_eq!(wo.config.max_budget_usd, Some(3.0));
    assert_eq!(wo.config.model.as_deref(), Some("m"));
}

#[test]
fn builder_max_turns_overrides_config_max_turns() {
    let cfg = RuntimeConfig {
        max_turns: Some(10),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t").config(cfg).max_turns(99).build();
    assert_eq!(wo.config.max_turns, Some(99));
}

#[test]
fn builder_max_budget_overrides_config_budget() {
    let cfg = RuntimeConfig {
        max_budget_usd: Some(1.0),
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("t")
        .config(cfg)
        .max_budget_usd(7.77)
        .build();
    assert_eq!(wo.config.max_budget_usd, Some(7.77));
}

// ===========================================================================
// 2. Serde roundtrip stability (serialize → deserialize → serialize = same)
// ===========================================================================

#[test]
fn roundtrip_stability_minimal() {
    let wo = builder_minimal().build();
    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn roundtrip_stability_with_vendor() {
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            ..Default::default()
        })
        .build();
    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn roundtrip_stability_with_policy() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            allowed_tools: vec!["a".into(), "b".into()],
            disallowed_tools: vec!["c".into()],
            deny_read: vec!["*.key".into()],
            deny_write: vec!["/etc/**".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["*.internal".into()],
            require_approval_for: vec!["deploy".into()],
        })
        .build();
    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn roundtrip_stability_with_capabilities() {
    let wo = WorkOrderBuilder::new("t")
        .requirements(many_capabilities(10))
        .build();
    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn roundtrip_stability_with_env() {
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            env: full_env_map(),
            ..Default::default()
        })
        .build();
    let json1 = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&wo2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn roundtrip_stability_pretty_vs_compact() {
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            env: full_env_map(),
            model: Some("gpt-4".into()),
            max_turns: Some(10),
            max_budget_usd: Some(1.5),
        })
        .build();
    // Pretty → deserialize → compact == compact from original
    let pretty = serde_json::to_string_pretty(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&pretty).unwrap();
    let compact_from_pretty = serde_json::to_string(&wo2).unwrap();
    let compact_direct = serde_json::to_string(&wo).unwrap();
    assert_eq!(compact_from_pretty, compact_direct);
}

// ===========================================================================
// 3. Dotted keys in vendor map ("abp.mode", "abp.request")
// ===========================================================================

#[test]
fn vendor_dotted_key_abp_mode() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp.mode".into(), json!("passthrough"));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor["abp.mode"], "passthrough");
}

#[test]
fn vendor_dotted_key_abp_request() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp.request".into(), json!({"stream": true}));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor["abp.request"]["stream"], true);
}

#[test]
fn vendor_dotted_key_survives_roundtrip() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp.mode".into(), json!("mapped"));
    vendor.insert("vendor.sub.key".into(), json!(42));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["abp.mode"], "mapped");
    assert_eq!(wo2.config.vendor["vendor.sub.key"], 42);
}

#[test]
fn vendor_dotted_vs_nested_are_distinct() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp.mode".into(), json!("flat-dotted"));
    vendor.insert("abp".into(), json!({"mode": "nested"}));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor["abp.mode"], "flat-dotted");
    assert_eq!(wo.config.vendor["abp"]["mode"], "nested");
}

// ===========================================================================
// 4. Large WorkOrders (many tools, long prompts, many capabilities)
// ===========================================================================

#[test]
fn large_tool_lists_roundtrip() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            allowed_tools: many_tools(200),
            disallowed_tools: many_tools(100),
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.allowed_tools.len(), 200);
    assert_eq!(wo2.policy.disallowed_tools.len(), 100);
}

#[test]
fn large_number_of_capabilities_roundtrip() {
    let wo = WorkOrderBuilder::new("t")
        .requirements(many_capabilities(26))
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.requirements.required.len(), 26);
}

#[test]
fn very_long_prompt_10k() {
    let task = "a".repeat(10_000);
    let wo = WorkOrderBuilder::new(task.clone()).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task.len(), 10_000);
}

#[test]
fn very_long_prompt_1m() {
    let task = "b".repeat(1_000_000);
    let wo = WorkOrderBuilder::new(task.clone()).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task.len(), 1_000_000);
}

#[test]
fn many_context_files() {
    let files: Vec<String> = (0..500).map(|i| format!("src/file_{i}.rs")).collect();
    let wo = WorkOrderBuilder::new("t")
        .context(ContextPacket {
            files: files.clone(),
            snippets: vec![],
        })
        .build();
    assert_eq!(wo.context.files.len(), 500);
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.files.len(), 500);
}

#[test]
fn many_context_snippets() {
    let snippets: Vec<ContextSnippet> = (0..100)
        .map(|i| ContextSnippet {
            name: format!("snippet_{i}"),
            content: format!("content for snippet {i}"),
        })
        .collect();
    let wo = WorkOrderBuilder::new("t")
        .context(ContextPacket {
            files: vec![],
            snippets: snippets.clone(),
        })
        .build();
    assert_eq!(wo.context.snippets.len(), 100);
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.snippets.len(), 100);
    assert_eq!(wo2.context.snippets[50].name, "snippet_50");
}

#[test]
fn many_vendor_entries() {
    let mut vendor = BTreeMap::new();
    for i in 0..100 {
        vendor.insert(format!("vendor_{i:03}"), json!({"idx": i}));
    }
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.vendor.len(), 100);
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor.len(), 100);
}

#[test]
fn many_env_entries() {
    let mut env = BTreeMap::new();
    for i in 0..200 {
        env.insert(format!("VAR_{i:03}"), format!("val_{i}"));
    }
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            env,
            ..Default::default()
        })
        .build();
    assert_eq!(wo.config.env.len(), 200);
}

#[test]
fn many_include_exclude_globs() {
    let inc: Vec<String> = (0..50).map(|i| format!("src/{i}/**/*.rs")).collect();
    let exc: Vec<String> = (0..50).map(|i| format!("target/{i}/**")).collect();
    let wo = WorkOrderBuilder::new("t")
        .include(inc.clone())
        .exclude(exc.clone())
        .build();
    assert_eq!(wo.workspace.include.len(), 50);
    assert_eq!(wo.workspace.exclude.len(), 50);
}

#[test]
fn large_deny_read_and_write_lists() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            deny_read: (0..100).map(|i| format!("secret_{i}/**")).collect(),
            deny_write: (0..100).map(|i| format!("protected_{i}/**")).collect(),
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.deny_read.len(), 100);
    assert_eq!(wo.policy.deny_write.len(), 100);
}

// ===========================================================================
// 5. BTreeMap deterministic serialization
// ===========================================================================

#[test]
fn btreemap_vendor_insert_order_irrelevant() {
    let mut v1 = BTreeMap::new();
    v1.insert("z".into(), json!(1));
    v1.insert("a".into(), json!(2));

    let mut v2 = BTreeMap::new();
    v2.insert("a".into(), json!(2));
    v2.insert("z".into(), json!(1));

    let wo1 = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: v1,
            ..Default::default()
        })
        .build();
    let wo2 = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: v2,
            ..Default::default()
        })
        .build();
    // Must produce the same JSON key order regardless of insertion order
    let j1 = serde_json::to_value(&wo1.config.vendor).unwrap();
    let j2 = serde_json::to_value(&wo2.config.vendor).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn btreemap_env_insert_order_irrelevant() {
    let mut e1: BTreeMap<String, String> = BTreeMap::new();
    e1.insert("ZZZ".into(), "last".into());
    e1.insert("AAA".into(), "first".into());

    let mut e2: BTreeMap<String, String> = BTreeMap::new();
    e2.insert("AAA".into(), "first".into());
    e2.insert("ZZZ".into(), "last".into());

    let json1 = serde_json::to_string(&e1).unwrap();
    let json2 = serde_json::to_string(&e2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn canonical_json_vendor_deterministic() {
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            ..Default::default()
        })
        .build();
    let c1 = abp_core::canonical_json(&wo).unwrap();
    let c2 = abp_core::canonical_json(&wo).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonical_json_same_for_clone() {
    let wo = WorkOrderBuilder::new("determinism")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            env: full_env_map(),
            model: Some("gpt-4".into()),
            max_turns: Some(10),
            max_budget_usd: Some(1.0),
        })
        .build();
    let cloned = wo.clone();
    let c1 = abp_core::canonical_json(&wo).unwrap();
    let c2 = abp_core::canonical_json(&cloned).unwrap();
    assert_eq!(c1, c2);
}

// ===========================================================================
// 6. JSON schema compliance
// ===========================================================================

#[test]
fn schema_validates_builder_with_all_fields() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();

    let wo = WorkOrderBuilder::new("full")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["target/**".into()])
        .context(ContextPacket {
            files: vec!["a.rs".into()],
            snippets: vec![ContextSnippet {
                name: "s".into(),
                content: "c".into(),
            }],
        })
        .policy(PolicyProfile {
            allowed_tools: vec!["r".into()],
            disallowed_tools: vec!["w".into()],
            deny_read: vec!["*.key".into()],
            deny_write: vec!["/etc/**".into()],
            allow_network: vec!["*.example.com".into()],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["deploy".into()],
        })
        .requirements(many_capabilities(5))
        .model("claude-3")
        .max_turns(10)
        .max_budget_usd(1.0)
        .build();

    let val = serde_json::to_value(&wo).unwrap();
    assert!(validator.validate(&val).is_ok());
}

#[test]
fn schema_validates_unicode_task() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let wo = WorkOrderBuilder::new("日本語テスト 🚀").build();
    let val = serde_json::to_value(&wo).unwrap();
    assert!(validator.validate(&val).is_ok());
}

#[test]
fn schema_rejects_missing_task() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let bad = json!({"id": "00000000-0000-0000-0000-000000000001"});
    assert!(validator.validate(&bad).is_err());
}

#[test]
fn schema_rejects_wrong_lane_type() {
    let schema = schemars::schema_for!(WorkOrder);
    let schema_value = serde_json::to_value(&schema).unwrap();
    let validator = jsonschema::validator_for(&schema_value).unwrap();
    let mut val = serde_json::to_value(builder_minimal().build()).unwrap();
    val["lane"] = json!(12345);
    assert!(validator.validate(&val).is_err());
}

// ===========================================================================
// 7. Unicode and special characters
// ===========================================================================

#[test]
fn unicode_cjk_in_task() {
    let task = "修复登录模块中的认证错误";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn unicode_arabic_in_task() {
    let task = "إصلاح خطأ المصادقة في وحدة تسجيل الدخول";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn unicode_in_vendor_keys() {
    let mut vendor = BTreeMap::new();
    vendor.insert("日本語キー".into(), json!("value"));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(wo2.config.vendor.contains_key("日本語キー"));
}

#[test]
fn unicode_in_env_values() {
    let mut env = BTreeMap::new();
    env.insert("GREETING".into(), "こんにちは".into());
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            env,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.env["GREETING"], "こんにちは");
}

#[test]
fn unicode_in_context_snippet() {
    let wo = WorkOrderBuilder::new("t")
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "Ünïcödé".into(),
                content: "Héllo Wörld 🌍".into(),
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.snippets[0].name, "Ünïcödé");
    assert_eq!(wo2.context.snippets[0].content, "Héllo Wörld 🌍");
}

#[test]
fn special_chars_in_glob_patterns() {
    let wo = WorkOrderBuilder::new("t")
        .include(vec!["src/**/*.{rs,toml}".into()])
        .exclude(vec!["[Bb]uild/**".into()])
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.workspace.include[0], "src/**/*.{rs,toml}");
    assert_eq!(wo2.workspace.exclude[0], "[Bb]uild/**");
}

#[test]
fn backslash_paths_in_workspace_root() {
    let wo = WorkOrderBuilder::new("t")
        .root(r"C:\Users\dev\project")
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.workspace.root, r"C:\Users\dev\project");
}

#[test]
fn control_characters_in_task() {
    let task = "tab\there\nnewline\rcarriage";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

#[test]
fn surrogate_pair_emoji_in_task() {
    let task = "🧑‍💻 coding with 👨‍👩‍👧‍👦 family";
    let wo = WorkOrderBuilder::new(task).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.task, task);
}

// ===========================================================================
// 8. All field combinations and defaults
// ===========================================================================

#[test]
fn builder_only_task_yields_defaults_for_everything_else() {
    let wo = WorkOrderBuilder::new("only task").build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.workspace.root, ".");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.policy.deny_read.is_empty());
    assert!(wo.policy.deny_write.is_empty());
    assert!(wo.policy.allow_network.is_empty());
    assert!(wo.policy.deny_network.is_empty());
    assert!(wo.policy.require_approval_for.is_empty());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.config.model.is_none());
    assert!(wo.config.vendor.is_empty());
    assert!(wo.config.env.is_empty());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.max_turns.is_none());
}

#[test]
fn runtime_config_default_all_none_or_empty() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn context_packet_default_is_empty() {
    let ctx = ContextPacket::default();
    assert!(ctx.files.is_empty());
    assert!(ctx.snippets.is_empty());
}

#[test]
fn policy_profile_default_is_empty() {
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
fn capability_requirements_default_is_empty() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

// ===========================================================================
// 9. Validation edge cases
// ===========================================================================

#[test]
fn validator_accepts_max_turns_one() {
    let wo = WorkOrderBuilder::new("t").max_turns(1).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_accepts_max_turns_u32_max() {
    let wo = WorkOrderBuilder::new("t").max_turns(u32::MAX).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_accepts_tiny_budget() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.001).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_accepts_large_budget() {
    let wo = WorkOrderBuilder::new("t")
        .max_budget_usd(999_999.99)
        .build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_rejects_negative_budget_small() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(-0.001).build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.max_budget_usd"));
}

#[test]
fn validator_multiple_errors_at_once() {
    let mut wo = WorkOrderBuilder::new("  ")
        .max_turns(0)
        .max_budget_usd(-1.0)
        .build();
    wo.config.model = Some("  ".into());
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    let error_count = warnings
        .iter()
        .filter(|w| matches!(w.severity, WarningSeverity::Error))
        .count();
    assert!(error_count >= 4);
}

#[test]
fn validator_empty_glob_in_allow_network() {
    let mut wo = builder_minimal().build();
    wo.policy.allow_network = vec!["".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.allow_network"));
}

#[test]
fn validator_empty_glob_in_deny_network() {
    let mut wo = builder_minimal().build();
    wo.policy.deny_network = vec!["   ".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "policy.deny_network"));
}

#[test]
fn validator_empty_glob_in_disallowed_tools() {
    let mut wo = builder_minimal().build();
    wo.policy.disallowed_tools = vec!["".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.disallowed_tools")
    );
}

#[test]
fn validator_empty_glob_in_require_approval() {
    let mut wo = builder_minimal().build();
    wo.policy.require_approval_for = vec![" ".into()];
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(
        warnings
            .iter()
            .any(|w| w.field == "policy.require_approval_for")
    );
}

#[test]
fn validator_whitespace_vendor_key() {
    let mut wo = builder_minimal().build();
    wo.config.vendor.insert("  ".into(), json!("val"));
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.iter().any(|w| w.field == "config.vendor"));
}

#[test]
fn validator_accepts_unicode_task() {
    let wo = WorkOrderBuilder::new("修复🐛").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

#[test]
fn validator_accepts_single_char_task() {
    let wo = WorkOrderBuilder::new("x").build();
    let warnings = ConfigValidator::new().validate_work_order(&wo);
    assert!(warnings.is_empty());
}

// ===========================================================================
// 10. ConfigDefaults edge cases
// ===========================================================================

#[test]
fn config_defaults_values() {
    assert_eq!(ConfigDefaults::default_max_turns(), 25);
    assert_eq!(ConfigDefaults::default_max_budget(), 1.0);
    assert_eq!(ConfigDefaults::default_model(), "gpt-4");
}

#[test]
fn config_defaults_apply_preserves_vendor_and_env() {
    let mut wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            env: full_env_map(),
            ..Default::default()
        })
        .build();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.vendor.len(), 3);
    assert_eq!(wo.config.env.len(), 2);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
}

#[test]
fn config_defaults_apply_idempotent() {
    let mut wo = builder_minimal().build();
    ConfigDefaults::apply_defaults(&mut wo);
    let turns1 = wo.config.max_turns;
    let budget1 = wo.config.max_budget_usd;
    let model1 = wo.config.model.clone();
    ConfigDefaults::apply_defaults(&mut wo);
    assert_eq!(wo.config.max_turns, turns1);
    assert_eq!(wo.config.max_budget_usd, budget1);
    assert_eq!(wo.config.model, model1);
}

// ===========================================================================
// 11. WorkOrder cloning deep equality
// ===========================================================================

#[test]
fn clone_deep_vendor_equality() {
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            env: full_env_map(),
            model: Some("m".into()),
            max_turns: Some(5),
            max_budget_usd: Some(0.5),
        })
        .build();
    let c = wo.clone();
    assert_eq!(
        serde_json::to_string(&wo).unwrap(),
        serde_json::to_string(&c).unwrap()
    );
}

#[test]
fn clone_mutate_original_does_not_affect_clone() {
    let mut wo = WorkOrderBuilder::new("original")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            ..Default::default()
        })
        .build();
    let cloned = wo.clone();
    wo.policy.allowed_tools.push("write".into());
    wo.task = "mutated".into();
    assert_eq!(cloned.policy.allowed_tools.len(), 1);
    assert_eq!(cloned.task, "original");
}

#[test]
fn clone_mutate_clone_does_not_affect_original() {
    let wo = WorkOrderBuilder::new("original")
        .config(RuntimeConfig {
            vendor: full_vendor_map(),
            ..Default::default()
        })
        .build();
    let mut cloned = wo.clone();
    cloned.config.vendor.insert("new_key".into(), json!(99));
    assert!(!wo.config.vendor.contains_key("new_key"));
}

// ===========================================================================
// 12. Extension trait deeper tests
// ===========================================================================

#[test]
fn ext_is_code_task_case_insensitive() {
    let wo = WorkOrderBuilder::new("IMPLEMENT the feature").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_is_code_task_fix_keyword() {
    let wo = WorkOrderBuilder::new("Please fix the problem").build();
    assert!(wo.is_code_task());
}

#[test]
fn ext_vendor_config_returns_correct_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.5}));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let val = wo.vendor_config("openai").unwrap();
    assert_eq!(val["temperature"], 0.5);
}

#[test]
fn ext_vendor_config_missing_key() {
    let wo = builder_minimal().build();
    assert!(wo.vendor_config("nonexistent").is_none());
}

#[test]
fn ext_required_capabilities_grep_keyword() {
    let wo = WorkOrderBuilder::new("grep for patterns").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolGrep));
}

#[test]
fn ext_required_capabilities_search_keyword() {
    let wo = WorkOrderBuilder::new("search the codebase").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolGrep));
}

#[test]
fn ext_required_capabilities_command_keyword() {
    let wo = WorkOrderBuilder::new("run a command to build").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolBash));
}

#[test]
fn ext_task_summary_exact_boundary() {
    let wo = WorkOrderBuilder::new("1234567890").build();
    assert_eq!(wo.task_summary(10), "1234567890");
    assert_eq!(wo.task_summary(9), "123456789…");
}

#[test]
fn ext_tool_budget_remaining_returns_configured() {
    let wo = WorkOrderBuilder::new("t").max_turns(1).build();
    assert_eq!(wo.tool_budget_remaining(), Some(1));
}

// ===========================================================================
// 13. Serde edge cases: values, types, partial JSON
// ===========================================================================

#[test]
fn deserialize_work_order_from_value() {
    let wo = builder_minimal().build();
    let val = serde_json::to_value(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_value(val).unwrap();
    assert_eq!(wo.id, wo2.id);
}

#[test]
fn deserialize_rejects_numeric_task() {
    let mut val = serde_json::to_value(builder_minimal().build()).unwrap();
    val["task"] = json!(12345);
    let result = serde_json::from_value::<WorkOrder>(val);
    assert!(result.is_err());
}

#[test]
fn deserialize_rejects_null_task() {
    let mut val = serde_json::to_value(builder_minimal().build()).unwrap();
    val["task"] = serde_json::Value::Null;
    let result = serde_json::from_value::<WorkOrder>(val);
    assert!(result.is_err());
}

#[test]
fn deserialize_rejects_invalid_uuid() {
    let mut val = serde_json::to_value(builder_minimal().build()).unwrap();
    val["id"] = json!("not-a-uuid");
    let result = serde_json::from_value::<WorkOrder>(val);
    assert!(result.is_err());
}

#[test]
fn deserialize_accepts_nil_uuid() {
    let mut val = serde_json::to_value(builder_minimal().build()).unwrap();
    val["id"] = json!("00000000-0000-0000-0000-000000000000");
    let wo: WorkOrder = serde_json::from_value(val).unwrap();
    assert!(wo.id.is_nil());
}

#[test]
fn vendor_value_types_bool_string_number_null_array_object() {
    let mut vendor = BTreeMap::new();
    vendor.insert("bool_val".into(), json!(true));
    vendor.insert("string_val".into(), json!("hello"));
    vendor.insert("number_val".into(), json!(42));
    vendor.insert("null_val".into(), serde_json::Value::Null);
    vendor.insert("array_val".into(), json!([1, 2, 3]));
    vendor.insert("object_val".into(), json!({"nested": true}));

    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["bool_val"], true);
    assert_eq!(wo2.config.vendor["string_val"], "hello");
    assert_eq!(wo2.config.vendor["number_val"], 42);
    assert!(wo2.config.vendor["null_val"].is_null());
    assert_eq!(wo2.config.vendor["array_val"].as_array().unwrap().len(), 3);
    assert_eq!(wo2.config.vendor["object_val"]["nested"], true);
}

#[test]
fn deeply_nested_vendor_value() {
    let deep = json!({"l1": {"l2": {"l3": {"l4": {"l5": "leaf"}}}}});
    let mut vendor = BTreeMap::new();
    vendor.insert("deep".into(), deep);
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(
        wo2.config.vendor["deep"]["l1"]["l2"]["l3"]["l4"]["l5"],
        "leaf"
    );
}

#[test]
fn float_budget_precision_roundtrip() {
    let wo = WorkOrderBuilder::new("t").max_budget_usd(0.1 + 0.2).build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    let diff = (wo2.config.max_budget_usd.unwrap() - (0.1_f64 + 0.2)).abs();
    assert!(diff < f64::EPSILON);
}

// ===========================================================================
// 14. WorkOrder ID uniqueness
// ===========================================================================

#[test]
fn hundred_work_orders_have_unique_ids() {
    let ids: Vec<_> = (0..100).map(|_| builder_minimal().build().id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 100);
}

#[test]
fn work_order_id_is_uuid_v4_format() {
    let wo = builder_minimal().build();
    let id_str = wo.id.to_string();
    // UUID v4 format: xxxxxxxx-xxxx-4xxx-[89ab]xxx-xxxxxxxxxxxx
    assert_eq!(id_str.len(), 36);
    assert_eq!(&id_str[14..15], "4");
}

// ===========================================================================
// 15. CONTRACT_VERSION in context
// ===========================================================================

#[test]
fn contract_version_format() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
    assert!(CONTRACT_VERSION.contains("v0"));
}

// ===========================================================================
// 16. Workspace spec variations
// ===========================================================================

#[test]
fn workspace_root_relative() {
    let wo = WorkOrderBuilder::new("t").root("./relative").build();
    assert_eq!(wo.workspace.root, "./relative");
}

#[test]
fn workspace_root_absolute_unix() {
    let wo = WorkOrderBuilder::new("t").root("/absolute/path").build();
    assert_eq!(wo.workspace.root, "/absolute/path");
}

#[test]
fn workspace_root_empty_string() {
    let wo = WorkOrderBuilder::new("t").root("").build();
    assert_eq!(wo.workspace.root, "");
}

#[test]
fn workspace_include_with_complex_globs() {
    let globs = vec![
        "**/*.rs".into(),
        "!test_*".into(),
        "src/{a,b,c}/*.rs".into(),
        "docs/[A-Z]*.md".into(),
    ];
    let wo = WorkOrderBuilder::new("t").include(globs.clone()).build();
    assert_eq!(wo.workspace.include, globs);
}

// ===========================================================================
// 17. Context packet edge cases
// ===========================================================================

#[test]
fn context_files_with_special_paths() {
    let wo = WorkOrderBuilder::new("t")
        .context(ContextPacket {
            files: vec![
                "src/main.rs".into(),
                "../sibling/file.txt".into(),
                "dir with spaces/file.rs".into(),
                "unicode/日本語.rs".into(),
            ],
            snippets: vec![],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.context.files.len(), 4);
    assert_eq!(wo2.context.files[3], "unicode/日本語.rs");
}

#[test]
fn context_snippet_empty_content() {
    let wo = WorkOrderBuilder::new("t")
        .context(ContextPacket {
            files: vec![],
            snippets: vec![ContextSnippet {
                name: "empty".into(),
                content: String::new(),
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(wo2.context.snippets[0].content.is_empty());
}

#[test]
fn context_snippet_large_content() {
    let content = "x".repeat(50_000);
    let wo = WorkOrderBuilder::new("t")
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

// ===========================================================================
// 18. Policy profile combinations
// ===========================================================================

#[test]
fn policy_both_allow_and_deny_tools() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            disallowed_tools: vec!["bash".into(), "exec".into()],
            ..Default::default()
        })
        .build();
    assert_eq!(wo.policy.allowed_tools.len(), 2);
    assert_eq!(wo.policy.disallowed_tools.len(), 2);
}

#[test]
fn policy_network_rules_roundtrip() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            allow_network: vec!["api.github.com".into(), "*.npmjs.org".into()],
            deny_network: vec!["*.evil.com".into(), "localhost".into()],
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.allow_network.len(), 2);
    assert_eq!(wo2.policy.deny_network.len(), 2);
}

#[test]
fn policy_require_approval_roundtrip() {
    let wo = WorkOrderBuilder::new("t")
        .policy(PolicyProfile {
            require_approval_for: vec!["deploy".into(), "delete".into(), "publish".into()],
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.policy.require_approval_for.len(), 3);
}

// ===========================================================================
// 19. Capability enum exhaustiveness and ordering
// ===========================================================================

#[test]
fn capability_ordering_is_consistent() {
    let a = Capability::Streaming;
    let b = Capability::ToolRead;
    // BTreeMap uses Ord, so capabilities must have consistent ordering
    let mut map = BTreeMap::new();
    map.insert(b.clone(), "b");
    map.insert(a.clone(), "a");
    let keys: Vec<_> = map.keys().collect();
    // Streaming sorts before ToolRead (by derived Ord on enum discriminant)
    assert_eq!(*keys[0], Capability::Streaming);
    assert_eq!(*keys[1], Capability::ToolRead);
}

#[test]
fn all_min_support_variants_roundtrip() {
    for (variant, expected) in [
        (MinSupport::Native, r#""native""#),
        (MinSupport::Emulated, r#""emulated""#),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let rt: MinSupport = serde_json::from_str(&json).unwrap();
        let rt_json = serde_json::to_string(&rt).unwrap();
        assert_eq!(rt_json, expected);
    }
}

#[test]
fn all_execution_lane_variants_roundtrip() {
    for (variant, expected) in [
        (ExecutionLane::PatchFirst, r#""patch_first""#),
        (ExecutionLane::WorkspaceFirst, r#""workspace_first""#),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let rt: ExecutionLane = serde_json::from_str(&json).unwrap();
        let rt_json = serde_json::to_string(&rt).unwrap();
        assert_eq!(rt_json, expected);
    }
}

#[test]
fn all_workspace_mode_variants_roundtrip() {
    for (variant, expected) in [
        (WorkspaceMode::PassThrough, r#""pass_through""#),
        (WorkspaceMode::Staged, r#""staged""#),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, expected);
        let rt: WorkspaceMode = serde_json::from_str(&json).unwrap();
        let rt_json = serde_json::to_string(&rt).unwrap();
        assert_eq!(rt_json, expected);
    }
}

// ===========================================================================
// 20. Miscellaneous edge cases
// ===========================================================================

#[test]
fn empty_string_model_is_some() {
    let wo = WorkOrderBuilder::new("t").model("").build();
    assert_eq!(wo.config.model, Some(String::new()));
}

#[test]
fn vendor_with_empty_object_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("empty_obj".into(), json!({}));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(
        wo2.config.vendor["empty_obj"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn vendor_with_empty_array_value() {
    let mut vendor = BTreeMap::new();
    vendor.insert("empty_arr".into(), json!([]));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert!(
        wo2.config.vendor["empty_arr"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn env_with_empty_value() {
    let mut env = BTreeMap::new();
    env.insert("EMPTY".into(), String::new());
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            env,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.env["EMPTY"], "");
}

#[test]
fn work_order_json_keys_are_expected_top_level() {
    let wo = builder_minimal().build();
    let val = serde_json::to_value(&wo).unwrap();
    let obj = val.as_object().unwrap();
    let expected = [
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ];
    for k in &expected {
        assert!(obj.contains_key(*k), "missing top-level key: {k}");
    }
    assert_eq!(obj.len(), expected.len());
}

#[test]
fn config_json_keys_are_expected() {
    let wo = builder_minimal().build();
    let val = serde_json::to_value(&wo).unwrap();
    let cfg = val["config"].as_object().unwrap();
    let expected = ["model", "vendor", "env", "max_budget_usd", "max_turns"];
    for k in &expected {
        assert!(cfg.contains_key(*k), "missing config key: {k}");
    }
}

#[test]
fn workspace_json_keys_are_expected() {
    let wo = builder_minimal().build();
    let val = serde_json::to_value(&wo).unwrap();
    let ws = val["workspace"].as_object().unwrap();
    let expected = ["root", "mode", "include", "exclude"];
    for k in &expected {
        assert!(ws.contains_key(*k), "missing workspace key: {k}");
    }
}

#[test]
fn vendor_numeric_json_value_preserves_type() {
    let mut vendor = BTreeMap::new();
    vendor.insert("int_val".into(), json!(42));
    vendor.insert("float_val".into(), json!(1.2345));
    let wo = WorkOrderBuilder::new("t")
        .config(RuntimeConfig {
            vendor,
            ..Default::default()
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo2.config.vendor["int_val"], 42);
    assert!((wo2.config.vendor["float_val"].as_f64().unwrap() - 1.2345).abs() < f64::EPSILON);
}

#[test]
fn builder_task_from_string_type() {
    let s = String::from("from owned string");
    let wo = WorkOrderBuilder::new(s).build();
    assert_eq!(wo.task, "from owned string");
}

#[test]
fn builder_task_from_str_ref() {
    let wo = WorkOrderBuilder::new("from &str").build();
    assert_eq!(wo.task, "from &str");
}

#[test]
fn builder_root_from_string_type() {
    let wo = WorkOrderBuilder::new("t")
        .root(String::from("/owned"))
        .build();
    assert_eq!(wo.workspace.root, "/owned");
}

#[test]
fn builder_model_from_string_type() {
    let wo = WorkOrderBuilder::new("t")
        .model(String::from("owned-model"))
        .build();
    assert_eq!(wo.config.model.as_deref(), Some("owned-model"));
}
