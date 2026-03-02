// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for contract types.
//!
//! Covers serde roundtrips, deterministic hashing, capability set operations,
//! BTreeMap ordering invariants, receipt hash sensitivity, policy engine
//! construction, and IR roundtrip stability.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use serde_json::json;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_policy::PolicyEngine;
use abp_receipt::{compute_hash, verify_hash};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

// -- Primitives -----------------------------------------------------------

fn arb_safe_string() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_ ./-]{1,32}".boxed()
}

fn arb_short_string() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9_]{0,15}".boxed()
}

fn arb_uuid() -> BoxedStrategy<Uuid> {
    (any::<u128>()).prop_map(Uuid::from_u128).boxed()
}

fn arb_datetime() -> BoxedStrategy<DateTime<Utc>> {
    // Use a fixed range of timestamps to avoid overflow issues.
    (1_000_000_000i64..2_000_000_000i64)
        .prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
        .boxed()
}

fn arb_json_value() -> BoxedStrategy<serde_json::Value> {
    prop_oneof![
        Just(json!(null)),
        any::<bool>().prop_map(|b| json!(b)),
        any::<i64>().prop_map(|n| json!(n)),
        arb_safe_string().prop_map(|s| json!(s)),
        Just(json!({})),
        Just(json!([])),
    ]
    .boxed()
}

// -- Enums ----------------------------------------------------------------

fn arb_execution_lane() -> BoxedStrategy<ExecutionLane> {
    prop_oneof![
        Just(ExecutionLane::PatchFirst),
        Just(ExecutionLane::WorkspaceFirst),
    ]
    .boxed()
}

fn arb_workspace_mode() -> BoxedStrategy<WorkspaceMode> {
    prop_oneof![
        Just(WorkspaceMode::PassThrough),
        Just(WorkspaceMode::Staged),
    ]
    .boxed()
}

fn arb_execution_mode() -> BoxedStrategy<ExecutionMode> {
    prop_oneof![
        Just(ExecutionMode::Passthrough),
        Just(ExecutionMode::Mapped),
    ]
    .boxed()
}

fn arb_outcome() -> BoxedStrategy<Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
    .boxed()
}

fn arb_min_support() -> BoxedStrategy<MinSupport> {
    prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated),].boxed()
}

fn arb_capability() -> BoxedStrategy<Capability> {
    prop_oneof![
        Just(Capability::Streaming),
        Just(Capability::ToolRead),
        Just(Capability::ToolWrite),
        Just(Capability::ToolEdit),
        Just(Capability::ToolBash),
        Just(Capability::ToolGlob),
        Just(Capability::ToolGrep),
        Just(Capability::ToolWebSearch),
        Just(Capability::ToolWebFetch),
        Just(Capability::ToolAskUser),
        Just(Capability::HooksPreToolUse),
        Just(Capability::HooksPostToolUse),
        Just(Capability::SessionResume),
        Just(Capability::SessionFork),
        Just(Capability::Checkpointing),
        Just(Capability::StructuredOutputJsonSchema),
        Just(Capability::McpClient),
        Just(Capability::McpServer),
        Just(Capability::ToolUse),
        Just(Capability::ExtendedThinking),
        Just(Capability::ImageInput),
        Just(Capability::PdfInput),
        Just(Capability::CodeExecution),
        Just(Capability::Logprobs),
        Just(Capability::SeedDeterminism),
        Just(Capability::StopSequences),
    ]
    .boxed()
}

fn arb_support_level() -> BoxedStrategy<SupportLevel> {
    prop_oneof![
        Just(SupportLevel::Native),
        Just(SupportLevel::Emulated),
        Just(SupportLevel::Unsupported),
        arb_safe_string().prop_map(|r| SupportLevel::Restricted { reason: r }),
    ]
    .boxed()
}

// -- Compound contract types -----------------------------------------------

fn arb_workspace_spec() -> BoxedStrategy<WorkspaceSpec> {
    (
        arb_safe_string(),
        arb_workspace_mode(),
        prop::collection::vec(arb_safe_string(), 0..3),
        prop::collection::vec(arb_safe_string(), 0..3),
    )
        .prop_map(|(root, mode, include, exclude)| WorkspaceSpec {
            root,
            mode,
            include,
            exclude,
        })
        .boxed()
}

fn arb_context_snippet() -> BoxedStrategy<ContextSnippet> {
    (arb_safe_string(), arb_safe_string())
        .prop_map(|(name, content)| ContextSnippet { name, content })
        .boxed()
}

fn arb_context_packet() -> BoxedStrategy<ContextPacket> {
    (
        prop::collection::vec(arb_safe_string(), 0..3),
        prop::collection::vec(arb_context_snippet(), 0..3),
    )
        .prop_map(|(files, snippets)| ContextPacket { files, snippets })
        .boxed()
}

fn arb_policy_profile() -> BoxedStrategy<PolicyProfile> {
    (
        prop::collection::vec(arb_safe_string(), 0..3),
        prop::collection::vec(arb_safe_string(), 0..3),
        prop::collection::vec(arb_safe_string(), 0..3),
        prop::collection::vec(arb_safe_string(), 0..3),
    )
        .prop_map(
            |(allowed_tools, disallowed_tools, deny_read, deny_write)| PolicyProfile {
                allowed_tools,
                disallowed_tools,
                deny_read,
                deny_write,
                allow_network: vec![],
                deny_network: vec![],
                require_approval_for: vec![],
            },
        )
        .boxed()
}

fn arb_capability_requirement() -> BoxedStrategy<CapabilityRequirement> {
    (arb_capability(), arb_min_support())
        .prop_map(|(capability, min_support)| CapabilityRequirement {
            capability,
            min_support,
        })
        .boxed()
}

fn arb_capability_requirements() -> BoxedStrategy<CapabilityRequirements> {
    prop::collection::vec(arb_capability_requirement(), 0..4)
        .prop_map(|required| CapabilityRequirements { required })
        .boxed()
}

fn arb_runtime_config() -> BoxedStrategy<RuntimeConfig> {
    (
        prop::option::of(arb_safe_string()),
        prop::option::of(1u32..100u32),
    )
        .prop_map(|(model, max_turns)| RuntimeConfig {
            model,
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns,
        })
        .boxed()
}

fn arb_capability_manifest() -> BoxedStrategy<CapabilityManifest> {
    prop::collection::vec((arb_capability(), arb_support_level()), 0..6)
        .prop_map(|pairs| pairs.into_iter().collect::<BTreeMap<_, _>>())
        .boxed()
}

fn arb_work_order() -> BoxedStrategy<WorkOrder> {
    (
        arb_uuid(),
        arb_safe_string(),
        arb_execution_lane(),
        arb_workspace_spec(),
        arb_context_packet(),
        arb_policy_profile(),
        arb_capability_requirements(),
        arb_runtime_config(),
    )
        .prop_map(
            |(id, task, lane, workspace, context, policy, requirements, config)| WorkOrder {
                id,
                task,
                lane,
                workspace,
                context,
                policy,
                requirements,
                config,
            },
        )
        .boxed()
}

fn arb_backend_identity() -> BoxedStrategy<BackendIdentity> {
    (
        arb_short_string(),
        prop::option::of(arb_short_string()),
        prop::option::of(arb_short_string()),
    )
        .prop_map(|(id, backend_version, adapter_version)| BackendIdentity {
            id,
            backend_version,
            adapter_version,
        })
        .boxed()
}

fn arb_run_metadata() -> BoxedStrategy<RunMetadata> {
    (arb_uuid(), arb_uuid(), arb_datetime(), arb_datetime())
        .prop_map(|(run_id, work_order_id, started_at, finished_at)| {
            let (s, f) = if started_at <= finished_at {
                (started_at, finished_at)
            } else {
                (finished_at, started_at)
            };
            let duration_ms = (f - s).num_milliseconds().max(0) as u64;
            RunMetadata {
                run_id,
                work_order_id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: s,
                finished_at: f,
                duration_ms,
            }
        })
        .boxed()
}

fn arb_usage_normalized() -> BoxedStrategy<UsageNormalized> {
    (
        prop::option::of(0u64..100_000u64),
        prop::option::of(0u64..100_000u64),
    )
        .prop_map(|(input_tokens, output_tokens)| UsageNormalized {
            input_tokens,
            output_tokens,
            ..UsageNormalized::default()
        })
        .boxed()
}

fn arb_artifact_ref() -> BoxedStrategy<ArtifactRef> {
    (arb_short_string(), arb_safe_string())
        .prop_map(|(kind, path)| ArtifactRef { kind, path })
        .boxed()
}

fn arb_verification_report() -> BoxedStrategy<VerificationReport> {
    (
        prop::option::of(arb_safe_string()),
        prop::option::of(arb_safe_string()),
        any::<bool>(),
    )
        .prop_map(|(git_diff, git_status, harness_ok)| VerificationReport {
            git_diff,
            git_status,
            harness_ok,
        })
        .boxed()
}

fn arb_agent_event_kind() -> BoxedStrategy<AgentEventKind> {
    prop_oneof![
        arb_safe_string().prop_map(|message| AgentEventKind::RunStarted { message }),
        arb_safe_string().prop_map(|message| AgentEventKind::RunCompleted { message }),
        arb_safe_string().prop_map(|text| AgentEventKind::AssistantDelta { text }),
        arb_safe_string().prop_map(|text| AgentEventKind::AssistantMessage { text }),
        (arb_short_string(), arb_json_value()).prop_map(|(tool_name, input)| {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id: None,
                parent_tool_use_id: None,
                input,
            }
        }),
        (arb_short_string(), arb_json_value(), any::<bool>()).prop_map(
            |(tool_name, output, is_error)| {
                AgentEventKind::ToolResult {
                    tool_name,
                    tool_use_id: None,
                    output,
                    is_error,
                }
            }
        ),
        (arb_safe_string(), arb_safe_string())
            .prop_map(|(path, summary)| AgentEventKind::FileChanged { path, summary }),
        (arb_safe_string(), prop::option::of(-1i32..256i32)).prop_map(|(command, exit_code)| {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview: None,
            }
        }),
        arb_safe_string().prop_map(|message| AgentEventKind::Warning { message }),
        arb_safe_string().prop_map(|message| AgentEventKind::Error {
            message,
            error_code: None
        }),
    ]
    .boxed()
}

fn arb_agent_event() -> BoxedStrategy<AgentEvent> {
    (arb_datetime(), arb_agent_event_kind())
        .prop_map(|(ts, kind)| AgentEvent {
            ts,
            kind,
            ext: None,
        })
        .boxed()
}

fn arb_receipt() -> BoxedStrategy<Receipt> {
    (
        arb_run_metadata(),
        arb_backend_identity(),
        arb_capability_manifest(),
        arb_execution_mode(),
        arb_usage_normalized(),
        prop::collection::vec(arb_agent_event(), 0..4),
        prop::collection::vec(arb_artifact_ref(), 0..3),
        arb_verification_report(),
        arb_outcome(),
    )
        .prop_map(
            |(
                meta,
                backend,
                capabilities,
                mode,
                usage,
                trace,
                artifacts,
                verification,
                outcome,
            )| {
                Receipt {
                    meta,
                    backend,
                    capabilities,
                    mode,
                    usage_raw: json!({}),
                    usage,
                    trace,
                    artifacts,
                    verification,
                    outcome,
                    receipt_sha256: None,
                }
            },
        )
        .boxed()
}

// -- IR strategies --------------------------------------------------------

fn arb_ir_role() -> BoxedStrategy<IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
    .boxed()
}

fn arb_ir_text_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_string()
        .prop_map(|text| IrContentBlock::Text { text })
        .boxed()
}

fn arb_ir_image_block() -> BoxedStrategy<IrContentBlock> {
    (arb_safe_string(), arb_safe_string())
        .prop_map(|(media_type, data)| IrContentBlock::Image { media_type, data })
        .boxed()
}

fn arb_ir_thinking_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_string()
        .prop_map(|text| IrContentBlock::Thinking { text })
        .boxed()
}

fn arb_ir_tool_use_block() -> BoxedStrategy<IrContentBlock> {
    (arb_short_string(), arb_short_string(), arb_json_value())
        .prop_map(|(id, name, input)| IrContentBlock::ToolUse { id, name, input })
        .boxed()
}

fn arb_ir_content_block() -> BoxedStrategy<IrContentBlock> {
    prop_oneof![
        arb_ir_text_block(),
        arb_ir_image_block(),
        arb_ir_thinking_block(),
        arb_ir_tool_use_block(),
    ]
    .boxed()
}

fn arb_ir_message() -> BoxedStrategy<IrMessage> {
    (
        arb_ir_role(),
        prop::collection::vec(arb_ir_content_block(), 1..4),
    )
        .prop_map(|(role, content)| IrMessage::new(role, content))
        .boxed()
}

fn arb_ir_conversation() -> BoxedStrategy<IrConversation> {
    prop::collection::vec(arb_ir_message(), 1..6)
        .prop_map(IrConversation::from_messages)
        .boxed()
}

fn arb_ir_tool_definition() -> BoxedStrategy<IrToolDefinition> {
    (arb_short_string(), arb_safe_string(), arb_json_value())
        .prop_map(|(name, description, parameters)| IrToolDefinition {
            name,
            description,
            parameters,
        })
        .boxed()
}

fn arb_ir_usage() -> BoxedStrategy<IrUsage> {
    (0u64..100_000, 0u64..100_000, 0u64..10_000, 0u64..10_000)
        .prop_map(|(inp, out, cr, cw)| IrUsage::with_cache(inp, out, cr, cw))
        .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  WorkOrder serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 1
    #[test]
    fn work_order_serde_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    // 2
    #[test]
    fn work_order_task_preserved(task in arb_safe_string()) {
        let wo = WorkOrder {
            id: Uuid::nil(),
            task: task.clone(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec { root: ".".into(), mode: WorkspaceMode::Staged, include: vec![], exclude: vec![] },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(task, rt.task);
    }

    // 3
    #[test]
    fn work_order_id_roundtrip(id in arb_uuid()) {
        let wo = WorkOrder {
            id,
            task: "t".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec { root: ".".into(), mode: WorkspaceMode::Staged, include: vec![], exclude: vec![] },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(id, rt.id);
    }

    // 4
    #[test]
    fn work_order_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    // 5
    #[test]
    fn work_order_lane_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        let lane_json_orig = serde_json::to_string(&wo.lane).unwrap();
        let lane_json_rt = serde_json::to_string(&rt.lane).unwrap();
        prop_assert_eq!(lane_json_orig, lane_json_rt);
    }

    // 6
    #[test]
    fn work_order_workspace_spec_roundtrip(ws in arb_workspace_spec()) {
        let json = serde_json::to_string(&ws).unwrap();
        let rt: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ws.root, &rt.root);
        prop_assert_eq!(&ws.include, &rt.include);
        prop_assert_eq!(&ws.exclude, &rt.exclude);
    }

    // 7
    #[test]
    fn work_order_context_roundtrip(ctx in arb_context_packet()) {
        let json = serde_json::to_string(&ctx).unwrap();
        let rt: ContextPacket = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ctx.files.len(), rt.files.len());
        prop_assert_eq!(ctx.snippets.len(), rt.snippets.len());
    }

    // 8
    #[test]
    fn work_order_config_roundtrip(cfg in arb_runtime_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cfg.model, &rt.model);
        prop_assert_eq!(cfg.max_turns, rt.max_turns);
    }

    // 9
    #[test]
    fn work_order_arbitrary_task_roundtrip(task in "\\PC{1,128}") {
        let wo = WorkOrder {
            id: Uuid::nil(),
            task: task.clone(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec { root: ".".into(), mode: WorkspaceMode::Staged, include: vec![], exclude: vec![] },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(task, rt.task);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Receipt hash determinism
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 10
    #[test]
    fn receipt_hash_deterministic(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // 11
    #[test]
    fn receipt_hash_length_is_64(r in arb_receipt()) {
        let h = compute_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
    }

    // 12
    #[test]
    fn receipt_hash_is_hex(r in arb_receipt()) {
        let h = compute_hash(&r).unwrap();
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // 13
    #[test]
    fn receipt_hash_ignores_stored_hash(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.receipt_sha256 = Some("previous_hash_value".into());
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // 14
    #[test]
    fn receipt_with_hash_verifies(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(verify_hash(&hashed));
    }

    // 15
    #[test]
    fn receipt_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&r.meta.run_id, &rt.meta.run_id);
        prop_assert_eq!(&r.outcome, &rt.outcome);
        prop_assert_eq!(&r.backend.id, &rt.backend.id);
    }

    // 16
    #[test]
    fn receipt_value_roundtrip(r in arb_receipt()) {
        let v = serde_json::to_value(&r).unwrap();
        let rt: Receipt = serde_json::from_value(v).unwrap();
        prop_assert_eq!(&r.meta.run_id, &rt.meta.run_id);
    }

    // 17
    #[test]
    fn receipt_hash_after_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        let h_orig = compute_hash(&r).unwrap();
        let h_rt = compute_hash(&rt).unwrap();
        prop_assert_eq!(h_orig, h_rt);
    }

    // 18
    #[test]
    fn receipt_outcome_roundtrip(outcome in arb_outcome()) {
        let json = serde_json::to_string(&outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(outcome, rt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  AgentEvent serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 19
    #[test]
    fn agent_event_serde_roundtrip(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    // 20
    #[test]
    fn agent_event_value_roundtrip(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_value(v).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    // 21
    #[test]
    fn agent_event_kind_tag_present(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        prop_assert!(v.get("type").is_some(), "AgentEvent must have 'type' tag");
    }

    // 22
    #[test]
    fn agent_event_run_started_roundtrip(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::RunStarted { message } = &rt.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 23
    #[test]
    fn agent_event_assistant_delta_roundtrip(text in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: text.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantDelta { text: t } = &rt.kind {
            prop_assert_eq!(&text, t);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 24
    #[test]
    fn agent_event_tool_call_roundtrip(name in arb_short_string(), val in arb_json_value()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: name.clone(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: val.clone(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::ToolCall { tool_name, input, .. } = &rt.kind {
            prop_assert_eq!(&name, tool_name);
            prop_assert_eq!(&val, input);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 25
    #[test]
    fn agent_event_tool_result_roundtrip(
        name in arb_short_string(),
        val in arb_json_value(),
        is_err in any::<bool>()
    ) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: name.clone(),
                tool_use_id: None,
                output: val.clone(),
                is_error: is_err,
            },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::ToolResult { tool_name, output, is_error, .. } = &rt.kind {
            prop_assert_eq!(&name, tool_name);
            prop_assert_eq!(&val, output);
            prop_assert_eq!(is_err, *is_error);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 26
    #[test]
    fn agent_event_file_changed_roundtrip(
        path in arb_safe_string(),
        summary in arb_safe_string()
    ) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged { path: path.clone(), summary: summary.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::FileChanged { path: p, summary: s } = &rt.kind {
            prop_assert_eq!(&path, p);
            prop_assert_eq!(&summary, s);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 27
    #[test]
    fn agent_event_command_executed_roundtrip(
        cmd in arb_safe_string(),
        exit in prop::option::of(-1i32..256i32)
    ) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: cmd.clone(),
                exit_code: exit,
                output_preview: None,
            },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::CommandExecuted { command, exit_code, .. } = &rt.kind {
            prop_assert_eq!(&cmd, command);
            prop_assert_eq!(exit, *exit_code);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 28
    #[test]
    fn agent_event_warning_roundtrip(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::Warning { message } = &rt.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 29
    #[test]
    fn agent_event_error_roundtrip(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error { message: msg.clone(), error_code: None },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::Error { message, .. } = &rt.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Capability set operations
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 30
    #[test]
    fn capability_serde_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cap, rt);
    }

    // 31
    #[test]
    fn capability_set_union_contains_both(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let union: BTreeSet<Capability> = sa.union(&sb).cloned().collect();
        for cap in &sa {
            prop_assert!(union.contains(cap));
        }
        for cap in &sb {
            prop_assert!(union.contains(cap));
        }
    }

    // 32
    #[test]
    fn capability_set_intersection_subset(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let inter: BTreeSet<Capability> = sa.intersection(&sb).cloned().collect();
        for cap in &inter {
            prop_assert!(sa.contains(cap));
            prop_assert!(sb.contains(cap));
        }
    }

    // 33
    #[test]
    fn capability_set_difference_excludes_other(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let diff: BTreeSet<Capability> = sa.difference(&sb).cloned().collect();
        for cap in &diff {
            prop_assert!(sa.contains(cap));
            prop_assert!(!sb.contains(cap));
        }
    }

    // 34
    #[test]
    fn capability_set_symmetric_difference(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let sym: BTreeSet<Capability> = sa.symmetric_difference(&sb).cloned().collect();
        for cap in &sym {
            prop_assert!(sa.contains(cap) ^ sb.contains(cap));
        }
    }

    // 35
    #[test]
    fn capability_set_union_cardinality(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let union: BTreeSet<Capability> = sa.union(&sb).cloned().collect();
        let inter: BTreeSet<Capability> = sa.intersection(&sb).cloned().collect();
        prop_assert_eq!(union.len(), sa.len() + sb.len() - inter.len());
    }

    // 36
    #[test]
    fn capability_set_union_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let u1: BTreeSet<Capability> = sa.union(&sb).cloned().collect();
        let u2: BTreeSet<Capability> = sb.union(&sa).cloned().collect();
        prop_assert_eq!(u1, u2);
    }

    // 37
    #[test]
    fn capability_set_intersection_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let i1: BTreeSet<Capability> = sa.intersection(&sb).cloned().collect();
        let i2: BTreeSet<Capability> = sb.intersection(&sa).cloned().collect();
        prop_assert_eq!(i1, i2);
    }

    // 38
    #[test]
    fn capability_set_is_subset_of_union(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let union: BTreeSet<Capability> = sa.union(&sb).cloned().collect();
        prop_assert!(sa.is_subset(&union));
        prop_assert!(sb.is_subset(&union));
    }

    // 39
    #[test]
    fn capability_manifest_serde_roundtrip(manifest in arb_capability_manifest()) {
        let json = serde_json::to_string(&manifest).unwrap();
        let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), rt.len());
        for k in rt.keys() {
            prop_assert!(manifest.contains_key(k));
        }
    }

    // 40
    #[test]
    fn support_level_serde_roundtrip(level in arb_support_level()) {
        let json = serde_json::to_string(&level).unwrap();
        let _rt: SupportLevel = serde_json::from_str(&json).unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  BTreeMap deterministic serialization
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 41
    #[test]
    fn btreemap_string_deterministic(
        pairs in prop::collection::vec((arb_safe_string(), arb_safe_string()), 0..10),
    ) {
        let m1: BTreeMap<String, String> = pairs.iter().cloned().collect();
        let mut m2 = BTreeMap::new();
        for (k, v) in pairs.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 42
    #[test]
    fn btreemap_capability_manifest_insertion_order(
        pairs in prop::collection::vec((arb_capability(), arb_support_level()), 0..8),
    ) {
        // Collect once to deduplicate, then insert in reverse to verify order independence.
        let m1: BTreeMap<Capability, SupportLevel> = pairs.iter().cloned().collect();
        let entries: Vec<_> = m1.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut m2: BTreeMap<Capability, SupportLevel> = BTreeMap::new();
        for (k, v) in entries.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 43
    #[test]
    fn btreemap_env_deterministic(
        pairs in prop::collection::vec((arb_short_string(), arb_safe_string()), 0..10),
    ) {
        let m1: BTreeMap<String, String> = pairs.iter().cloned().collect();
        let mut m2 = BTreeMap::new();
        for (k, v) in pairs.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        prop_assert_eq!(
            serde_json::to_string(&m1).unwrap(),
            serde_json::to_string(&m2).unwrap()
        );
    }

    // 44
    #[test]
    fn btreemap_json_value_deterministic(
        pairs in prop::collection::vec((arb_short_string(), arb_json_value()), 0..8),
    ) {
        let m1: BTreeMap<String, serde_json::Value> = pairs.iter().cloned().collect();
        let mut m2: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for (k, v) in pairs.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        prop_assert_eq!(
            serde_json::to_string(&m1).unwrap(),
            serde_json::to_string(&m2).unwrap()
        );
    }

    // 45
    #[test]
    fn btreemap_roundtrip_preserves_keys(
        pairs in prop::collection::vec((arb_short_string(), arb_safe_string()), 1..10),
    ) {
        let map: BTreeMap<String, String> = pairs.into_iter().collect();
        let json = serde_json::to_string(&map).unwrap();
        let rt: BTreeMap<String, String> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(map, rt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Receipt hash sensitivity
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 46
    #[test]
    fn receipt_hash_changes_when_outcome_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 47
    #[test]
    fn receipt_hash_changes_when_backend_id_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_modified", r2.backend.id);
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 48
    #[test]
    fn receipt_hash_changes_when_run_id_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.run_id = Uuid::from_u128(r.meta.run_id.as_u128().wrapping_add(1));
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 49
    #[test]
    fn receipt_hash_changes_when_duration_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.duration_ms = r.meta.duration_ms.wrapping_add(1);
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 50
    #[test]
    fn receipt_hash_changes_when_mode_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.mode = match r.mode {
            ExecutionMode::Passthrough => ExecutionMode::Mapped,
            ExecutionMode::Mapped => ExecutionMode::Passthrough,
        };
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 51
    #[test]
    fn receipt_hash_changes_when_contract_version_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.contract_version = format!("{}_x", r2.meta.contract_version);
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 52
    #[test]
    fn receipt_hash_changes_when_trace_appended(r in arb_receipt(), evt in arb_agent_event()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.trace.push(evt);
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 53
    #[test]
    fn receipt_hash_changes_when_harness_ok_flipped(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.verification.harness_ok = !r.verification.harness_ok;
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 54
    #[test]
    fn receipt_hash_changes_when_artifact_added(r in arb_receipt(), art in arb_artifact_ref()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.artifacts.push(art);
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }

    // 55
    #[test]
    fn receipt_hash_changes_when_work_order_id_changes(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.work_order_id = Uuid::from_u128(r.meta.work_order_id.as_u128().wrapping_add(1));
        let h_changed = compute_hash(&r2).unwrap();
        prop_assert_ne!(h_orig, h_changed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  PolicyProfile → engine construction
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 56
    #[test]
    fn policy_default_constructs(policy in Just(PolicyProfile::default())) {
        let engine = PolicyEngine::new(&policy);
        prop_assert!(engine.is_ok());
    }

    // 57
    #[test]
    fn policy_with_glob_tools_constructs(
        allowed in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..4),
        denied in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..4),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy);
        prop_assert!(engine.is_ok());
    }

    // 58
    #[test]
    fn policy_with_deny_paths_constructs(
        deny_read in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
        deny_write in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
    ) {
        let policy = PolicyProfile {
            deny_read,
            deny_write,
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy);
        prop_assert!(engine.is_ok());
    }

    // 59
    #[test]
    fn policy_serde_roundtrip(policy in arb_policy_profile()) {
        let json = serde_json::to_string(&policy).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&policy.allowed_tools, &rt.allowed_tools);
        prop_assert_eq!(&policy.disallowed_tools, &rt.disallowed_tools);
        prop_assert_eq!(&policy.deny_read, &rt.deny_read);
        prop_assert_eq!(&policy.deny_write, &rt.deny_write);
    }

    // 60
    #[test]
    fn policy_wildcard_allowlist_constructs(
        denied in prop::collection::vec("[a-zA-Z]{1,8}", 0..4),
    ) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: denied,
            ..PolicyProfile::default()
        };
        prop_assert!(PolicyEngine::new(&policy).is_ok());
    }

    // 61
    #[test]
    fn policy_engine_default_allows_any_tool(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    // 62
    #[test]
    fn policy_deny_read_blocks_matching_path(
        filename in "[a-z]{1,8}"
    ) {
        let pattern = format!("**/{}*", &filename);
        let policy = PolicyProfile {
            deny_read: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_read_path(std::path::Path::new(&filename));
        prop_assert!(!decision.allowed);
    }

    // 63
    #[test]
    fn policy_deny_write_blocks_matching_path(
        filename in "[a-z]{1,8}"
    ) {
        let pattern = format!("**/{}*", &filename);
        let policy = PolicyProfile {
            deny_write: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_write_path(std::path::Path::new(&filename));
        prop_assert!(!decision.allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  IR types roundtrip stability
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 64
    #[test]
    fn ir_role_serde_roundtrip(role in arb_ir_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let rt: IrRole = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(role, rt);
    }

    // 65
    #[test]
    fn ir_text_block_roundtrip(text in arb_safe_string()) {
        let block = IrContentBlock::Text { text: text.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 66
    #[test]
    fn ir_image_block_roundtrip(media in arb_safe_string(), data in arb_safe_string()) {
        let block = IrContentBlock::Image { media_type: media.clone(), data: data.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 67
    #[test]
    fn ir_thinking_block_roundtrip(text in arb_safe_string()) {
        let block = IrContentBlock::Thinking { text: text.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 68
    #[test]
    fn ir_tool_use_block_roundtrip(
        id in arb_short_string(),
        name in arb_short_string(),
        input in arb_json_value()
    ) {
        let block = IrContentBlock::ToolUse { id: id.clone(), name: name.clone(), input: input.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 69
    #[test]
    fn ir_tool_result_block_roundtrip(
        tool_use_id in arb_short_string(),
        is_error in any::<bool>(),
        inner_text in arb_safe_string()
    ) {
        let block = IrContentBlock::ToolResult {
            tool_use_id,
            content: vec![IrContentBlock::Text { text: inner_text }],
            is_error,
        };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 70
    #[test]
    fn ir_message_roundtrip(msg in arb_ir_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let rt: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(msg, rt);
    }

    // 71
    #[test]
    fn ir_conversation_roundtrip(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv, rt);
    }

    // 72
    #[test]
    fn ir_conversation_message_count_preserved(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv.len(), rt.len());
    }

    // 73
    #[test]
    fn ir_tool_definition_roundtrip(td in arb_ir_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let rt: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(td, rt);
    }

    // 74
    #[test]
    fn ir_usage_roundtrip(u in arb_ir_usage()) {
        let json = serde_json::to_string(&u).unwrap();
        let rt: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u, rt);
    }

    // 75
    #[test]
    fn ir_usage_merge_commutative(a in arb_ir_usage(), b in arb_ir_usage()) {
        let ab = a.merge(b);
        let ba = b.merge(a);
        prop_assert_eq!(ab, ba);
    }

    // 76
    #[test]
    fn ir_usage_from_io_total(inp in 0u64..100_000, out in 0u64..100_000) {
        let u = IrUsage::from_io(inp, out);
        prop_assert_eq!(u.total_tokens, inp + out);
        prop_assert_eq!(u.cache_read_tokens, 0);
        prop_assert_eq!(u.cache_write_tokens, 0);
    }

    // 77
    #[test]
    fn ir_content_block_serde_roundtrip(block in arb_ir_content_block()) {
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 78
    #[test]
    fn ir_message_text_only_check(text in arb_safe_string()) {
        let msg = IrMessage::text(IrRole::User, &text);
        prop_assert!(msg.is_text_only());
        prop_assert_eq!(msg.text_content(), text);
    }

    // 79
    #[test]
    fn ir_conversation_value_roundtrip(conv in arb_ir_conversation()) {
        let v = serde_json::to_value(&conv).unwrap();
        let rt: IrConversation = serde_json::from_value(v).unwrap();
        prop_assert_eq!(conv, rt);
    }

    // 80
    #[test]
    fn ir_message_new_preserves_role(role in arb_ir_role(), text in arb_safe_string()) {
        let msg = IrMessage::text(role, &text);
        prop_assert_eq!(msg.role, role);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  Additional cross-cutting invariants
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 81
    #[test]
    fn execution_mode_serde_roundtrip(mode in arb_execution_mode()) {
        let json = serde_json::to_string(&mode).unwrap();
        let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(mode, rt);
    }

    // 82
    #[test]
    fn execution_lane_serde_roundtrip(lane in arb_execution_lane()) {
        let json = serde_json::to_string(&lane).unwrap();
        let _rt: ExecutionLane = serde_json::from_str(&json).unwrap();
    }

    // 83
    #[test]
    fn workspace_mode_serde_roundtrip(mode in arb_workspace_mode()) {
        let json = serde_json::to_string(&mode).unwrap();
        let _rt: WorkspaceMode = serde_json::from_str(&json).unwrap();
    }

    // 84
    #[test]
    fn min_support_serde_roundtrip(ms in arb_min_support()) {
        let json = serde_json::to_string(&ms).unwrap();
        let _rt: MinSupport = serde_json::from_str(&json).unwrap();
    }

    // 85
    #[test]
    fn backend_identity_serde_roundtrip(bi in arb_backend_identity()) {
        let json = serde_json::to_string(&bi).unwrap();
        let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&bi.id, &rt.id);
        prop_assert_eq!(&bi.backend_version, &rt.backend_version);
        prop_assert_eq!(&bi.adapter_version, &rt.adapter_version);
    }

    // 86
    #[test]
    fn run_metadata_serde_roundtrip(rm in arb_run_metadata()) {
        let json = serde_json::to_string(&rm).unwrap();
        let rt: RunMetadata = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(rm.run_id, rt.run_id);
        prop_assert_eq!(rm.work_order_id, rt.work_order_id);
        prop_assert_eq!(rm.duration_ms, rt.duration_ms);
    }

    // 87
    #[test]
    fn usage_normalized_serde_roundtrip(u in arb_usage_normalized()) {
        let json = serde_json::to_string(&u).unwrap();
        let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u.input_tokens, rt.input_tokens);
        prop_assert_eq!(u.output_tokens, rt.output_tokens);
    }

    // 88
    #[test]
    fn artifact_ref_serde_roundtrip(ar in arb_artifact_ref()) {
        let json = serde_json::to_string(&ar).unwrap();
        let rt: ArtifactRef = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ar.kind, &rt.kind);
        prop_assert_eq!(&ar.path, &rt.path);
    }

    // 89
    #[test]
    fn verification_report_serde_roundtrip(vr in arb_verification_report()) {
        let json = serde_json::to_string(&vr).unwrap();
        let rt: VerificationReport = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(vr.harness_ok, rt.harness_ok);
        prop_assert_eq!(&vr.git_diff, &rt.git_diff);
        prop_assert_eq!(&vr.git_status, &rt.git_status);
    }

    // 90
    #[test]
    fn context_snippet_serde_roundtrip(cs in arb_context_snippet()) {
        let json = serde_json::to_string(&cs).unwrap();
        let rt: ContextSnippet = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cs.name, &rt.name);
        prop_assert_eq!(&cs.content, &rt.content);
    }

    // 91
    #[test]
    fn capability_requirement_serde_roundtrip(cr in arb_capability_requirement()) {
        let json = serde_json::to_string(&cr).unwrap();
        let _rt: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    }

    // 92
    #[test]
    fn capability_requirements_serde_roundtrip(cr in arb_capability_requirements()) {
        let json = serde_json::to_string(&cr).unwrap();
        let rt: CapabilityRequirements = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cr.required.len(), rt.required.len());
    }
}
