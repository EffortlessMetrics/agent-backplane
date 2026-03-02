// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::validate::validate_receipt;
use abp_core::*;
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// WorkOrderBuilder tests
// ---------------------------------------------------------------------------

#[test]
fn wo_minimal_build() {
    let wo = WorkOrderBuilder::new("do something").build();

    assert_eq!(wo.task, "do something");
    assert!(!wo.id.is_nil());
    assert_eq!(wo.workspace.root, ".");
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    assert!(wo.requirements.required.is_empty());
}

#[test]
fn wo_full_build() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.lock".into()],
        allow_network: vec!["example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["write".into()],
    };
    let ctx = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "be careful".into(),
        }],
    };
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let mut vendor = BTreeMap::new();
    vendor.insert("key".to_string(), serde_json::json!("val"));
    let mut env = BTreeMap::new();
    env.insert("FOO".to_string(), "bar".to_string());
    let config = RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor,
        env,
        max_budget_usd: Some(1.5),
        max_turns: Some(20),
    };

    let wo = WorkOrderBuilder::new("full task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .context(ctx)
        .policy(policy)
        .requirements(reqs)
        .config(config)
        .build();

    assert_eq!(wo.task, "full task");
    assert_eq!(wo.workspace.root, "/tmp/ws");
    assert_eq!(wo.workspace.include, vec!["src/**"]);
    assert_eq!(wo.workspace.exclude, vec!["target/**"]);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(20));
    assert_eq!(wo.config.max_budget_usd, Some(1.5));
    assert_eq!(wo.config.env.get("FOO").unwrap(), "bar");
    assert_eq!(
        wo.config.vendor.get("key").unwrap(),
        &serde_json::json!("val")
    );
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.policy.disallowed_tools, vec!["bash"]);
    assert_eq!(wo.context.files, vec!["README.md"]);
    assert_eq!(wo.context.snippets.len(), 1);
    assert_eq!(wo.requirements.required.len(), 1);
}

#[test]
fn wo_task_description_preserved() {
    let wo = WorkOrderBuilder::new("Fix the login bug in auth module").build();
    assert_eq!(wo.task, "Fix the login bug in auth module");
}

#[test]
fn wo_model_override() {
    let wo = WorkOrderBuilder::new("task").model("claude-3").build();
    assert_eq!(wo.config.model.as_deref(), Some("claude-3"));
}

#[test]
fn wo_max_turns() {
    let wo = WorkOrderBuilder::new("task").max_turns(42).build();
    assert_eq!(wo.config.max_turns, Some(42));
}

#[test]
fn wo_budget_limits() {
    let wo = WorkOrderBuilder::new("task").max_budget_usd(9.99).build();
    assert_eq!(wo.config.max_budget_usd, Some(9.99));
}

#[test]
fn wo_tool_allowlist() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into(), "glob".into()],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["read", "write", "glob"]);
}

#[test]
fn wo_policy_profile_attached() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["/etc/**".into()],
        deny_write: vec!["/usr/**".into()],
        allow_network: vec!["*.github.com".into()],
        deny_network: vec!["*.malware.net".into()],
        require_approval_for: vec!["write".into()],
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.deny_read, vec!["/etc/**"]);
    assert_eq!(wo.policy.deny_write, vec!["/usr/**"]);
    assert_eq!(wo.policy.allow_network, vec!["*.github.com"]);
    assert_eq!(wo.policy.deny_network, vec!["*.malware.net"]);
    assert_eq!(wo.policy.require_approval_for, vec!["write"]);
}

#[test]
fn wo_vendor_config_passthrough() {
    let mut vendor = BTreeMap::new();
    vendor.insert("anthropic".into(), serde_json::json!({"max_tokens": 4096}));
    vendor.insert("openai".into(), serde_json::json!({"temperature": 0.7}));
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(
        wo.config.vendor["anthropic"],
        serde_json::json!({"max_tokens": 4096})
    );
    assert_eq!(
        wo.config.vendor["openai"],
        serde_json::json!({"temperature": 0.7})
    );
}

#[test]
fn wo_env_btreemap_preserved() {
    let mut env = BTreeMap::new();
    env.insert("RUST_LOG".into(), "debug".into());
    env.insert("APP_ENV".into(), "test".into());
    let config = RuntimeConfig {
        env,
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("task").config(config).build();
    assert_eq!(wo.config.env.len(), 2);
    assert_eq!(wo.config.env["RUST_LOG"], "debug");
    assert_eq!(wo.config.env["APP_ENV"], "test");
}

#[test]
fn wo_context_items_preserved() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![
            ContextSnippet {
                name: "snippet1".into(),
                content: "first snippet".into(),
            },
            ContextSnippet {
                name: "snippet2".into(),
                content: "second snippet".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    assert_eq!(wo.context.files.len(), 2);
    assert_eq!(wo.context.snippets.len(), 2);
    assert_eq!(wo.context.snippets[0].name, "snippet1");
    assert_eq!(wo.context.snippets[1].content, "second snippet");
}

#[test]
fn wo_builder_reusability_independent_orders() {
    // Builder consumes self, but the same pattern yields independent work orders.
    let wo1 = WorkOrderBuilder::new("task A").model("gpt-4").build();
    let wo2 = WorkOrderBuilder::new("task A").model("gpt-4").build();
    assert_ne!(wo1.id, wo2.id);
    assert_eq!(wo1.task, wo2.task);
    assert_eq!(wo1.config.model, wo2.config.model);
}

#[test]
fn wo_default_values_correct() {
    let wo = WorkOrderBuilder::new("task").build();
    assert_eq!(wo.workspace.root, ".");
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_turns.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.vendor.is_empty());
    assert!(wo.config.env.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.policy.disallowed_tools.is_empty());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
}

#[test]
fn wo_unique_id_generation() {
    let ids: Vec<Uuid> = (0..10)
        .map(|_| WorkOrderBuilder::new("task").build().id)
        .collect();
    // All IDs must be unique.
    for (i, a) in ids.iter().enumerate() {
        assert!(!a.is_nil());
        for b in &ids[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

// ---------------------------------------------------------------------------
// ReceiptBuilder tests
// ---------------------------------------------------------------------------

#[test]
fn receipt_minimal_from_work_order_id() {
    let wo_id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("mock").work_order_id(wo_id).build();
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn receipt_full_with_all_fields() {
    let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let wo_id = Uuid::new_v4();

    let event = AgentEvent {
        ts: start,
        kind: AgentEventKind::RunStarted {
            message: "hello".into(),
        },
        ext: None,
    };
    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let receipt = ReceiptBuilder::new("test-backend")
        .backend_id("overridden")
        .outcome(Outcome::Partial)
        .started_at(start)
        .finished_at(end)
        .work_order_id(wo_id)
        .add_trace_event(event)
        .add_artifact(artifact)
        .backend_version("1.0")
        .adapter_version("0.1")
        .mode(ExecutionMode::Passthrough)
        .capabilities(caps)
        .usage_raw(serde_json::json!({"tokens": 100}))
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        })
        .verification(VerificationReport {
            harness_ok: true,
            ..Default::default()
        })
        .build();

    assert_eq!(receipt.backend.id, "overridden");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.1"));
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.meta.work_order_id, wo_id);
    assert_eq!(receipt.meta.started_at, start);
    assert_eq!(receipt.meta.finished_at, end);
    assert_eq!(receipt.meta.duration_ms, 5000);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    assert_eq!(receipt.trace.len(), 1);
    assert_eq!(receipt.artifacts.len(), 1);
    assert!(receipt.verification.harness_ok);
    assert!(receipt.capabilities.contains_key(&Capability::ToolRead));
}

#[test]
fn receipt_outcome_complete() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Complete).build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn receipt_outcome_failed() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_outcome_partial() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn receipt_events_list_preserved() {
    let now = Utc::now();
    let make_event = |msg: &str| AgentEvent {
        ts: now,
        kind: AgentEventKind::AssistantMessage {
            text: msg.to_string(),
        },
        ext: None,
    };

    let receipt = ReceiptBuilder::new("mock")
        .add_trace_event(make_event("first"))
        .add_trace_event(make_event("second"))
        .add_trace_event(make_event("third"))
        .build();

    assert_eq!(receipt.trace.len(), 3);
}

#[test]
fn receipt_hash_is_generated() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hashing should succeed");

    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_hash_changes_when_content_changes() {
    let start = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 6, 1, 0, 1, 0).unwrap();
    let wo_id = Uuid::nil();

    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .work_order_id(wo_id)
        .started_at(start)
        .finished_at(end)
        .with_hash()
        .unwrap();

    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .work_order_id(wo_id)
        .started_at(start)
        .finished_at(end)
        .with_hash()
        .unwrap();

    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_timing_fields() {
    let start = Utc.with_ymd_and_hms(2025, 3, 15, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 3, 15, 10, 2, 30).unwrap();

    let receipt = ReceiptBuilder::new("mock")
        .started_at(start)
        .finished_at(end)
        .build();

    assert_eq!(receipt.meta.started_at, start);
    assert_eq!(receipt.meta.finished_at, end);
    assert_eq!(receipt.meta.duration_ms, 150_000);
}

#[test]
fn receipt_token_cost_tracking() {
    let usage = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.05),
    };

    let receipt = ReceiptBuilder::new("mock").usage(usage).build();

    assert_eq!(receipt.usage.input_tokens, Some(1000));
    assert_eq!(receipt.usage.output_tokens, Some(500));
    assert_eq!(receipt.usage.cache_read_tokens, Some(200));
    assert_eq!(receipt.usage.cache_write_tokens, Some(100));
    assert_eq!(receipt.usage.request_units, Some(3));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.05));
}

#[test]
fn receipt_build_without_hash_then_add() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert!(receipt.receipt_sha256.is_none());

    let hashed = receipt.with_hash().expect("hashing should succeed");
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_serde_roundtrip() {
    let start = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 6, 1, 12, 5, 0).unwrap();
    let wo_id = Uuid::new_v4();

    let event = AgentEvent {
        ts: start,
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "foo.txt"}),
        },
        ext: None,
    };

    let original = ReceiptBuilder::new("serde-test")
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(end)
        .work_order_id(wo_id)
        .add_trace_event(event)
        .backend_version("2.0")
        .adapter_version("1.0")
        .mode(ExecutionMode::Passthrough)
        .usage(UsageNormalized {
            input_tokens: Some(42),
            output_tokens: Some(84),
            estimated_cost_usd: Some(0.01),
            ..Default::default()
        })
        .usage_raw(serde_json::json!({"raw": true}))
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&original).expect("serialize");
    let deserialized: Receipt = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.backend.id, original.backend.id);
    assert_eq!(
        deserialized.backend.backend_version,
        original.backend.backend_version
    );
    assert_eq!(
        deserialized.backend.adapter_version,
        original.backend.adapter_version
    );
    assert_eq!(deserialized.outcome, original.outcome);
    assert_eq!(deserialized.mode, original.mode);
    assert_eq!(deserialized.meta.work_order_id, original.meta.work_order_id);
    assert_eq!(deserialized.meta.run_id, original.meta.run_id);
    assert_eq!(deserialized.meta.started_at, original.meta.started_at);
    assert_eq!(deserialized.meta.finished_at, original.meta.finished_at);
    assert_eq!(deserialized.meta.duration_ms, original.meta.duration_ms);
    assert_eq!(
        deserialized.meta.contract_version,
        original.meta.contract_version
    );
    assert_eq!(deserialized.receipt_sha256, original.receipt_sha256);
    assert_eq!(deserialized.trace.len(), original.trace.len());
    assert_eq!(deserialized.usage.input_tokens, original.usage.input_tokens);
    assert_eq!(
        deserialized.usage.output_tokens,
        original.usage.output_tokens
    );
    assert_eq!(deserialized.usage_raw, original.usage_raw);
}

#[test]
fn receipt_from_work_order_reference() {
    let wo = WorkOrderBuilder::new("linked task").build();
    let receipt = ReceiptBuilder::new("mock").work_order_id(wo.id).build();

    assert_eq!(receipt.meta.work_order_id, wo.id);
}

#[test]
fn receipt_error_event_in_failed_outcome() {
    let now = Utc::now();
    let error_event = AgentEvent {
        ts: now,
        kind: AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: None,
        },
        ext: None,
    };

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_trace_event(error_event)
        .build();

    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.trace.len(), 1);
    match &receipt.trace[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert_eq!(message, "something went wrong");
        }
        other => panic!("expected Error event, got {:?}", other),
    }
}

#[test]
fn receipt_model_field_via_work_order() {
    // Receipt doesn't carry its own model, but the RuntimeConfig on WorkOrder does.
    let wo = WorkOrderBuilder::new("task").model("o3-mini").build();
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));

    // Verify model survives serde roundtrip on the work order side.
    let json = serde_json::to_string(&wo).unwrap();
    let deser: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.config.model.as_deref(), Some("o3-mini"));
}

#[test]
fn receipt_builder_custom_vendor_data() {
    let raw = serde_json::json!({
        "vendor": "anthropic",
        "model": "claude-3-opus",
        "usage": {"input_tokens": 512, "output_tokens": 256}
    });

    let receipt = ReceiptBuilder::new("anthropic-adapter")
        .usage_raw(raw.clone())
        .build();

    assert_eq!(receipt.usage_raw, raw);
}

#[test]
fn receipt_produces_valid_receipt() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hashing should succeed");

    validate_receipt(&receipt).expect("receipt should be valid");
}

#[test]
fn receipt_default_values_are_sensible() {
    let receipt = ReceiptBuilder::new("test").build();

    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.meta.work_order_id, Uuid::nil());
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
    assert!(receipt.backend.backend_version.is_none());
    assert!(receipt.backend.adapter_version.is_none());
    assert!(receipt.trace.is_empty());
    assert!(receipt.artifacts.is_empty());
    assert!(receipt.receipt_sha256.is_none());
    assert!(receipt.capabilities.is_empty());
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    assert_eq!(receipt.meta.duration_ms, 0);
}

#[test]
fn receipt_unique_run_ids() {
    let ids: Vec<Uuid> = (0..10)
        .map(|_| ReceiptBuilder::new("mock").build().meta.run_id)
        .collect();
    for (i, a) in ids.iter().enumerate() {
        assert!(!a.is_nil());
        for b in &ids[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn receipt_verification_report() {
    let verification = VerificationReport {
        git_diff: Some("diff --git a/foo b/foo".into()),
        git_status: Some("M foo".into()),
        harness_ok: true,
    };

    let receipt = ReceiptBuilder::new("mock")
        .verification(verification)
        .build();

    assert!(receipt.verification.harness_ok);
    assert_eq!(
        receipt.verification.git_diff.as_deref(),
        Some("diff --git a/foo b/foo")
    );
    assert_eq!(receipt.verification.git_status.as_deref(), Some("M foo"));
}

#[test]
fn receipt_capabilities_manifest() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::Streaming, SupportLevel::Unsupported);

    let receipt = ReceiptBuilder::new("mock").capabilities(caps).build();

    assert_eq!(receipt.capabilities.len(), 3);
    assert!(receipt.capabilities.contains_key(&Capability::ToolRead));
    assert!(receipt.capabilities.contains_key(&Capability::ToolWrite));
    assert!(receipt.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn wo_serde_roundtrip() {
    let wo = WorkOrderBuilder::new("roundtrip test")
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(2.0)
        .root("/workspace")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();

    let json = serde_json::to_string(&wo).unwrap();
    let deser: WorkOrder = serde_json::from_str(&json).unwrap();

    assert_eq!(deser.id, wo.id);
    assert_eq!(deser.task, wo.task);
    assert_eq!(deser.config.model, wo.config.model);
    assert_eq!(deser.config.max_turns, wo.config.max_turns);
    assert_eq!(deser.config.max_budget_usd, wo.config.max_budget_usd);
    assert_eq!(deser.workspace.root, wo.workspace.root);
}

#[test]
fn wo_include_exclude_globs() {
    let wo = WorkOrderBuilder::new("task")
        .include(vec!["src/**/*.rs".into(), "Cargo.toml".into()])
        .exclude(vec!["target/**".into()])
        .build();

    assert_eq!(wo.workspace.include.len(), 2);
    assert_eq!(wo.workspace.exclude.len(), 1);
    assert_eq!(wo.workspace.include[0], "src/**/*.rs");
}

#[test]
fn wo_workspace_mode_passthrough() {
    let wo = WorkOrderBuilder::new("task")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();

    let json = serde_json::to_string(&wo.workspace.mode).unwrap();
    assert_eq!(json, "\"pass_through\"");
}

#[test]
fn wo_workspace_mode_staged_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let json = serde_json::to_string(&wo.workspace.mode).unwrap();
    assert_eq!(json, "\"staged\"");
}

#[test]
fn receipt_hash_deterministic() {
    let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();
    let wo_id = Uuid::nil();

    let build = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .work_order_id(wo_id)
            .started_at(start)
            .finished_at(end)
            .build()
    };

    // Build two receipts with same content but different run_ids.
    // Hash must differ because run_id is part of the hash input.
    let r1 = build().with_hash().unwrap();
    let r2 = build().with_hash().unwrap();
    // Different run_ids → different hashes.
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);

    // But the same receipt hashed twice yields the same hash.
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}
