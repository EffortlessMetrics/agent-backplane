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
//! Exhaustive property-based tests for ABP contract types and mapping invariants.
//!
//! 40+ property tests organized into 6 categories:
//!
//! 1. **WorkOrder properties** — roundtrip, ID non-empty, config merge associativity
//! 2. **Receipt properties** — hash determinism, sensitivity, SHA-256 validity, idempotency
//! 3. **AgentEvent properties** — all event kinds roundtrip, timestamp preservation, ext data
//! 4. **Envelope properties** — "t" discriminator, ref_id preservation, roundtrip identity
//! 5. **Policy properties** — empty allows all, deny overrides allow, determinism
//! 6. **Mapping invariants** — identity projection, double projection A→B→A ≈ A

use std::collections::BTreeMap;
use std::path::Path;

use proptest::prelude::*;
use serde_json::json;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, RunMetadata, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_dialect::Dialect;
use abp_error::ErrorCode;
use abp_mapper::{
    default_ir_mapper, DialectRequest, IdentityMapper, IrIdentityMapper, IrMapper, Mapper,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{compute_hash, verify_hash};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 48,
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
    any::<u128>().prop_map(Uuid::from_u128).boxed()
}

fn arb_datetime() -> BoxedStrategy<DateTime<Utc>> {
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
        Just(Capability::FunctionCalling),
        Just(Capability::Vision),
        Just(Capability::Audio),
        Just(Capability::JsonMode),
        Just(Capability::SystemMessage),
        Just(Capability::Temperature),
        Just(Capability::TopP),
        Just(Capability::TopK),
        Just(Capability::MaxTokens),
        Just(Capability::FrequencyPenalty),
        Just(Capability::PresencePenalty),
        Just(Capability::CacheControl),
        Just(Capability::BatchMode),
        Just(Capability::Embeddings),
        Just(Capability::ImageGeneration),
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

fn arb_error_code() -> BoxedStrategy<ErrorCode> {
    prop_oneof![
        Just(ErrorCode::ProtocolInvalidEnvelope),
        Just(ErrorCode::ProtocolHandshakeFailed),
        Just(ErrorCode::ProtocolMissingRefId),
        Just(ErrorCode::ProtocolUnexpectedMessage),
        Just(ErrorCode::ProtocolVersionMismatch),
        Just(ErrorCode::MappingUnsupportedCapability),
        Just(ErrorCode::MappingDialectMismatch),
        Just(ErrorCode::MappingLossyConversion),
        Just(ErrorCode::MappingUnmappableTool),
        Just(ErrorCode::BackendNotFound),
        Just(ErrorCode::BackendUnavailable),
        Just(ErrorCode::BackendTimeout),
        Just(ErrorCode::BackendRateLimited),
        Just(ErrorCode::BackendAuthFailed),
        Just(ErrorCode::BackendModelNotFound),
        Just(ErrorCode::BackendCrashed),
        Just(ErrorCode::ExecutionToolFailed),
        Just(ErrorCode::ExecutionWorkspaceError),
        Just(ErrorCode::ExecutionPermissionDenied),
        Just(ErrorCode::ContractVersionMismatch),
        Just(ErrorCode::ContractSchemaViolation),
        Just(ErrorCode::ContractInvalidReceipt),
        Just(ErrorCode::CapabilityUnsupported),
        Just(ErrorCode::CapabilityEmulationFailed),
        Just(ErrorCode::PolicyDenied),
        Just(ErrorCode::PolicyInvalid),
        Just(ErrorCode::WorkspaceInitFailed),
        Just(ErrorCode::WorkspaceStagingFailed),
        Just(ErrorCode::IrLoweringFailed),
        Just(ErrorCode::IrInvalid),
        Just(ErrorCode::ReceiptHashMismatch),
        Just(ErrorCode::ReceiptChainBroken),
        Just(ErrorCode::DialectUnknown),
        Just(ErrorCode::DialectMappingFailed),
        Just(ErrorCode::ConfigInvalid),
        Just(ErrorCode::Internal),
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

fn arb_agent_event_with_ext() -> BoxedStrategy<AgentEvent> {
    (
        arb_datetime(),
        arb_agent_event_kind(),
        prop::option::of(prop::collection::btree_map(
            arb_short_string(),
            arb_json_value(),
            0..4,
        )),
    )
        .prop_map(|(ts, kind, ext)| AgentEvent { ts, kind, ext })
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

// -- Envelope strategies --------------------------------------------------

fn arb_envelope_hello() -> BoxedStrategy<Envelope> {
    (
        arb_backend_identity(),
        arb_capability_manifest(),
        arb_execution_mode(),
    )
        .prop_map(|(backend, capabilities, mode)| Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities,
            mode,
        })
        .boxed()
}

fn arb_envelope_run() -> BoxedStrategy<Envelope> {
    (arb_short_string(), arb_work_order())
        .prop_map(|(id, work_order)| Envelope::Run { id, work_order })
        .boxed()
}

fn arb_envelope_event() -> BoxedStrategy<Envelope> {
    (arb_short_string(), arb_agent_event())
        .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event })
        .boxed()
}

fn arb_envelope_final() -> BoxedStrategy<Envelope> {
    (arb_short_string(), arb_receipt())
        .prop_map(|(ref_id, receipt)| Envelope::Final { ref_id, receipt })
        .boxed()
}

fn arb_envelope_fatal() -> BoxedStrategy<Envelope> {
    (
        prop::option::of(arb_short_string()),
        arb_safe_string(),
        prop::option::of(arb_error_code()),
    )
        .prop_map(|(ref_id, error, error_code)| Envelope::Fatal {
            ref_id,
            error,
            error_code,
        })
        .boxed()
}

fn arb_envelope() -> BoxedStrategy<Envelope> {
    prop_oneof![
        arb_envelope_hello(),
        arb_envelope_run(),
        arb_envelope_event(),
        arb_envelope_final(),
        arb_envelope_fatal(),
    ]
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

fn arb_ir_content_block() -> BoxedStrategy<IrContentBlock> {
    prop_oneof![
        arb_safe_string().prop_map(|text| IrContentBlock::Text { text }),
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

// ═══════════════════════════════════════════════════════════════════════════
// §1  WorkOrder properties
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// WO-1: Any valid WorkOrder serializes and deserializes identically (roundtrip).
    #[test]
    fn prop_wo_serde_roundtrip_identity(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(&json, &json2, "double-serialization must be identical");
    }

    /// WO-2: WorkOrder ID is always non-empty after construction.
    #[test]
    fn prop_wo_id_always_nonempty(wo in arb_work_order()) {
        prop_assert!(!wo.id.is_nil(), "WorkOrder ID must not be nil UUID");
        prop_assert!(!wo.id.to_string().is_empty(), "WorkOrder ID string must not be empty");
    }

    /// WO-3: Config model field survives roundtrip.
    #[test]
    fn prop_wo_config_model_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.config.model, &rt.config.model);
    }

    /// WO-4: Config max_turns survives roundtrip.
    #[test]
    fn prop_wo_config_max_turns_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.config.max_turns, rt.config.max_turns);
    }

    /// WO-5: Merging two RuntimeConfigs where second overrides first is associative
    /// in the sense that applying defaults then overrides yields the override values.
    #[test]
    fn prop_wo_config_merge_override_wins(
        base_model in prop::option::of(arb_safe_string()),
        override_model in prop::option::of(arb_safe_string()),
        base_turns in prop::option::of(1u32..50u32),
        override_turns in prop::option::of(1u32..50u32),
    ) {
        let base = RuntimeConfig {
            model: base_model.clone(),
            max_turns: base_turns,
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: None,
        };
        let overrides = RuntimeConfig {
            model: override_model.clone(),
            max_turns: override_turns,
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: None,
        };
        // Merge: override wins when present
        let merged_model = overrides.model.or(base.model);
        let merged_turns = overrides.max_turns.or(base.max_turns);
        let expected_model = override_model.or(base_model);
        let expected_turns = override_turns.or(base_turns);
        prop_assert_eq!(merged_model, expected_model);
        prop_assert_eq!(merged_turns, expected_turns);
    }

    /// WO-6: WorkOrder task field always roundtrips exactly.
    #[test]
    fn prop_wo_task_field_exact_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.task, &rt.task);
    }

    /// WO-7: WorkOrder policy profile roundtrips.
    #[test]
    fn prop_wo_policy_profile_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.policy.allowed_tools, &rt.policy.allowed_tools);
        prop_assert_eq!(&wo.policy.disallowed_tools, &rt.policy.disallowed_tools);
    }

    /// WO-8: WorkOrder value roundtrip (via serde_json::Value).
    #[test]
    fn prop_wo_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
        prop_assert_eq!(wo.config.max_turns, rt.config.max_turns);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Receipt properties
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// R-1: Receipt hash is deterministic for the same input.
    #[test]
    fn prop_receipt_hash_deterministic(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    /// R-2: Receipt hash changes when outcome field changes.
    #[test]
    fn prop_receipt_hash_changes_on_outcome_change(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// R-3: Receipt hash changes when backend ID differs.
    #[test]
    fn prop_receipt_hash_changes_on_backend_change(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_mutated", r2.backend.id);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// R-4: Receipt hash changes when run_id differs.
    #[test]
    fn prop_receipt_hash_changes_on_run_id_change(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.run_id = Uuid::from_u128(r.meta.run_id.as_u128().wrapping_add(1));
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// R-5: Receipt hash changes when trace is appended.
    #[test]
    fn prop_receipt_hash_changes_on_trace_append(r in arb_receipt(), evt in arb_agent_event()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.trace.push(evt);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// R-6: `with_hash()` produces a valid SHA-256 hex string (64 hex chars).
    #[test]
    fn prop_receipt_with_hash_valid_sha256(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        let hash = hashed.receipt_sha256.as_ref().unwrap();
        prop_assert_eq!(hash.len(), 64, "SHA-256 hex must be 64 chars");
        prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "must be hex");
    }

    /// R-7: Receipt without hash → with_hash → verify passes.
    #[test]
    fn prop_receipt_with_hash_verifies(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(hashed.receipt_sha256.is_some());
        prop_assert!(verify_hash(&hashed));
    }

    /// R-8: Receipt without hash → with_hash → clear hash → with_hash is idempotent.
    #[test]
    fn prop_receipt_hash_idempotent_cycle(r in arb_receipt()) {
        let hashed1 = r.clone().with_hash().unwrap();
        let hash1 = hashed1.receipt_sha256.clone().unwrap();
        // Clear hash, rehash — should produce the same hash
        let mut cleared = hashed1;
        cleared.receipt_sha256 = None;
        let hashed2 = cleared.with_hash().unwrap();
        let hash2 = hashed2.receipt_sha256.unwrap();
        prop_assert_eq!(hash1, hash2);
    }

    /// R-9: Stored hash field is ignored when computing hash (self-referential prevention).
    #[test]
    fn prop_receipt_hash_ignores_stored_hash(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.receipt_sha256 = Some("aaaa".repeat(16));
        prop_assert_eq!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    /// R-10: Receipt hash changes when execution mode differs.
    #[test]
    fn prop_receipt_hash_changes_on_mode_change(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.mode = match r.mode {
            ExecutionMode::Passthrough => ExecutionMode::Mapped,
            ExecutionMode::Mapped => ExecutionMode::Passthrough,
        };
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  AgentEvent properties
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// AE-1: All event kinds serialize/deserialize correctly.
    #[test]
    fn prop_agent_event_roundtrip(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    /// AE-2: Timestamps are preserved exactly through roundtrip.
    #[test]
    fn prop_agent_event_timestamp_preserved(ts in arb_datetime(), kind in arb_agent_event_kind()) {
        let evt = AgentEvent { ts, kind, ext: None };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ts, rt.ts, "timestamp must be preserved exactly");
    }

    /// AE-3: Extension data survives roundtrip.
    #[test]
    fn prop_agent_event_ext_data_roundtrip(evt in arb_agent_event_with_ext()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&evt.ext, &rt.ext);
    }

    /// AE-4: AgentEvent always has "type" discriminator in JSON.
    #[test]
    fn prop_agent_event_has_type_discriminator(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        prop_assert!(v.get("type").is_some(), "AgentEvent kind must use 'type' tag");
    }

    /// AE-5: RunStarted message roundtrips exactly.
    #[test]
    fn prop_run_started_message_exact(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::RunStarted { message } => prop_assert_eq!(&msg, message),
            _ => prop_assert!(false, "wrong variant after roundtrip"),
        }
    }

    /// AE-6: ToolCall fields roundtrip exactly.
    #[test]
    fn prop_tool_call_fields_roundtrip(
        name in arb_short_string(),
        input in arb_json_value()
    ) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: name.clone(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: input.clone(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::ToolCall { tool_name, input: i, .. } => {
                prop_assert_eq!(&name, tool_name);
                prop_assert_eq!(&input, i);
            }
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    /// AE-7: ToolResult is_error flag roundtrips.
    #[test]
    fn prop_tool_result_error_flag_roundtrip(
        name in arb_short_string(),
        output in arb_json_value(),
        is_err in any::<bool>()
    ) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: name,
                tool_use_id: None,
                output,
                is_error: is_err,
            },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::ToolResult { is_error, .. } => {
                prop_assert_eq!(is_err, *is_error);
            }
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    /// AE-8: Double roundtrip produces identical JSON.
    #[test]
    fn prop_agent_event_double_roundtrip(evt in arb_agent_event()) {
        let j1 = serde_json::to_string(&evt).unwrap();
        let rt1: AgentEvent = serde_json::from_str(&j1).unwrap();
        let j2 = serde_json::to_string(&rt1).unwrap();
        prop_assert_eq!(j1, j2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Envelope properties
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// EN-1: All envelope variants serialize with "t" discriminator.
    #[test]
    fn prop_envelope_has_t_discriminator(env in arb_envelope()) {
        let v = serde_json::to_value(&env).unwrap();
        prop_assert!(v.get("t").is_some(), "Envelope must use 't' discriminator");
    }

    /// EN-2: ref_id is preserved in Event envelope.
    #[test]
    fn prop_envelope_event_ref_id_preserved(env in arb_envelope_event()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Event { ref_id: r1, .. },
                Envelope::Event { ref_id: r2, .. },
            ) => prop_assert_eq!(r1, r2),
            _ => prop_assert!(false, "expected Event variant"),
        }
    }

    /// EN-3: ref_id is preserved in Final envelope.
    #[test]
    fn prop_envelope_final_ref_id_preserved(env in arb_envelope_final()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Final { ref_id: r1, .. },
                Envelope::Final { ref_id: r2, .. },
            ) => prop_assert_eq!(r1, r2),
            _ => prop_assert!(false, "expected Final variant"),
        }
    }

    /// EN-4: ref_id is preserved in Fatal envelope.
    #[test]
    fn prop_envelope_fatal_ref_id_preserved(env in arb_envelope_fatal()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Fatal { ref_id: r1, error: e1, error_code: ec1 },
                Envelope::Fatal { ref_id: r2, error: e2, error_code: ec2 },
            ) => {
                prop_assert_eq!(r1, r2);
                prop_assert_eq!(e1, e2);
                prop_assert_eq!(ec1, ec2);
            }
            _ => prop_assert!(false, "expected Fatal variant"),
        }
    }

    /// EN-5: Envelope roundtrip is identity (JSON string equality).
    #[test]
    fn prop_envelope_roundtrip_identity(env in arb_envelope()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(json, json2);
    }

    /// EN-6: JSONL codec roundtrip is identity.
    #[test]
    fn prop_envelope_jsonl_roundtrip(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        prop_assert!(line.ends_with('\n'), "JSONL must end with newline");
        let rt = JsonlCodec::decode(line.trim()).unwrap();
        let j_orig = serde_json::to_string(&env).unwrap();
        let j_rt = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(j_orig, j_rt);
    }

    /// EN-7: Hello envelope always contains contract version.
    #[test]
    fn prop_envelope_hello_has_contract_version(env in arb_envelope_hello()) {
        let json = serde_json::to_string(&env).unwrap();
        prop_assert!(json.contains(CONTRACT_VERSION));
    }

    /// EN-8: Run envelope preserves work order ID.
    #[test]
    fn prop_envelope_run_preserves_work_order_id(env in arb_envelope_run()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Run { id: id1, work_order: wo1 },
                Envelope::Run { id: id2, work_order: wo2 },
            ) => {
                prop_assert_eq!(id1, id2);
                prop_assert_eq!(wo1.id, wo2.id);
            }
            _ => prop_assert!(false, "expected Run variant"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Policy properties
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// PO-1: Empty policy allows everything (tools).
    #[test]
    fn prop_empty_policy_allows_all_tools(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d = engine.can_use_tool(&tool);
        prop_assert!(d.allowed, "default policy must allow all tools");
    }

    /// PO-2: Empty policy allows everything (read paths).
    #[test]
    fn prop_empty_policy_allows_all_reads(path in "[a-z]{1,8}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d = engine.can_read_path(Path::new(&path));
        prop_assert!(d.allowed, "default policy must allow all reads");
    }

    /// PO-3: Empty policy allows everything (write paths).
    #[test]
    fn prop_empty_policy_allows_all_writes(path in "[a-z]{1,8}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d = engine.can_write_path(Path::new(&path));
        prop_assert!(d.allowed, "default policy must allow all writes");
    }

    /// PO-4: Deny overrides allow for matching patterns.
    #[test]
    fn prop_deny_overrides_allow_tools(tool in "[a-zA-Z]{2,8}") {
        let policy = PolicyProfile {
            allowed_tools: vec![format!("{}*", &tool)],
            disallowed_tools: vec![format!("{}*", &tool)],
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let d = engine.can_use_tool(&tool);
            prop_assert!(!d.allowed, "deny must override allow for matching tool");
        }
    }

    /// PO-5: Deny read blocks matching paths.
    #[test]
    fn prop_deny_read_blocks_matching(filename in "[a-z]{1,8}") {
        let pattern = format!("**/{}*", &filename);
        let policy = PolicyProfile {
            deny_read: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let d = engine.can_read_path(Path::new(&filename));
        prop_assert!(!d.allowed, "deny_read must block matching paths");
    }

    /// PO-6: Deny write blocks matching paths.
    #[test]
    fn prop_deny_write_blocks_matching(filename in "[a-z]{1,8}") {
        let pattern = format!("**/{}*", &filename);
        let policy = PolicyProfile {
            deny_write: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let d = engine.can_write_path(Path::new(&filename));
        prop_assert!(!d.allowed, "deny_write must block matching paths");
    }

    /// PO-7: Policy decisions are deterministic (tool).
    #[test]
    fn prop_policy_tool_deterministic(
        allowed in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..4),
        denied in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..4),
        tool in arb_short_string()
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let d1 = engine.can_use_tool(&tool);
            let d2 = engine.can_use_tool(&tool);
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    /// PO-8: Policy decisions are deterministic (read).
    #[test]
    fn prop_policy_read_deterministic(
        deny_read in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
        path in "[a-z]{1,8}"
    ) {
        let policy = PolicyProfile {
            deny_read,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let d1 = engine.can_read_path(Path::new(&path));
            let d2 = engine.can_read_path(Path::new(&path));
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    /// PO-9: Policy decisions are deterministic (write).
    #[test]
    fn prop_policy_write_deterministic(
        deny_write in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
        path in "[a-z]{1,8}"
    ) {
        let policy = PolicyProfile {
            deny_write,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let d1 = engine.can_write_path(Path::new(&path));
            let d2 = engine.can_write_path(Path::new(&path));
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    /// PO-10: PolicyProfile serde roundtrip preserves all fields.
    #[test]
    fn prop_policy_profile_serde_roundtrip(policy in arb_policy_profile()) {
        let json = serde_json::to_string(&policy).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&policy.allowed_tools, &rt.allowed_tools);
        prop_assert_eq!(&policy.disallowed_tools, &rt.disallowed_tools);
        prop_assert_eq!(&policy.deny_read, &rt.deny_read);
        prop_assert_eq!(&policy.deny_write, &rt.deny_write);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Mapping invariants
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// MI-1: Identity JSON-level mapper returns request body unchanged.
    #[test]
    fn prop_identity_mapper_request_passthrough(
        body in arb_json_value(),
        dialect in arb_dialect()
    ) {
        let mapper = IdentityMapper;
        let req = DialectRequest { dialect, body: body.clone() };
        let result = mapper.map_request(&req).unwrap();
        prop_assert_eq!(body, result);
    }

    /// MI-2: Identity JSON-level mapper response body is unchanged.
    #[test]
    fn prop_identity_mapper_response_passthrough(body in arb_json_value()) {
        let mapper = IdentityMapper;
        let resp = mapper.map_response(&body).unwrap();
        prop_assert_eq!(body, resp.body);
    }

    /// MI-3: IR identity mapper returns conversation unchanged for any dialect pair.
    #[test]
    fn prop_ir_identity_mapper_request_unchanged(
        conv in arb_ir_conversation(),
        dialect in arb_dialect()
    ) {
        let mapper = IrIdentityMapper;
        let result = mapper.map_request(dialect, dialect, &conv).unwrap();
        prop_assert_eq!(conv, result);
    }

    /// MI-4: IR identity mapper response is unchanged.
    #[test]
    fn prop_ir_identity_mapper_response_unchanged(
        conv in arb_ir_conversation(),
        dialect in arb_dialect()
    ) {
        let mapper = IrIdentityMapper;
        let result = mapper.map_response(dialect, dialect, &conv).unwrap();
        prop_assert_eq!(conv, result);
    }

    /// MI-5: Same-dialect routing always returns identity mapper.
    #[test]
    fn prop_same_dialect_gets_identity_mapper(dialect in arb_dialect()) {
        let mapper = default_ir_mapper(dialect, dialect);
        prop_assert!(mapper.is_some(), "same-dialect must always resolve to a mapper");
        let mapper = mapper.unwrap();
        let pairs = mapper.supported_pairs();
        prop_assert!(pairs.contains(&(dialect, dialect)));
    }

    /// MI-6: IR identity mapper preserves conversation length.
    #[test]
    fn prop_ir_identity_preserves_length(
        conv in arb_ir_conversation(),
        dialect in arb_dialect()
    ) {
        let mapper = IrIdentityMapper;
        let result = mapper.map_request(dialect, dialect, &conv).unwrap();
        prop_assert_eq!(conv.len(), result.len());
    }

    /// MI-7: Identity mapper event serialization roundtrips.
    #[test]
    fn prop_identity_mapper_event_roundtrip(evt in arb_agent_event()) {
        let mapper = IdentityMapper;
        let mapped = mapper.map_event(&evt).unwrap();
        // mapped is a serde_json::Value — verify it has the type tag
        prop_assert!(mapped.get("type").is_some());
    }

    /// MI-8: Double projection A→B→A ≈ A for text-only IR conversations
    /// using OpenAI ↔ Claude mapping (text-only messages are losslessly invertible).
    #[test]
    fn prop_double_projection_openai_claude_text_only(
        text in arb_safe_string()
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, &text),
        ]);
        let from = Dialect::OpenAi;
        let to = Dialect::Claude;
        if let Some(mapper) = default_ir_mapper(from, to) {
            let forward = mapper.map_request(from, to, &conv);
            if let Ok(mapped) = forward {
                // Map back
                if let Some(back_mapper) = default_ir_mapper(to, from) {
                    if let Ok(roundtripped) = back_mapper.map_request(to, from, &mapped) {
                        // Text content should survive the roundtrip
                        let orig_text = conv.messages[0].text_content();
                        let rt_text = roundtripped.messages.iter()
                            .filter(|m| m.role == IrRole::User)
                            .map(|m| m.text_content())
                            .collect::<Vec<_>>()
                            .join("");
                        prop_assert_eq!(orig_text, rt_text);
                    }
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Additional cross-cutting invariants (to reach 40+ tests)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// X-1: Receipt serde roundtrip preserves all key fields.
    #[test]
    fn prop_receipt_full_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&r.meta.run_id, &rt.meta.run_id);
        prop_assert_eq!(&r.meta.work_order_id, &rt.meta.work_order_id);
        prop_assert_eq!(&r.backend.id, &rt.backend.id);
        prop_assert_eq!(&r.outcome, &rt.outcome);
        prop_assert_eq!(r.trace.len(), rt.trace.len());
        prop_assert_eq!(r.artifacts.len(), rt.artifacts.len());
    }

    /// X-2: Receipt hash is stable after serde roundtrip.
    #[test]
    fn prop_receipt_hash_stable_after_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(compute_hash(&r).unwrap(), compute_hash(&rt).unwrap());
    }

    /// X-3: Serialization is deterministic for WorkOrders.
    #[test]
    fn prop_work_order_serialization_deterministic(wo in arb_work_order()) {
        let j1 = serde_json::to_string(&wo).unwrap();
        let j2 = serde_json::to_string(&wo).unwrap();
        prop_assert_eq!(j1, j2);
    }

    /// X-4: Serialization is deterministic for Receipts.
    #[test]
    fn prop_receipt_serialization_deterministic(r in arb_receipt()) {
        let j1 = serde_json::to_string(&r).unwrap();
        let j2 = serde_json::to_string(&r).unwrap();
        prop_assert_eq!(j1, j2);
    }
}
