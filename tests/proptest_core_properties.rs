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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive property-based tests for core ABP types.
//!
//! Covers 12 property categories:
//! 1. WorkOrder serde roundtrip
//! 2. Receipt hash determinism
//! 3. Receipt hash uniqueness (sensitivity)
//! 4. AgentEvent serde roundtrip
//! 5. Envelope serde roundtrip
//! 6. IR type roundtrip
//! 7. Policy decisions are deterministic
//! 8. Glob patterns produce correct decisions
//! 9. ErrorCode stability
//! 10. Capability negotiation commutativity
//! 11. Deterministic serialization (BTreeMap)
//! 12. Contract version consistency

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use serde_json::json;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_error::ErrorCode;
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
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

// ═══════════════════════════════════════════════════════════════════════════
// §1  WorkOrder serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_work_order_json_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    #[test]
    fn prop_work_order_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    #[test]
    fn prop_work_order_lane_preserved(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_string(&wo.lane).unwrap(),
            serde_json::to_string(&rt.lane).unwrap()
        );
    }

    #[test]
    fn prop_work_order_workspace_roundtrip(ws in arb_workspace_spec()) {
        let json = serde_json::to_string(&ws).unwrap();
        let rt: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ws.root, &rt.root);
        prop_assert_eq!(&ws.include, &rt.include);
        prop_assert_eq!(&ws.exclude, &rt.exclude);
    }

    #[test]
    fn prop_work_order_context_roundtrip(ctx in arb_context_packet()) {
        let json = serde_json::to_string(&ctx).unwrap();
        let rt: ContextPacket = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ctx.files.len(), rt.files.len());
        prop_assert_eq!(ctx.snippets.len(), rt.snippets.len());
    }

    #[test]
    fn prop_work_order_config_roundtrip(cfg in arb_runtime_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cfg.model, &rt.model);
        prop_assert_eq!(cfg.max_turns, rt.max_turns);
    }

    #[test]
    fn prop_work_order_double_roundtrip(wo in arb_work_order()) {
        let j1 = serde_json::to_string(&wo).unwrap();
        let rt1: WorkOrder = serde_json::from_str(&j1).unwrap();
        let j2 = serde_json::to_string(&rt1).unwrap();
        let rt2: WorkOrder = serde_json::from_str(&j2).unwrap();
        prop_assert_eq!(rt1.id, rt2.id);
        prop_assert_eq!(&rt1.task, &rt2.task);
        prop_assert_eq!(j1, j2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Receipt hash determinism
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_receipt_hash_deterministic(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    #[test]
    fn prop_receipt_hash_is_64_hex_chars(r in arb_receipt()) {
        let h = compute_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn prop_receipt_hash_ignores_stored_hash(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.receipt_sha256 = Some("deadbeef".into());
        prop_assert_eq!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_receipt_with_hash_verifies(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(hashed.receipt_sha256.is_some());
        prop_assert!(verify_hash(&hashed));
    }

    #[test]
    fn prop_receipt_hash_stable_after_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(compute_hash(&r).unwrap(), compute_hash(&rt).unwrap());
    }

    #[test]
    fn prop_receipt_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&r.meta.run_id, &rt.meta.run_id);
        prop_assert_eq!(&r.outcome, &rt.outcome);
        prop_assert_eq!(&r.backend.id, &rt.backend.id);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Receipt hash uniqueness (sensitivity)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_hash_changes_when_outcome_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_backend_id_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_x", r2.backend.id);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_run_id_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.run_id = Uuid::from_u128(r.meta.run_id.as_u128().wrapping_add(1));
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_duration_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.duration_ms = r.meta.duration_ms.wrapping_add(1);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_mode_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.mode = match r.mode {
            ExecutionMode::Passthrough => ExecutionMode::Mapped,
            ExecutionMode::Mapped => ExecutionMode::Passthrough,
        };
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_contract_version_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.contract_version = format!("{}_x", r2.meta.contract_version);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_trace_appended(r in arb_receipt(), evt in arb_agent_event()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.trace.push(evt);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_harness_ok_flipped(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.verification.harness_ok = !r.verification.harness_ok;
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    #[test]
    fn prop_hash_changes_when_work_order_id_differs(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.work_order_id = Uuid::from_u128(r.meta.work_order_id.as_u128().wrapping_add(1));
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  AgentEvent serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_agent_event_json_roundtrip(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    #[test]
    fn prop_agent_event_value_roundtrip(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_value(v).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    #[test]
    fn prop_agent_event_has_type_tag(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        prop_assert!(v.get("type").is_some(), "AgentEvent must have 'type' discriminator");
    }

    #[test]
    fn prop_run_started_roundtrip(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::RunStarted { message } => prop_assert_eq!(&msg, message),
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn prop_assistant_message_roundtrip(text in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::AssistantMessage { text: t } => prop_assert_eq!(&text, t),
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn prop_tool_call_roundtrip(name in arb_short_string(), val in arb_json_value()) {
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
        match &rt.kind {
            AgentEventKind::ToolCall { tool_name, input, .. } => {
                prop_assert_eq!(&name, tool_name);
                prop_assert_eq!(&val, input);
            }
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn prop_tool_result_roundtrip(
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
        match &rt.kind {
            AgentEventKind::ToolResult { tool_name, output, is_error, .. } => {
                prop_assert_eq!(&name, tool_name);
                prop_assert_eq!(&val, output);
                prop_assert_eq!(is_err, *is_error);
            }
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn prop_file_changed_roundtrip(path in arb_safe_string(), summary in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged { path: path.clone(), summary: summary.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::FileChanged { path: p, summary: s } => {
                prop_assert_eq!(&path, p);
                prop_assert_eq!(&summary, s);
            }
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn prop_warning_roundtrip(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::Warning { message } => prop_assert_eq!(&msg, message),
            _ => prop_assert!(false, "wrong variant"),
        }
    }

    #[test]
    fn prop_error_event_roundtrip(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error { message: msg.clone(), error_code: None },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        match &rt.kind {
            AgentEventKind::Error { message, .. } => prop_assert_eq!(&msg, message),
            _ => prop_assert!(false, "wrong variant"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Envelope serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_envelope_json_roundtrip(env in arb_envelope()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        let j2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(json, j2);
    }

    #[test]
    fn prop_envelope_has_t_tag(env in arb_envelope()) {
        let v = serde_json::to_value(&env).unwrap();
        prop_assert!(v.get("t").is_some(), "Envelope must use 't' discriminator");
    }

    #[test]
    fn prop_envelope_hello_roundtrip(env in arb_envelope_hello()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Hello { contract_version: cv1, backend: b1, .. },
                Envelope::Hello { contract_version: cv2, backend: b2, .. },
            ) => {
                prop_assert_eq!(cv1, cv2);
                prop_assert_eq!(&b1.id, &b2.id);
            }
            _ => prop_assert!(false, "expected Hello variant"),
        }
    }

    #[test]
    fn prop_envelope_run_roundtrip(env in arb_envelope_run()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Run { id: id1, work_order: wo1 },
                Envelope::Run { id: id2, work_order: wo2 },
            ) => {
                prop_assert_eq!(id1, id2);
                prop_assert_eq!(wo1.id, wo2.id);
                prop_assert_eq!(&wo1.task, &wo2.task);
            }
            _ => prop_assert!(false, "expected Run variant"),
        }
    }

    #[test]
    fn prop_envelope_event_roundtrip(env in arb_envelope_event()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Event { ref_id: r1, event: e1 },
                Envelope::Event { ref_id: r2, event: e2 },
            ) => {
                prop_assert_eq!(r1, r2);
                prop_assert_eq!(e1.ts, e2.ts);
            }
            _ => prop_assert!(false, "expected Event variant"),
        }
    }

    #[test]
    fn prop_envelope_final_roundtrip(env in arb_envelope_final()) {
        let json = serde_json::to_string(&env).unwrap();
        let rt: Envelope = serde_json::from_str(&json).unwrap();
        match (&env, &rt) {
            (
                Envelope::Final { ref_id: r1, receipt: rc1 },
                Envelope::Final { ref_id: r2, receipt: rc2 },
            ) => {
                prop_assert_eq!(r1, r2);
                prop_assert_eq!(rc1.meta.run_id, rc2.meta.run_id);
            }
            _ => prop_assert!(false, "expected Final variant"),
        }
    }

    #[test]
    fn prop_envelope_fatal_roundtrip(env in arb_envelope_fatal()) {
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

    #[test]
    fn prop_envelope_jsonl_codec_roundtrip(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        prop_assert!(line.ends_with('\n'), "JSONL line must end with newline");
        let rt = JsonlCodec::decode(line.trim()).unwrap();
        let j_orig = serde_json::to_string(&env).unwrap();
        let j_rt = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(j_orig, j_rt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  IR type roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_ir_role_roundtrip(role in arb_ir_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let rt: IrRole = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(role, rt);
    }

    #[test]
    fn prop_ir_content_block_roundtrip(block in arb_ir_content_block()) {
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    #[test]
    fn prop_ir_message_roundtrip(msg in arb_ir_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let rt: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(msg, rt);
    }

    #[test]
    fn prop_ir_conversation_roundtrip(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv, rt);
    }

    #[test]
    fn prop_ir_conversation_length_preserved(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn prop_ir_tool_definition_roundtrip(td in arb_ir_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let rt: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(td, rt);
    }

    #[test]
    fn prop_ir_usage_roundtrip(u in arb_ir_usage()) {
        let json = serde_json::to_string(&u).unwrap();
        let rt: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u, rt);
    }

    #[test]
    fn prop_ir_usage_merge_commutative(a in arb_ir_usage(), b in arb_ir_usage()) {
        let ab = a.merge(b);
        let ba = b.merge(a);
        prop_assert_eq!(ab, ba);
    }

    #[test]
    fn prop_ir_tool_result_nested_roundtrip(
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
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Policy decisions are deterministic
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_policy_default_allows_any_tool(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d1 = engine.can_use_tool(&tool);
        let d2 = engine.can_use_tool(&tool);
        prop_assert_eq!(d1.allowed, d2.allowed);
        prop_assert!(d1.allowed, "default policy allows everything");
    }

    #[test]
    fn prop_policy_tool_decision_deterministic(
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

    #[test]
    fn prop_policy_read_decision_deterministic(
        deny_read in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
        path in "[a-z]{1,8}"
    ) {
        let policy = PolicyProfile {
            deny_read,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let d1 = engine.can_read_path(std::path::Path::new(&path));
            let d2 = engine.can_read_path(std::path::Path::new(&path));
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    #[test]
    fn prop_policy_write_decision_deterministic(
        deny_write in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
        path in "[a-z]{1,8}"
    ) {
        let policy = PolicyProfile {
            deny_write,
            ..PolicyProfile::default()
        };
        if let Ok(engine) = PolicyEngine::new(&policy) {
            let d1 = engine.can_write_path(std::path::Path::new(&path));
            let d2 = engine.can_write_path(std::path::Path::new(&path));
            prop_assert_eq!(d1.allowed, d2.allowed);
        }
    }

    #[test]
    fn prop_policy_deny_read_blocks_match(filename in "[a-z]{1,8}") {
        let pattern = format!("**/{}*", &filename);
        let policy = PolicyProfile {
            deny_read: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_read_path(std::path::Path::new(&filename));
        prop_assert!(!decision.allowed);
    }

    #[test]
    fn prop_policy_deny_write_blocks_match(filename in "[a-z]{1,8}") {
        let pattern = format!("**/{}*", &filename);
        let policy = PolicyProfile {
            deny_write: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_write_path(std::path::Path::new(&filename));
        prop_assert!(!decision.allowed);
    }

    #[test]
    fn prop_policy_serde_roundtrip(policy in arb_policy_profile()) {
        let json = serde_json::to_string(&policy).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&policy.allowed_tools, &rt.allowed_tools);
        prop_assert_eq!(&policy.disallowed_tools, &rt.disallowed_tools);
        prop_assert_eq!(&policy.deny_read, &rt.deny_read);
        prop_assert_eq!(&policy.deny_write, &rt.deny_write);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Glob patterns produce correct decisions
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_glob_empty_allows_everything(path in arb_safe_string()) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }

    #[test]
    fn prop_glob_exclude_denies_matching(ext in "[a-z]{1,4}") {
        let pattern = format!("*.{ext}");
        let globs = IncludeExcludeGlobs::new(&[], &[pattern]).unwrap();
        let filename = format!("file.{ext}");
        prop_assert_eq!(globs.decide_str(&filename), MatchDecision::DeniedByExclude);
    }

    #[test]
    fn prop_glob_include_gates_nonmatching(ext in "[a-z]{2,4}") {
        let pattern = format!("*.{ext}");
        let globs = IncludeExcludeGlobs::new(&[pattern], &[]).unwrap();
        let included = format!("file.{ext}");
        prop_assert_eq!(globs.decide_str(&included), MatchDecision::Allowed);
        prop_assert_eq!(
            globs.decide_str("file.zzzzzzz"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn prop_glob_exclude_beats_include(ext in "[a-z]{2,4}") {
        let inc_pattern = format!("*.{ext}");
        let exc_pattern = inc_pattern.clone();
        let globs = IncludeExcludeGlobs::new(&[inc_pattern], &[exc_pattern]).unwrap();
        let filename = format!("file.{ext}");
        prop_assert_eq!(globs.decide_str(&filename), MatchDecision::DeniedByExclude);
    }

    #[test]
    fn prop_glob_decision_deterministic(
        path in arb_safe_string(),
        includes in prop::collection::vec("[a-z*]{1,6}", 0..3),
        excludes in prop::collection::vec("[a-z*]{1,6}", 0..3)
    ) {
        if let Ok(globs) = IncludeExcludeGlobs::new(&includes, &excludes) {
            let d1 = globs.decide_str(&path);
            let d2 = globs.decide_str(&path);
            prop_assert_eq!(d1, d2);
        }
    }

    #[test]
    fn prop_glob_decide_path_matches_decide_str(path in "[a-zA-Z0-9/_.]{1,20}") {
        let globs = IncludeExcludeGlobs::new(
            &["src/**".to_string()],
            &["src/secret/**".to_string()],
        ).unwrap();
        prop_assert_eq!(
            globs.decide_str(&path),
            globs.decide_path(std::path::Path::new(&path))
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  ErrorCode stability
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_error_code_serde_roundtrip(code in arb_error_code()) {
        let json = serde_json::to_string(&code).unwrap();
        let rt: ErrorCode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(code, rt);
    }

    #[test]
    fn prop_error_code_as_str_stable_across_roundtrip(code in arb_error_code()) {
        let s1 = code.as_str();
        let json = serde_json::to_string(&code).unwrap();
        let rt: ErrorCode = serde_json::from_str(&json).unwrap();
        let s2 = rt.as_str();
        prop_assert_eq!(s1, s2);
    }

    #[test]
    fn prop_error_code_as_str_matches_serde(code in arb_error_code()) {
        let as_str = code.as_str();
        let serde_str = serde_json::to_string(&code).unwrap();
        // serde_json wraps in quotes: "backend_timeout"
        let expected = format!("\"{as_str}\"");
        prop_assert_eq!(serde_str, expected);
    }

    #[test]
    fn prop_error_code_category_deterministic(code in arb_error_code()) {
        let c1 = code.category();
        let c2 = code.category();
        prop_assert_eq!(
            serde_json::to_string(&c1).unwrap(),
            serde_json::to_string(&c2).unwrap()
        );
    }

    #[test]
    fn prop_error_code_message_nonempty(code in arb_error_code()) {
        prop_assert!(!code.message().is_empty());
    }

    #[test]
    fn prop_error_code_as_str_is_snake_case(code in arb_error_code()) {
        let s = code.as_str();
        prop_assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "as_str must be snake_case, got: {s}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Capability negotiation commutativity
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_capability_set_union_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.into_iter().collect();
        let sb: BTreeSet<Capability> = b.into_iter().collect();
        let u1: BTreeSet<Capability> = sa.union(&sb).cloned().collect();
        let u2: BTreeSet<Capability> = sb.union(&sa).cloned().collect();
        prop_assert_eq!(u1, u2);
    }

    #[test]
    fn prop_capability_set_intersection_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.into_iter().collect();
        let sb: BTreeSet<Capability> = b.into_iter().collect();
        let i1: BTreeSet<Capability> = sa.intersection(&sb).cloned().collect();
        let i2: BTreeSet<Capability> = sb.intersection(&sa).cloned().collect();
        prop_assert_eq!(i1, i2);
    }

    #[test]
    fn prop_negotiation_order_independent(
        caps in prop::collection::vec(arb_capability(), 1..6),
        manifest in arb_capability_manifest()
    ) {
        let req1 = NegotiationRequest {
            required: caps.clone(),
            preferred: vec![],
            minimum_support: SupportLevel::Emulated,
        };
        let mut reversed = caps;
        reversed.reverse();
        let req2 = NegotiationRequest {
            required: reversed,
            preferred: vec![],
            minimum_support: SupportLevel::Emulated,
        };
        let r1 = CapabilityNegotiator::negotiate(&req1, &manifest);
        let r2 = CapabilityNegotiator::negotiate(&req2, &manifest);
        prop_assert_eq!(r1.is_compatible, r2.is_compatible);
        // satisfied sets should be the same (order may differ)
        let s1: BTreeSet<_> = r1.satisfied.into_iter().collect();
        let s2: BTreeSet<_> = r2.satisfied.into_iter().collect();
        prop_assert_eq!(s1, s2);
    }

    #[test]
    fn prop_negotiation_deterministic(
        caps in prop::collection::vec(arb_capability(), 1..6),
        manifest in arb_capability_manifest()
    ) {
        let req = NegotiationRequest {
            required: caps,
            preferred: vec![],
            minimum_support: SupportLevel::Emulated,
        };
        let r1 = CapabilityNegotiator::negotiate(&req, &manifest);
        let r2 = CapabilityNegotiator::negotiate(&req, &manifest);
        prop_assert_eq!(r1.is_compatible, r2.is_compatible);
        prop_assert_eq!(r1.satisfied.len(), r2.satisfied.len());
        prop_assert_eq!(r1.unsatisfied.len(), r2.unsatisfied.len());
    }

    #[test]
    fn prop_capability_serde_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cap, rt);
    }

    #[test]
    fn prop_capability_manifest_roundtrip(manifest in arb_capability_manifest()) {
        let json = serde_json::to_string(&manifest).unwrap();
        let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), rt.len());
        for k in rt.keys() {
            prop_assert!(manifest.contains_key(k));
        }
    }

    #[test]
    fn prop_support_level_satisfies_reflexive_native(_unused in 0..1u32) {
        prop_assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
        prop_assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §11  Deterministic serialization (BTreeMap)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_btreemap_insertion_order_irrelevant(
        pairs in prop::collection::vec((arb_safe_string(), arb_safe_string()), 0..10),
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

    #[test]
    fn prop_btreemap_capability_manifest_order_irrelevant(
        pairs in prop::collection::vec((arb_capability(), arb_support_level()), 0..8),
    ) {
        let m1: BTreeMap<Capability, SupportLevel> = pairs.iter().cloned().collect();
        let entries: Vec<_> = m1.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut m2: BTreeMap<Capability, SupportLevel> = BTreeMap::new();
        for (k, v) in entries.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        prop_assert_eq!(
            serde_json::to_string(&m1).unwrap(),
            serde_json::to_string(&m2).unwrap()
        );
    }

    #[test]
    fn prop_btreemap_json_value_order_irrelevant(
        pairs in prop::collection::vec((arb_short_string(), arb_json_value()), 0..8),
    ) {
        let deduped: BTreeMap<String, serde_json::Value> = pairs.iter().cloned().collect();
        let m1 = deduped.clone();
        let mut m2: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for (k, v) in deduped.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        prop_assert_eq!(
            serde_json::to_string(&m1).unwrap(),
            serde_json::to_string(&m2).unwrap()
        );
    }

    #[test]
    fn prop_btreemap_roundtrip_preserves_all_keys(
        pairs in prop::collection::vec((arb_short_string(), arb_safe_string()), 1..10),
    ) {
        let map: BTreeMap<String, String> = pairs.into_iter().collect();
        let json = serde_json::to_string(&map).unwrap();
        let rt: BTreeMap<String, String> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(map, rt);
    }

    #[test]
    fn prop_receipt_serialization_deterministic(r in arb_receipt()) {
        let j1 = serde_json::to_string(&r).unwrap();
        let j2 = serde_json::to_string(&r).unwrap();
        prop_assert_eq!(j1, j2);
    }

    #[test]
    fn prop_work_order_serialization_deterministic(wo in arb_work_order()) {
        let j1 = serde_json::to_string(&wo).unwrap();
        let j2 = serde_json::to_string(&wo).unwrap();
        prop_assert_eq!(j1, j2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §12  Contract version consistency
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn prop_receipt_preserves_contract_version(r in arb_receipt()) {
        prop_assert_eq!(&r.meta.contract_version, CONTRACT_VERSION);
        let json = serde_json::to_string(&r).unwrap();
        prop_assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn prop_envelope_hello_contains_contract_version(env in arb_envelope_hello()) {
        let json = serde_json::to_string(&env).unwrap();
        prop_assert!(json.contains(CONTRACT_VERSION));
    }

    #[test]
    fn prop_receipt_contract_version_survives_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&rt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn prop_outcome_roundtrip(outcome in arb_outcome()) {
        let json = serde_json::to_string(&outcome).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(outcome, rt);
    }

    #[test]
    fn prop_capability_requirements_roundtrip(cr in arb_capability_requirements()) {
        let json = serde_json::to_string(&cr).unwrap();
        let rt: CapabilityRequirements = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cr.required.len(), rt.required.len());
    }
}
