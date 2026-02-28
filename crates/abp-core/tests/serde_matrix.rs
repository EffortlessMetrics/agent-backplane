// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive serde compatibility matrix tests for all abp-core contract types.
//!
//! For each serializable type we verify:
//! 1. JSON roundtrip (serialize → deserialize → serialize → compare)
//! 2. Forward compat (extra unknown fields are tolerated)
//! 3. Missing optional fields deserialize correctly
//! 4. Pretty vs compact JSON both deserialize identically
//! 5. Null values handled correctly (null ≡ absent for Option<T>)

use std::collections::BTreeMap;

use abp_core::*;
use chrono::Utc;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Assert JSON roundtrip: serialize → deserialize → serialize, then compare JSON values.
fn assert_roundtrip<T: Serialize + DeserializeOwned>(val: &T) {
    let json_val = serde_json::to_value(val).expect("serialize to Value");
    let back: T = serde_json::from_value(json_val.clone()).expect("deserialize from Value");
    let json_val2 = serde_json::to_value(&back).expect("re-serialize to Value");
    assert_eq!(json_val, json_val2, "roundtrip mismatch");
}

/// Assert that both pretty and compact JSON deserialize to the same Value.
fn assert_pretty_compact_equal<T: Serialize + DeserializeOwned>(val: &T) {
    let compact = serde_json::to_string(val).unwrap();
    let pretty = serde_json::to_string_pretty(val).unwrap();
    let from_compact: T = serde_json::from_str(&compact).unwrap();
    let from_pretty: T = serde_json::from_str(&pretty).unwrap();
    let v1 = serde_json::to_value(&from_compact).unwrap();
    let v2 = serde_json::to_value(&from_pretty).unwrap();
    assert_eq!(v1, v2, "pretty vs compact deserialization mismatch");
}

fn sample_work_order_full() -> WorkOrder {
    let mut vendor = BTreeMap::new();
    vendor.insert("openai".into(), json!({"temperature": 0.7}));
    let mut env = BTreeMap::new();
    env.insert("RUST_LOG".into(), "debug".into());

    WorkOrder {
        id: Uuid::nil(),
        task: "Fix all the bugs".into(),
        lane: ExecutionLane::WorkspaceFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "error".into(),
                content: "panic at line 42".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read_file".into()],
            disallowed_tools: vec!["rm".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["bash".into()],
        },
        requirements: CapabilityRequirements {
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
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor,
            env,
            max_budget_usd: Some(1.5),
            max_turns: Some(20),
        },
    }
}

fn sample_work_order_minimal() -> WorkOrder {
    WorkOrderBuilder::new("minimal task").build()
}

fn sample_receipt_full() -> Receipt {
    let now = Utc::now();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Passthrough,
        usage_raw: json!({"prompt_tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.01),
        },
        trace: vec![AgentEvent {
            ts: now,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.diff".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("+line".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn sample_receipt_minimal() -> Receipt {
    ReceiptBuilder::new("mock").outcome(Outcome::Complete).build()
}

// ===========================================================================
// 1. WorkOrder tests
// ===========================================================================

#[test]
fn work_order_full_roundtrip() {
    assert_roundtrip(&sample_work_order_full());
}

#[test]
fn work_order_minimal_roundtrip() {
    assert_roundtrip(&sample_work_order_minimal());
}

#[test]
fn work_order_pretty_vs_compact() {
    assert_pretty_compact_equal(&sample_work_order_full());
    assert_pretty_compact_equal(&sample_work_order_minimal());
}

#[test]
fn work_order_extra_fields_tolerated() {
    let mut v = serde_json::to_value(sample_work_order_full()).unwrap();
    v["future_field"] = json!("ignored");
    v["config"]["new_knob"] = json!(true);
    v["workspace"]["priority"] = json!(1);
    let wo: WorkOrder = serde_json::from_value(v).unwrap();
    assert_eq!(wo.task, "Fix all the bugs");
}

#[test]
fn work_order_null_optional_config_fields() {
    let mut v = serde_json::to_value(sample_work_order_full()).unwrap();
    v["config"]["model"] = Value::Null;
    v["config"]["max_budget_usd"] = Value::Null;
    v["config"]["max_turns"] = Value::Null;
    let wo: WorkOrder = serde_json::from_value(v).unwrap();
    assert!(wo.config.model.is_none());
    assert!(wo.config.max_budget_usd.is_none());
    assert!(wo.config.max_turns.is_none());
}

// ===========================================================================
// 2. Receipt tests
// ===========================================================================

#[test]
fn receipt_full_roundtrip() {
    assert_roundtrip(&sample_receipt_full());
}

#[test]
fn receipt_minimal_roundtrip() {
    assert_roundtrip(&sample_receipt_minimal());
}

#[test]
fn receipt_with_hash_roundtrip() {
    let receipt = sample_receipt_full().with_hash().unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_roundtrip(&receipt);
}

#[test]
fn receipt_without_hash_roundtrip() {
    let receipt = sample_receipt_minimal();
    assert!(receipt.receipt_sha256.is_none());
    assert_roundtrip(&receipt);
}

#[test]
fn receipt_pretty_vs_compact() {
    assert_pretty_compact_equal(&sample_receipt_full());
}

#[test]
fn receipt_extra_fields_tolerated() {
    let mut v = serde_json::to_value(sample_receipt_full()).unwrap();
    v["new_v2_field"] = json!("future");
    v["meta"]["extra"] = json!(999);
    v["backend"]["region"] = json!("us-east-1");
    v["usage"]["total_latency_ms"] = json!(100);
    v["verification"]["lint_ok"] = json!(true);
    let r: Receipt = serde_json::from_value(v).unwrap();
    assert_eq!(r.backend.id, "sidecar:node");
}

#[test]
fn receipt_null_optional_fields() {
    let mut v = serde_json::to_value(sample_receipt_full()).unwrap();
    v["backend"]["backend_version"] = Value::Null;
    v["backend"]["adapter_version"] = Value::Null;
    v["receipt_sha256"] = Value::Null;
    v["usage"]["input_tokens"] = Value::Null;
    v["usage"]["estimated_cost_usd"] = Value::Null;
    v["verification"]["git_diff"] = Value::Null;
    v["verification"]["git_status"] = Value::Null;
    let r: Receipt = serde_json::from_value(v).unwrap();
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
    assert!(r.receipt_sha256.is_none());
    assert!(r.usage.input_tokens.is_none());
    assert!(r.verification.git_diff.is_none());
}

#[test]
fn receipt_missing_mode_defaults_to_mapped() {
    let mut v = serde_json::to_value(sample_receipt_minimal()).unwrap();
    v.as_object_mut().unwrap().remove("mode");
    let r: Receipt = serde_json::from_value(v).unwrap();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

// ===========================================================================
// 3. AgentEvent — each variant roundtrips
// ===========================================================================

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[test]
fn agent_event_run_started_roundtrip() {
    let e = make_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    assert_roundtrip(&e);
    assert_pretty_compact_equal(&e);
}

#[test]
fn agent_event_run_completed_roundtrip() {
    let e = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_assistant_delta_roundtrip() {
    let e = make_event(AgentEventKind::AssistantDelta {
        text: "partial".into(),
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_assistant_message_roundtrip() {
    let e = make_event(AgentEventKind::AssistantMessage {
        text: "full msg".into(),
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_tool_call_roundtrip() {
    let e = make_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: Some("tu_0".into()),
        input: json!({"path": "src/main.rs"}),
    });
    assert_roundtrip(&e);
    assert_pretty_compact_equal(&e);
}

#[test]
fn agent_event_tool_call_null_optionals() {
    let v = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "tool_call",
        "tool_name": "bash",
        "tool_use_id": null,
        "parent_tool_use_id": null,
        "input": {}
    });
    let e: AgentEvent = serde_json::from_value(v).unwrap();
    if let AgentEventKind::ToolCall { tool_use_id, parent_tool_use_id, .. } = &e.kind {
        assert!(tool_use_id.is_none());
        assert!(parent_tool_use_id.is_none());
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn agent_event_tool_result_roundtrip() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu_1".into()),
        output: json!({"content": "file data"}),
        is_error: false,
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_tool_result_error_variant() {
    let e = make_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!("command failed"),
        is_error: true,
    });
    assert_roundtrip(&e);
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["is_error"], true);
}

#[test]
fn agent_event_file_changed_roundtrip() {
    let e = make_event(AgentEventKind::FileChanged {
        path: "src/lib.rs".into(),
        summary: "added function".into(),
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_command_executed_roundtrip() {
    let e = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("all tests passed".into()),
    });
    assert_roundtrip(&e);
    assert_pretty_compact_equal(&e);
}

#[test]
fn agent_event_command_executed_null_optionals() {
    let v = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "command_executed",
        "command": "ls",
        "exit_code": null,
        "output_preview": null
    });
    let e: AgentEvent = serde_json::from_value(v).unwrap();
    if let AgentEventKind::CommandExecuted { exit_code, output_preview, .. } = &e.kind {
        assert!(exit_code.is_none());
        assert!(output_preview.is_none());
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn agent_event_warning_roundtrip() {
    let e = make_event(AgentEventKind::Warning {
        message: "careful".into(),
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_error_roundtrip() {
    let e = make_event(AgentEventKind::Error {
        message: "boom".into(),
    });
    assert_roundtrip(&e);
}

#[test]
fn agent_event_with_ext_roundtrip() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: Some(ext),
    };
    assert_roundtrip(&e);
}

#[test]
fn agent_event_extra_fields_tolerated() {
    let v = json!({
        "ts": "2025-01-01T00:00:00Z",
        "type": "warning",
        "message": "oops",
        "severity": "high"
    });
    let e: AgentEvent = serde_json::from_value(v).unwrap();
    if let AgentEventKind::Warning { message } = &e.kind {
        assert_eq!(message, "oops");
    } else {
        panic!("expected Warning");
    }
}

// ===========================================================================
// 4. Capability — each variant roundtrips
// ===========================================================================

#[test]
fn capability_all_variants_roundtrip() {
    let variants = vec![
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
    for cap in &variants {
        assert_roundtrip(cap);
        // Also verify string form deserializes back
        let s = serde_json::to_string(cap).unwrap();
        let back: Capability = serde_json::from_str(&s).unwrap();
        assert_eq!(*cap, back);
    }
}

#[test]
fn capability_pretty_vs_compact() {
    // Capabilities are simple strings, but test within a manifest context
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::McpServer, SupportLevel::Emulated);
    assert_pretty_compact_equal(&manifest);
}

// ===========================================================================
// 5. SupportLevel — each variant roundtrips
// ===========================================================================

#[test]
fn support_level_native_roundtrip() {
    assert_roundtrip(&SupportLevel::Native);
}

#[test]
fn support_level_emulated_roundtrip() {
    assert_roundtrip(&SupportLevel::Emulated);
}

#[test]
fn support_level_unsupported_roundtrip() {
    assert_roundtrip(&SupportLevel::Unsupported);
}

#[test]
fn support_level_restricted_roundtrip() {
    let r = SupportLevel::Restricted {
        reason: "policy forbids".into(),
    };
    assert_roundtrip(&r);
    assert_pretty_compact_equal(&r);
}

#[test]
fn support_level_restricted_extra_fields() {
    // Restricted is externally tagged: {"restricted": {"reason": "..."}}
    let v = json!({"restricted": {"reason": "blocked", "details": "extra"}});
    let sl: SupportLevel = serde_json::from_value(v).unwrap();
    if let SupportLevel::Restricted { reason } = &sl {
        assert_eq!(reason, "blocked");
    } else {
        panic!("expected Restricted");
    }
}

// ===========================================================================
// 6. MinSupport — each variant roundtrips
// ===========================================================================

#[test]
fn min_support_all_variants_roundtrip() {
    assert_roundtrip(&MinSupport::Native);
    assert_roundtrip(&MinSupport::Emulated);

    let native_json = serde_json::to_value(&MinSupport::Native).unwrap();
    assert_eq!(native_json, json!("native"));
    let emulated_json = serde_json::to_value(&MinSupport::Emulated).unwrap();
    assert_eq!(emulated_json, json!("emulated"));
}

#[test]
fn min_support_pretty_vs_compact() {
    assert_pretty_compact_equal(&MinSupport::Native);
    assert_pretty_compact_equal(&MinSupport::Emulated);
}

// ===========================================================================
// 7. PolicyProfile — full and empty
// ===========================================================================

#[test]
fn policy_profile_empty_roundtrip() {
    let p = PolicyProfile::default();
    assert_roundtrip(&p);
    let v = serde_json::to_value(&p).unwrap();
    assert!(v["allowed_tools"].as_array().unwrap().is_empty());
    assert!(v["disallowed_tools"].as_array().unwrap().is_empty());
}

#[test]
fn policy_profile_full_roundtrip() {
    let p = PolicyProfile {
        allowed_tools: vec!["read_file".into(), "write_file".into()],
        disallowed_tools: vec!["rm".into()],
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        deny_write: vec!["Cargo.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["bash".into(), "execute_command".into()],
    };
    assert_roundtrip(&p);
    assert_pretty_compact_equal(&p);
}

#[test]
fn policy_profile_extra_fields_tolerated() {
    let v = json!({
        "allowed_tools": [],
        "disallowed_tools": [],
        "deny_read": [],
        "deny_write": [],
        "allow_network": [],
        "deny_network": [],
        "require_approval_for": [],
        "max_file_size": 1048576
    });
    let p: PolicyProfile = serde_json::from_value(v).unwrap();
    assert!(p.allowed_tools.is_empty());
}

// ===========================================================================
// 8. ExecutionMode — each variant
// ===========================================================================

#[test]
fn execution_mode_all_variants_roundtrip() {
    assert_roundtrip(&ExecutionMode::Passthrough);
    assert_roundtrip(&ExecutionMode::Mapped);

    assert_eq!(serde_json::to_value(ExecutionMode::Passthrough).unwrap(), json!("passthrough"));
    assert_eq!(serde_json::to_value(ExecutionMode::Mapped).unwrap(), json!("mapped"));
}

#[test]
fn execution_mode_pretty_vs_compact() {
    assert_pretty_compact_equal(&ExecutionMode::Passthrough);
    assert_pretty_compact_equal(&ExecutionMode::Mapped);
}

// ===========================================================================
// 9. ExecutionLane — each variant
// ===========================================================================

#[test]
fn execution_lane_all_variants_roundtrip() {
    assert_roundtrip(&ExecutionLane::PatchFirst);
    assert_roundtrip(&ExecutionLane::WorkspaceFirst);

    assert_eq!(serde_json::to_value(ExecutionLane::PatchFirst).unwrap(), json!("patch_first"));
    assert_eq!(
        serde_json::to_value(ExecutionLane::WorkspaceFirst).unwrap(),
        json!("workspace_first")
    );
}

#[test]
fn execution_lane_pretty_vs_compact() {
    assert_pretty_compact_equal(&ExecutionLane::PatchFirst);
    assert_pretty_compact_equal(&ExecutionLane::WorkspaceFirst);
}

// ===========================================================================
// 10. WorkspaceMode — each variant
// ===========================================================================

#[test]
fn workspace_mode_all_variants_roundtrip() {
    assert_roundtrip(&WorkspaceMode::PassThrough);
    assert_roundtrip(&WorkspaceMode::Staged);

    assert_eq!(serde_json::to_value(WorkspaceMode::PassThrough).unwrap(), json!("pass_through"));
    assert_eq!(serde_json::to_value(WorkspaceMode::Staged).unwrap(), json!("staged"));
}

#[test]
fn workspace_mode_pretty_vs_compact() {
    assert_pretty_compact_equal(&WorkspaceMode::PassThrough);
    assert_pretty_compact_equal(&WorkspaceMode::Staged);
}

// ===========================================================================
// 11. Outcome — each variant
// ===========================================================================

#[test]
fn outcome_all_variants_roundtrip() {
    assert_roundtrip(&Outcome::Complete);
    assert_roundtrip(&Outcome::Partial);
    assert_roundtrip(&Outcome::Failed);

    assert_eq!(serde_json::to_value(Outcome::Complete).unwrap(), json!("complete"));
    assert_eq!(serde_json::to_value(Outcome::Partial).unwrap(), json!("partial"));
    assert_eq!(serde_json::to_value(Outcome::Failed).unwrap(), json!("failed"));
}

#[test]
fn outcome_pretty_vs_compact() {
    assert_pretty_compact_equal(&Outcome::Complete);
    assert_pretty_compact_equal(&Outcome::Partial);
    assert_pretty_compact_equal(&Outcome::Failed);
}

// ===========================================================================
// 12. CapabilityRequirement — all support level combos
// ===========================================================================

#[test]
fn capability_requirement_native_roundtrip() {
    let req = CapabilityRequirement {
        capability: Capability::ToolRead,
        min_support: MinSupport::Native,
    };
    assert_roundtrip(&req);
}

#[test]
fn capability_requirement_emulated_roundtrip() {
    let req = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Emulated,
    };
    assert_roundtrip(&req);
}

#[test]
fn capability_requirement_all_combos_roundtrip() {
    let capabilities = [Capability::ToolRead, Capability::McpClient, Capability::Streaming];
    let supports = [MinSupport::Native, MinSupport::Emulated];

    for cap in &capabilities {
        for min in &supports {
            let req = CapabilityRequirement {
                capability: cap.clone(),
                min_support: min.clone(),
            };
            assert_roundtrip(&req);
        }
    }
}

#[test]
fn capability_requirement_extra_fields_tolerated() {
    let v = json!({
        "capability": "tool_read",
        "min_support": "native",
        "priority": "high"
    });
    let req: CapabilityRequirement = serde_json::from_value(v).unwrap();
    assert_eq!(req.capability, Capability::ToolRead);
}

#[test]
fn capability_requirements_full_roundtrip() {
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
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
        ],
    };
    assert_roundtrip(&reqs);
    assert_pretty_compact_equal(&reqs);
}

// ===========================================================================
// 13. Sub-struct roundtrips
// ===========================================================================

#[test]
fn runtime_config_full_roundtrip() {
    let cfg = RuntimeConfig {
        model: Some("claude-3".into()),
        vendor: {
            let mut m = BTreeMap::new();
            m.insert("anthropic".into(), json!({"max_tokens": 4096}));
            m
        },
        env: {
            let mut m = BTreeMap::new();
            m.insert("API_KEY".into(), "secret".into());
            m
        },
        max_budget_usd: Some(5.0),
        max_turns: Some(50),
    };
    assert_roundtrip(&cfg);
    assert_pretty_compact_equal(&cfg);
}

#[test]
fn backend_identity_full_roundtrip() {
    let id = BackendIdentity {
        id: "sidecar:claude".into(),
        backend_version: Some("3.5".into()),
        adapter_version: Some("0.2.0".into()),
    };
    assert_roundtrip(&id);
    assert_pretty_compact_equal(&id);
}

#[test]
fn backend_identity_minimal_missing_optionals() {
    let v = json!({"id": "mock"});
    let id: BackendIdentity = serde_json::from_value(v).unwrap();
    assert_eq!(id.id, "mock");
    assert!(id.backend_version.is_none());
    assert!(id.adapter_version.is_none());
    assert_roundtrip(&id);
}

#[test]
fn usage_normalized_full_roundtrip() {
    let u = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cache_read_tokens: Some(200),
        cache_write_tokens: Some(100),
        request_units: Some(3),
        estimated_cost_usd: Some(0.05),
    };
    assert_roundtrip(&u);
    assert_pretty_compact_equal(&u);
}

#[test]
fn usage_normalized_all_null() {
    let v = json!({
        "input_tokens": null,
        "output_tokens": null,
        "cache_read_tokens": null,
        "cache_write_tokens": null,
        "request_units": null,
        "estimated_cost_usd": null
    });
    let u: UsageNormalized = serde_json::from_value(v).unwrap();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn verification_report_full_roundtrip() {
    let vr = VerificationReport {
        git_diff: Some("+new line\n-old line".into()),
        git_status: Some("M src/lib.rs\nA tests/new.rs".into()),
        harness_ok: true,
    };
    assert_roundtrip(&vr);
    assert_pretty_compact_equal(&vr);
}

#[test]
fn context_packet_with_snippets_roundtrip() {
    let ctx = ContextPacket {
        files: vec!["a.rs".into(), "b.rs".into()],
        snippets: vec![
            ContextSnippet {
                name: "err".into(),
                content: "panic!".into(),
            },
            ContextSnippet {
                name: "log".into(),
                content: "WARN: timeout".into(),
            },
        ],
    };
    assert_roundtrip(&ctx);
    assert_pretty_compact_equal(&ctx);
}

#[test]
fn artifact_ref_roundtrip() {
    let a = ArtifactRef {
        kind: "log".into(),
        path: "logs/run.txt".into(),
    };
    assert_roundtrip(&a);
}

// ===========================================================================
// 14. CapabilityManifest (BTreeMap<Capability, SupportLevel>)
// ===========================================================================

#[test]
fn capability_manifest_roundtrip() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    m.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "no MCP server configured".into(),
        },
    );
    assert_roundtrip(&m);
    assert_pretty_compact_equal(&m);
}

#[test]
fn capability_manifest_empty_roundtrip() {
    let m = CapabilityManifest::new();
    assert_roundtrip(&m);
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v, json!({}));
}

// ===========================================================================
// 15. Complex nested JSON fixtures
// ===========================================================================

#[test]
fn full_receipt_json_fixture_deserializes() {
    let fixture = json!({
        "meta": {
            "run_id": "00000000-0000-0000-0000-000000000000",
            "work_order_id": "11111111-1111-1111-1111-111111111111",
            "contract_version": "abp/v0.1",
            "started_at": "2025-06-01T12:00:00Z",
            "finished_at": "2025-06-01T12:05:00Z",
            "duration_ms": 300000
        },
        "backend": {
            "id": "sidecar:claude",
            "backend_version": "3.5-sonnet",
            "adapter_version": "0.1.0"
        },
        "capabilities": {
            "streaming": "native",
            "tool_read": "native",
            "tool_write": "native",
            "tool_edit": "native",
            "tool_bash": "emulated",
            "mcp_client": {"restricted": {"reason": "no server"}}
        },
        "mode": "passthrough",
        "usage_raw": {
            "prompt_tokens": 5000,
            "completion_tokens": 2000,
            "total_tokens": 7000
        },
        "usage": {
            "input_tokens": 5000,
            "output_tokens": 2000,
            "cache_read_tokens": 1000,
            "cache_write_tokens": 500,
            "request_units": null,
            "estimated_cost_usd": 0.12
        },
        "trace": [
            {
                "ts": "2025-06-01T12:00:01Z",
                "type": "run_started",
                "message": "Beginning task"
            },
            {
                "ts": "2025-06-01T12:01:00Z",
                "type": "assistant_delta",
                "text": "Let me "
            },
            {
                "ts": "2025-06-01T12:01:01Z",
                "type": "assistant_message",
                "text": "Let me analyze the code."
            },
            {
                "ts": "2025-06-01T12:02:00Z",
                "type": "tool_call",
                "tool_name": "read_file",
                "tool_use_id": "tu_abc",
                "parent_tool_use_id": null,
                "input": {"path": "src/main.rs"}
            },
            {
                "ts": "2025-06-01T12:02:01Z",
                "type": "tool_result",
                "tool_name": "read_file",
                "tool_use_id": "tu_abc",
                "output": {"content": "fn main() {}"},
                "is_error": false
            },
            {
                "ts": "2025-06-01T12:03:00Z",
                "type": "file_changed",
                "path": "src/main.rs",
                "summary": "Added error handling"
            },
            {
                "ts": "2025-06-01T12:04:00Z",
                "type": "command_executed",
                "command": "cargo test",
                "exit_code": 0,
                "output_preview": "test result: ok. 42 passed"
            },
            {
                "ts": "2025-06-01T12:04:30Z",
                "type": "warning",
                "message": "One test was slow"
            },
            {
                "ts": "2025-06-01T12:04:59Z",
                "type": "run_completed",
                "message": "All changes applied"
            }
        ],
        "artifacts": [
            {"kind": "patch", "path": "changes.diff"},
            {"kind": "log", "path": "run.log"}
        ],
        "verification": {
            "git_diff": "diff --git a/src/main.rs b/src/main.rs\n+added",
            "git_status": "M src/main.rs",
            "harness_ok": true
        },
        "outcome": "complete",
        "receipt_sha256": null
    });

    let r: Receipt = serde_json::from_value(fixture).unwrap();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.trace.len(), 9);
    assert_eq!(r.artifacts.len(), 2);
    assert_eq!(r.outcome, Outcome::Complete);
    // Re-serialize and deserialize again to confirm stability
    assert_roundtrip(&r);
}

#[test]
fn full_work_order_json_fixture_deserializes() {
    let fixture = json!({
        "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "task": "Refactor authentication module",
        "lane": "patch_first",
        "workspace": {
            "root": "/home/user/project",
            "mode": "pass_through",
            "include": [],
            "exclude": ["node_modules/**", ".git/**"]
        },
        "context": {
            "files": [],
            "snippets": []
        },
        "policy": {
            "allowed_tools": [],
            "disallowed_tools": [],
            "deny_read": [],
            "deny_write": [],
            "allow_network": [],
            "deny_network": ["*"],
            "require_approval_for": []
        },
        "requirements": {
            "required": []
        },
        "config": {
            "model": null,
            "vendor": {},
            "env": {},
            "max_budget_usd": null,
            "max_turns": null
        }
    });

    let wo: WorkOrder = serde_json::from_value(fixture).unwrap();
    assert_eq!(wo.task, "Refactor authentication module");
    assert_eq!(
        serde_json::to_value(&wo.workspace.mode).unwrap(),
        json!("pass_through")
    );
    assert_roundtrip(&wo);
}

// ===========================================================================
// 16. Null handling edge cases
// ===========================================================================

#[test]
fn receipt_sha256_null_vs_present() {
    let mut r = sample_receipt_minimal();
    assert!(r.receipt_sha256.is_none());

    let v1 = serde_json::to_value(&r).unwrap();
    assert!(v1["receipt_sha256"].is_null());

    r = r.with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let v2 = serde_json::to_value(&r).unwrap();
    assert!(v2["receipt_sha256"].is_string());
    assert_eq!(v2["receipt_sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn agent_event_ext_null_vs_absent_vs_some() {
    let ts = "2025-01-01T00:00:00Z";

    // ext absent
    let v_absent = json!({"ts": ts, "type": "warning", "message": "a"});
    let e1: AgentEvent = serde_json::from_value(v_absent).unwrap();
    assert!(e1.ext.is_none());

    // ext null
    let v_null = json!({"ts": ts, "type": "warning", "message": "a", "ext": null});
    let e2: AgentEvent = serde_json::from_value(v_null).unwrap();
    assert!(e2.ext.is_none());

    // ext present
    let v_some = json!({"ts": ts, "type": "warning", "message": "a", "ext": {"k": "v"}});
    let e3: AgentEvent = serde_json::from_value(v_some).unwrap();
    assert!(e3.ext.is_some());
    assert!(e3.ext.unwrap().contains_key("k"));
}

// ===========================================================================
// 17. WorkspaceSpec roundtrip
// ===========================================================================

#[test]
fn workspace_spec_roundtrip() {
    let ws = WorkspaceSpec {
        root: "/project".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["**/*.rs".into()],
        exclude: vec!["target/**".into()],
    };
    assert_roundtrip(&ws);
    assert_pretty_compact_equal(&ws);
}

// ===========================================================================
// 18. RunMetadata roundtrip
// ===========================================================================

#[test]
fn run_metadata_roundtrip() {
    let now = Utc::now();
    let meta = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::new_v4(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: now,
        finished_at: now,
        duration_ms: 12345,
    };
    assert_roundtrip(&meta);
    assert_pretty_compact_equal(&meta);
}

// ===========================================================================
// 19. ContextSnippet roundtrip
// ===========================================================================

#[test]
fn context_snippet_roundtrip() {
    let s = ContextSnippet {
        name: "error_log".into(),
        content: "thread 'main' panicked at 'index out of bounds'".into(),
    };
    assert_roundtrip(&s);
}

// ===========================================================================
// 20. Deterministic canonical JSON
// ===========================================================================

#[test]
fn canonical_json_deterministic_for_receipt() {
    let r = sample_receipt_full();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2, "canonical_json must be deterministic");
}

#[test]
fn canonical_json_deterministic_for_work_order() {
    let wo = sample_work_order_full();
    let j1 = canonical_json(&wo).unwrap();
    let j2 = canonical_json(&wo).unwrap();
    assert_eq!(j1, j2, "canonical_json must be deterministic");
}
