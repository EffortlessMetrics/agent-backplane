// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-SDK property-based tests.
//!
//! Validates invariants across the contract surface: serde roundtrips,
//! deterministic hashing, IR fidelity, dialect detection idempotency,
//! policy compilation safety, and glob decision consistency.

use std::collections::BTreeMap;
use std::path::Path;

use proptest::prelude::*;
use serde_json::json;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec, canonical_json, receipt_hash, sha256_hex,
};
use abp_dialect::{Dialect, DialectDetector};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_receipt::{compute_hash, verify_hash};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════════

fn arb_safe_string() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_ ./-]{1,32}".boxed()
}

fn arb_short_string() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9_]{0,15}".boxed()
}

fn arb_uuid() -> BoxedStrategy<Uuid> {
    any::<u128>().prop_map(Uuid::from_u128).boxed()
}

fn arb_datetime() -> BoxedStrategy<chrono::DateTime<chrono::Utc>> {
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

// -- Enum strategies -------------------------------------------------------

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
    prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)].boxed()
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

fn arb_dialect() -> BoxedStrategy<Dialect> {
    prop_oneof![
        Just(Dialect::OpenAi),
        Just(Dialect::Claude),
        Just(Dialect::Gemini),
        Just(Dialect::Codex),
        Just(Dialect::Kimi),
        Just(Dialect::Copilot),
    ]
    .boxed()
}

// -- Compound strategies ---------------------------------------------------

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
            error_code: None,
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

// -- IR strategies ---------------------------------------------------------

fn arb_ir_role() -> BoxedStrategy<IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
    .boxed()
}

fn arb_ir_content_block() -> BoxedStrategy<IrContentBlock> {
    prop_oneof![
        arb_safe_string().prop_map(|text| IrContentBlock::Text { text }),
        (arb_safe_string(), arb_safe_string())
            .prop_map(|(media_type, data)| IrContentBlock::Image { media_type, data }),
        arb_safe_string().prop_map(|text| IrContentBlock::Thinking { text }),
        (arb_short_string(), arb_short_string(), arb_json_value())
            .prop_map(|(id, name, input)| IrContentBlock::ToolUse { id, name, input }),
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

// -- Glob strategies -------------------------------------------------------

fn arb_valid_glob() -> BoxedStrategy<String> {
    prop_oneof![
        Just("*".to_string()),
        Just("**".to_string()),
        Just("*.rs".to_string()),
        Just("**/*.rs".to_string()),
        Just("src/**".to_string()),
        Just("tests/**".to_string()),
        Just("*.txt".to_string()),
        Just("**/*.json".to_string()),
        Just("src/*.rs".to_string()),
        Just("docs/**/*.md".to_string()),
        arb_short_string().prop_map(|s| format!("{s}*")),
        arb_short_string().prop_map(|s| format!("**/{s}")),
    ]
    .boxed()
}

fn arb_path_string() -> BoxedStrategy<String> {
    prop_oneof![
        arb_safe_string(),
        arb_short_string().prop_map(|s| format!("src/{s}.rs")),
        arb_short_string().prop_map(|s| format!("tests/{s}.rs")),
        arb_short_string().prop_map(|s| format!("{s}.txt")),
        arb_short_string().prop_map(|s| format!("docs/{s}.md")),
    ]
    .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  WorkOrder serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs01_work_order_full_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    #[test]
    fn cs02_work_order_task_preserved(task in arb_safe_string()) {
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

    #[test]
    fn cs03_work_order_uuid_roundtrip(id in arb_uuid()) {
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

    #[test]
    fn cs04_work_order_lane_roundtrip(lane in arb_execution_lane()) {
        let wo = WorkOrder {
            id: Uuid::nil(),
            task: "t".into(),
            lane: lane.clone(),
            workspace: WorkspaceSpec { root: ".".into(), mode: WorkspaceMode::Staged, include: vec![], exclude: vec![] },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        let lane_json = serde_json::to_value(&lane).unwrap();
        let rt_lane_json = serde_json::to_value(&rt.lane).unwrap();
        prop_assert_eq!(lane_json, rt_lane_json);
    }

    #[test]
    fn cs05_work_order_workspace_roundtrip(ws in arb_workspace_spec()) {
        let wo = WorkOrder {
            id: Uuid::nil(),
            task: "t".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: ws.clone(),
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ws.root, &rt.workspace.root);
        prop_assert_eq!(&ws.include, &rt.workspace.include);
        prop_assert_eq!(&ws.exclude, &rt.workspace.exclude);
    }

    #[test]
    fn cs06_work_order_context_roundtrip(ctx in arb_context_packet()) {
        let wo = WorkOrder {
            id: Uuid::nil(),
            task: "t".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec { root: ".".into(), mode: WorkspaceMode::Staged, include: vec![], exclude: vec![] },
            context: ctx.clone(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ctx.files.len(), rt.context.files.len());
        prop_assert_eq!(ctx.snippets.len(), rt.context.snippets.len());
    }

    #[test]
    fn cs07_work_order_config_roundtrip(cfg in arb_runtime_config()) {
        let wo = WorkOrder {
            id: Uuid::nil(),
            task: "t".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec { root: ".".into(), mode: WorkspaceMode::Staged, include: vec![], exclude: vec![] },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: cfg.clone(),
        };
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cfg.model, &rt.config.model);
        prop_assert_eq!(cfg.max_turns, rt.config.max_turns);
    }

    #[test]
    fn cs08_work_order_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Receipt canonical hashing determinism
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs09_receipt_hash_deterministic(r in arb_receipt()) {
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        prop_assert_eq!(&h1, &h2);
    }

    #[test]
    fn cs10_receipt_hash_length(r in arb_receipt()) {
        let h = receipt_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
    }

    #[test]
    fn cs11_receipt_with_hash_sets_field(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(hashed.receipt_sha256.is_some());
        prop_assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn cs12_receipt_hash_ignores_existing_hash(r in arb_receipt()) {
        let mut r1 = r.clone();
        r1.receipt_sha256 = Some("abc".into());
        let mut r2 = r;
        r2.receipt_sha256 = Some("xyz".into());
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_eq!(&h1, &h2);
    }

    #[test]
    fn cs13_receipt_compute_hash_matches_core(r in arb_receipt()) {
        let core_hash = receipt_hash(&r).unwrap();
        let receipt_hash_val = compute_hash(&r).unwrap();
        prop_assert_eq!(&core_hash, &receipt_hash_val);
    }

    #[test]
    fn cs14_receipt_verify_hash_after_with_hash(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(verify_hash(&hashed));
    }

    #[test]
    fn cs15_sha256_hex_deterministic(data in prop::collection::vec(any::<u8>(), 0..128)) {
        let h1 = sha256_hex(&data);
        let h2 = sha256_hex(&data);
        prop_assert_eq!(&h1, &h2);
        prop_assert_eq!(h1.len(), 64);
    }

    #[test]
    fn cs16_canonical_json_deterministic(wo in arb_work_order()) {
        let j1 = canonical_json(&wo).unwrap();
        let j2 = canonical_json(&wo).unwrap();
        prop_assert_eq!(&j1, &j2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  AgentEvent serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs17_agent_event_serde_roundtrip(ev in arb_agent_event()) {
        let json = serde_json::to_string(&ev).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ev.ts, rt.ts);
    }

    #[test]
    fn cs18_agent_event_value_roundtrip(ev in arb_agent_event()) {
        let v = serde_json::to_value(&ev).unwrap();
        prop_assert!(v.is_object());
        let rt: AgentEvent = serde_json::from_value(v).unwrap();
        prop_assert_eq!(ev.ts, rt.ts);
    }

    #[test]
    fn cs19_agent_event_has_type_tag(ev in arb_agent_event()) {
        let v = serde_json::to_value(&ev).unwrap();
        prop_assert!(v.get("type").is_some(), "event must have a 'type' tag");
    }

    #[test]
    fn cs20_agent_event_type_tag_preserved(ev in arb_agent_event()) {
        let v1 = serde_json::to_value(&ev).unwrap();
        let rt: AgentEvent = serde_json::from_value(v1.clone()).unwrap();
        let v2 = serde_json::to_value(&rt).unwrap();
        prop_assert_eq!(&v1["type"], &v2["type"]);
    }

    #[test]
    fn cs21_agent_event_run_started_roundtrip(msg in arb_safe_string()) {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::RunStarted { message } = &rt.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    #[test]
    fn cs22_agent_event_tool_call_roundtrip(name in arb_short_string(), input in arb_json_value()) {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: name.clone(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: input.clone(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::ToolCall { tool_name, .. } = &rt.kind {
            prop_assert_eq!(&name, tool_name);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    #[test]
    fn cs23_agent_event_assistant_message_roundtrip(text in arb_safe_string()) {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantMessage { text: rt_text } = &rt.kind {
            prop_assert_eq!(&text, rt_text);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    #[test]
    fn cs24_agent_event_file_changed_roundtrip(
        path in arb_safe_string(),
        summary in arb_safe_string(),
    ) {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: path.clone(),
                summary: summary.clone(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::FileChanged { path: rp, summary: rs } = &rt.kind {
            prop_assert_eq!(&path, rp);
            prop_assert_eq!(&summary, rs);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  IR conversation roundtrip preserves message count
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs25_ir_conversation_message_count(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn cs26_ir_conversation_roles_preserved(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        for (orig, roundtripped) in conv.messages.iter().zip(rt.messages.iter()) {
            prop_assert_eq!(orig.role, roundtripped.role);
        }
    }

    #[test]
    fn cs27_ir_conversation_content_count_preserved(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        for (orig, roundtripped) in conv.messages.iter().zip(rt.messages.iter()) {
            prop_assert_eq!(orig.content.len(), roundtripped.content.len());
        }
    }

    #[test]
    fn cs28_ir_message_text_roundtrip(role in arb_ir_role(), text in arb_safe_string()) {
        let msg = IrMessage::text(role, &text);
        let json = serde_json::to_string(&msg).unwrap();
        let rt: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&text, &rt.text_content());
    }

    #[test]
    fn cs29_ir_message_is_text_only_preserved(msg in arb_ir_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let rt: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(msg.is_text_only(), rt.is_text_only());
    }

    #[test]
    fn cs30_ir_conversation_equality_after_roundtrip(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&conv, &rt);
    }

    #[test]
    fn cs31_ir_tool_definition_roundtrip(td in arb_ir_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let rt: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&td, &rt);
    }

    #[test]
    fn cs32_ir_usage_roundtrip(u in arb_ir_usage()) {
        let json = serde_json::to_string(&u).unwrap();
        let rt: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u, rt);
    }

    #[test]
    fn cs33_ir_usage_from_io_total(inp in 0u64..100_000, out in 0u64..100_000) {
        let u = IrUsage::from_io(inp, out);
        prop_assert_eq!(u.total_tokens, inp + out);
    }

    #[test]
    fn cs34_ir_usage_merge_commutative(a in arb_ir_usage(), b in arb_ir_usage()) {
        let ab = a.merge(b);
        let ba = b.merge(a);
        prop_assert_eq!(ab.input_tokens, ba.input_tokens);
        prop_assert_eq!(ab.output_tokens, ba.output_tokens);
        prop_assert_eq!(ab.total_tokens, ba.total_tokens);
    }

    #[test]
    fn cs35_ir_conversation_from_messages_len(msgs in prop::collection::vec(arb_ir_message(), 0..8)) {
        let n = msgs.len();
        let conv = IrConversation::from_messages(msgs);
        prop_assert_eq!(conv.len(), n);
        prop_assert_eq!(conv.is_empty(), n == 0);
    }

    #[test]
    fn cs36_ir_conversation_push_increments(conv in arb_ir_conversation(), msg in arb_ir_message()) {
        let before = conv.len();
        let after = conv.push(msg);
        prop_assert_eq!(after.len(), before + 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Dialect detection idempotency
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs37_dialect_detection_idempotent_openai(model in arb_short_string()) {
        let msg = json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}],
        });
        let detector = DialectDetector::new();
        let r1 = detector.detect(&msg);
        let r2 = detector.detect(&msg);
        match (r1, r2) {
            (Some(a), Some(b)) => {
                prop_assert_eq!(a.dialect, b.dialect);
                prop_assert!((a.confidence - b.confidence).abs() < f64::EPSILON);
            }
            (None, None) => {}
            _ => prop_assert!(false, "detection results differ in Some/None"),
        }
    }

    #[test]
    fn cs38_dialect_detection_idempotent_claude(model in arb_short_string()) {
        let msg = json!({
            "type": "message",
            "model": model,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        });
        let detector = DialectDetector::new();
        let r1 = detector.detect(&msg);
        let r2 = detector.detect(&msg);
        match (r1, r2) {
            (Some(a), Some(b)) => prop_assert_eq!(a.dialect, b.dialect),
            (None, None) => {}
            _ => prop_assert!(false, "detection mismatch"),
        }
    }

    #[test]
    fn cs39_dialect_detection_idempotent_gemini(_x in 0u8..1) {
        let msg = json!({
            "contents": [{"parts": [{"text": "hello"}]}],
        });
        let detector = DialectDetector::new();
        let r1 = detector.detect(&msg);
        let r2 = detector.detect(&msg);
        match (r1, r2) {
            (Some(a), Some(b)) => prop_assert_eq!(a.dialect, b.dialect),
            (None, None) => {}
            _ => prop_assert!(false, "detection mismatch"),
        }
    }

    #[test]
    fn cs40_dialect_detect_all_idempotent(model in arb_short_string()) {
        let msg = json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}],
        });
        let detector = DialectDetector::new();
        let r1 = detector.detect_all(&msg);
        let r2 = detector.detect_all(&msg);
        prop_assert_eq!(r1.len(), r2.len());
        for (a, b) in r1.iter().zip(r2.iter()) {
            prop_assert_eq!(a.dialect, b.dialect);
        }
    }

    #[test]
    fn cs41_dialect_serde_roundtrip(d in arb_dialect()) {
        let json = serde_json::to_value(d).unwrap();
        let rt: Dialect = serde_json::from_value(json).unwrap();
        prop_assert_eq!(d, rt);
    }

    #[test]
    fn cs42_dialect_label_not_empty(d in arb_dialect()) {
        prop_assert!(!d.label().is_empty());
    }

    #[test]
    fn cs43_dialect_display_matches_label(d in arb_dialect()) {
        prop_assert_eq!(d.to_string(), d.label());
    }

    #[test]
    fn cs44_dialect_non_object_returns_none(val in arb_json_value()) {
        if !val.is_object() {
            let detector = DialectDetector::new();
            prop_assert!(detector.detect(&val).is_none());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  PolicyProfile compilation never panics
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs45_policy_default_compiles(_x in 0u8..1) {
        let engine = PolicyEngine::new(&PolicyProfile::default());
        prop_assert!(engine.is_ok());
    }

    #[test]
    fn cs46_policy_arbitrary_safe_strings_compile(profile in arb_policy_profile()) {
        // Safe strings should compile without panicking (they may fail on invalid globs).
        let _engine = PolicyEngine::new(&profile);
    }

    #[test]
    fn cs47_policy_tool_decision_consistent(
        tool in arb_short_string(),
        profile in arb_policy_profile(),
    ) {
        if let Ok(engine) = PolicyEngine::new(&profile) {
            let d1 = engine.can_use_tool(&tool);
            let d2 = engine.can_use_tool(&tool);
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    #[test]
    fn cs48_policy_read_decision_consistent(
        path in arb_path_string(),
        profile in arb_policy_profile(),
    ) {
        if let Ok(engine) = PolicyEngine::new(&profile) {
            let d1 = engine.can_read_path(Path::new(&path));
            let d2 = engine.can_read_path(Path::new(&path));
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    #[test]
    fn cs49_policy_write_decision_consistent(
        path in arb_path_string(),
        profile in arb_policy_profile(),
    ) {
        if let Ok(engine) = PolicyEngine::new(&profile) {
            let d1 = engine.can_write_path(Path::new(&path));
            let d2 = engine.can_write_path(Path::new(&path));
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    #[test]
    fn cs50_policy_empty_allows_tool(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    #[test]
    fn cs51_policy_empty_allows_read(path in arb_path_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
    }

    #[test]
    fn cs52_policy_empty_allows_write(path in arb_path_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }

    #[test]
    fn cs53_policy_deny_tool_is_denied(tool in arb_short_string()) {
        let profile = PolicyProfile {
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&profile).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  IncludeExcludeGlobs decisions are consistent
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs54_glob_empty_allows_everything(path in arb_path_string()) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }

    #[test]
    fn cs55_glob_decision_idempotent(
        includes in prop::collection::vec(arb_valid_glob(), 0..3),
        excludes in prop::collection::vec(arb_valid_glob(), 0..3),
        path in arb_path_string(),
    ) {
        if let Ok(globs) = IncludeExcludeGlobs::new(&includes, &excludes) {
            let d1 = globs.decide_str(&path);
            let d2 = globs.decide_str(&path);
            prop_assert_eq!(d1, d2);
        }
    }

    #[test]
    fn cs56_glob_decide_path_matches_decide_str(
        includes in prop::collection::vec(arb_valid_glob(), 0..3),
        excludes in prop::collection::vec(arb_valid_glob(), 0..3),
        path in arb_path_string(),
    ) {
        if let Ok(globs) = IncludeExcludeGlobs::new(&includes, &excludes) {
            let str_decision = globs.decide_str(&path);
            let path_decision = globs.decide_path(Path::new(&path));
            prop_assert_eq!(str_decision, path_decision);
        }
    }

    #[test]
    fn cs57_glob_exclude_takes_precedence(
        glob_pat in arb_valid_glob(),
        path in arb_path_string(),
    ) {
        // If the same pattern is in both include and exclude, exclude wins.
        if let Ok(globs) = IncludeExcludeGlobs::new(
            std::slice::from_ref(&glob_pat),
            std::slice::from_ref(&glob_pat),
        ) {
            let decision = globs.decide_str(&path);
            // If the path matches the pattern, it must be denied by exclude.
            if decision != MatchDecision::DeniedByMissingInclude {
                prop_assert_eq!(decision, MatchDecision::DeniedByExclude);
            }
        }
    }

    #[test]
    fn cs58_glob_is_allowed_agrees_with_enum(
        includes in prop::collection::vec(arb_valid_glob(), 0..3),
        excludes in prop::collection::vec(arb_valid_glob(), 0..3),
        path in arb_path_string(),
    ) {
        if let Ok(globs) = IncludeExcludeGlobs::new(&includes, &excludes) {
            let decision = globs.decide_str(&path);
            prop_assert_eq!(
                decision.is_allowed(),
                decision == MatchDecision::Allowed,
            );
        }
    }

    #[test]
    fn cs59_glob_valid_patterns_compile(
        includes in prop::collection::vec(arb_valid_glob(), 0..4),
        excludes in prop::collection::vec(arb_valid_glob(), 0..4),
    ) {
        prop_assert!(IncludeExcludeGlobs::new(&includes, &excludes).is_ok());
    }

    #[test]
    fn cs60_glob_no_include_no_exclude_all_allowed(paths in prop::collection::vec(arb_path_string(), 1..8)) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        for p in &paths {
            prop_assert_eq!(globs.decide_str(p), MatchDecision::Allowed);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Cross-cutting invariants
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn cs61_receipt_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(r.meta.run_id, rt.meta.run_id);
        prop_assert_eq!(&r.backend.id, &rt.backend.id);
        prop_assert_eq!(r.trace.len(), rt.trace.len());
    }

    #[test]
    fn cs62_receipt_outcome_roundtrip(outcome in arb_outcome()) {
        let json = serde_json::to_string(&outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(outcome, rt);
    }

    #[test]
    fn cs63_capability_serde_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cap, rt);
    }

    #[test]
    fn cs64_execution_mode_roundtrip(mode in arb_execution_mode()) {
        let json = serde_json::to_string(&mode).unwrap();
        let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(mode, rt);
    }

    #[test]
    fn cs65_contract_version_in_receipt(r in arb_receipt()) {
        prop_assert_eq!(&r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn cs66_capability_manifest_roundtrip(manifest in arb_capability_manifest()) {
        let json = serde_json::to_string(&manifest).unwrap();
        let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), rt.len());
        for key in manifest.keys() {
            prop_assert!(rt.contains_key(key));
        }
    }

    #[test]
    fn cs67_ir_content_block_roundtrip(block in arb_ir_content_block()) {
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&block, &rt);
    }
}
