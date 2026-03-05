#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive property-based tests for the Agent Backplane.
//!
//! Covers receipt hashing, JSON roundtrips, policy evaluation, glob matching,
//! envelope encoding, error taxonomy, config merging, and IR mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;
use std::path::Path;

use proptest::prelude::*;
use serde_json::json;

use abp_config::{merge_configs, BackendEntry, BackplaneConfig};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode,
    WorkspaceSpec, CONTRACT_VERSION,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{compute_hash, verify_hash};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 48,
        ..ProptestConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategies – primitives
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

// ═══════════════════════════════════════════════════════════════════════════
// Strategies – enums
// ═══════════════════════════════════════════════════════════════════════════

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
    ]
    .boxed()
}

fn arb_error_code_full() -> BoxedStrategy<ErrorCode> {
    prop_oneof![
        arb_error_code(),
        Just(ErrorCode::ReceiptHashMismatch),
        Just(ErrorCode::ReceiptChainBroken),
        Just(ErrorCode::DialectUnknown),
        Just(ErrorCode::DialectMappingFailed),
        Just(ErrorCode::ConfigInvalid),
        Just(ErrorCode::Internal),
    ]
    .boxed()
}

fn arb_ir_role() -> BoxedStrategy<IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
    .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategies – compound types
// ═══════════════════════════════════════════════════════════════════════════

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

fn arb_envelope_hello() -> BoxedStrategy<Envelope> {
    (
        arb_backend_identity(),
        arb_capability_manifest(),
        arb_execution_mode(),
    )
        .prop_map(|(backend, capabilities, mode)| {
            Envelope::hello_with_mode(backend, capabilities, mode)
        })
        .boxed()
}

fn arb_envelope_run() -> BoxedStrategy<Envelope> {
    (arb_safe_string(), arb_work_order())
        .prop_map(|(id, work_order)| Envelope::Run { id, work_order })
        .boxed()
}

fn arb_envelope_event() -> BoxedStrategy<Envelope> {
    (arb_safe_string(), arb_agent_event())
        .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event })
        .boxed()
}

fn arb_envelope_fatal() -> BoxedStrategy<Envelope> {
    (
        prop::option::of(arb_safe_string()),
        arb_safe_string(),
        prop::option::of(arb_error_code_full()),
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
        arb_envelope_fatal(),
    ]
    .boxed()
}

fn arb_backplane_config() -> BoxedStrategy<BackplaneConfig> {
    (
        prop::option::of(arb_short_string()),
        prop::option::of(arb_safe_string()),
        prop::option::of(prop_oneof![
            Just("error".to_string()),
            Just("warn".to_string()),
            Just("info".to_string()),
            Just("debug".to_string()),
            Just("trace".to_string()),
        ]),
        prop::option::of(arb_safe_string()),
        prop::option::of(1u16..65535u16),
    )
        .prop_map(
            |(default_backend, workspace_dir, log_level, receipts_dir, port)| BackplaneConfig {
                default_backend,
                workspace_dir,
                log_level,
                receipts_dir,
                bind_address: None,
                port,
                policy_profiles: vec![],
                backends: BTreeMap::new(),
            },
        )
        .boxed()
}

fn arb_error_info() -> BoxedStrategy<ErrorInfo> {
    (arb_error_code_full(), arb_safe_string())
        .prop_map(|(code, message)| ErrorInfo::new(code, message))
        .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  Receipt hashing: deterministic, sensitive, self-referential safe
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 1
    #[test]
    fn receipt_hash_is_deterministic(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // 2
    #[test]
    fn receipt_hash_length_64_hex(r in arb_receipt()) {
        let h = compute_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // 3
    #[test]
    fn receipt_hash_ignores_stored_hash(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.receipt_sha256 = Some("aaaa".into());
        prop_assert_eq!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 4
    #[test]
    fn receipt_with_hash_then_verify(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(verify_hash(&hashed));
    }

    // 5
    #[test]
    fn receipt_hash_differs_when_outcome_changes(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 6
    #[test]
    fn receipt_hash_differs_when_backend_id_changes(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_x", r2.backend.id);
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 7
    #[test]
    fn receipt_hash_differs_when_run_id_changes(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.meta.run_id = Uuid::from_u128(r.meta.run_id.as_u128().wrapping_add(1));
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 8
    #[test]
    fn receipt_hash_differs_when_duration_changes(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.meta.duration_ms = r.meta.duration_ms.wrapping_add(1);
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 9
    #[test]
    fn receipt_hash_differs_when_mode_flipped(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.mode = match r.mode {
            ExecutionMode::Passthrough => ExecutionMode::Mapped,
            ExecutionMode::Mapped => ExecutionMode::Passthrough,
        };
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 10
    #[test]
    fn receipt_hash_differs_when_trace_appended(r in arb_receipt(), evt in arb_agent_event()) {
        let mut r2 = r.clone();
        r2.trace.push(evt);
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 11
    #[test]
    fn receipt_hash_differs_when_artifact_added(r in arb_receipt(), art in arb_artifact_ref()) {
        let mut r2 = r.clone();
        r2.artifacts.push(art);
        prop_assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
    }

    // 12
    #[test]
    fn receipt_hash_survives_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(compute_hash(&r).unwrap(), compute_hash(&rt).unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  JSON roundtrip: WorkOrder, Receipt, AgentEvent
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 13
    #[test]
    fn work_order_json_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    // 14
    #[test]
    fn work_order_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
    }

    // 15
    #[test]
    fn receipt_json_roundtrip(r in arb_receipt()) {
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
    fn agent_event_json_roundtrip(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    // 18
    #[test]
    fn agent_event_value_roundtrip(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_value(v).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    // 19
    #[test]
    fn agent_event_has_type_tag(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        prop_assert!(v.get("type").is_some());
    }

    // 20
    #[test]
    fn workspace_spec_json_roundtrip(ws in arb_workspace_spec()) {
        let json = serde_json::to_string(&ws).unwrap();
        let rt: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ws.root, &rt.root);
        prop_assert_eq!(&ws.include, &rt.include);
        prop_assert_eq!(&ws.exclude, &rt.exclude);
    }

    // 21
    #[test]
    fn runtime_config_json_roundtrip(cfg in arb_runtime_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cfg.model, &rt.model);
        prop_assert_eq!(cfg.max_turns, rt.max_turns);
    }

    // 22
    #[test]
    fn backend_identity_json_roundtrip(bi in arb_backend_identity()) {
        let json = serde_json::to_string(&bi).unwrap();
        let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&bi.id, &rt.id);
    }

    // 23
    #[test]
    fn run_metadata_json_roundtrip(rm in arb_run_metadata()) {
        let json = serde_json::to_string(&rm).unwrap();
        let rt: RunMetadata = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(rm.run_id, rt.run_id);
        prop_assert_eq!(rm.duration_ms, rt.duration_ms);
    }

    // 24
    #[test]
    fn usage_normalized_json_roundtrip(u in arb_usage_normalized()) {
        let json = serde_json::to_string(&u).unwrap();
        let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u.input_tokens, rt.input_tokens);
        prop_assert_eq!(u.output_tokens, rt.output_tokens);
    }

    // 25
    #[test]
    fn artifact_ref_json_roundtrip(ar in arb_artifact_ref()) {
        let json = serde_json::to_string(&ar).unwrap();
        let rt: ArtifactRef = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ar.kind, &rt.kind);
        prop_assert_eq!(&ar.path, &rt.path);
    }

    // 26
    #[test]
    fn verification_report_json_roundtrip(vr in arb_verification_report()) {
        let json = serde_json::to_string(&vr).unwrap();
        let rt: VerificationReport = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(vr.harness_ok, rt.harness_ok);
    }

    // 27
    #[test]
    fn context_snippet_json_roundtrip(cs in arb_context_snippet()) {
        let json = serde_json::to_string(&cs).unwrap();
        let rt: ContextSnippet = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cs.name, &rt.name);
        prop_assert_eq!(&cs.content, &rt.content);
    }

    // 28
    #[test]
    fn capability_requirement_json_roundtrip(cr in arb_capability_requirement()) {
        let json = serde_json::to_string(&cr).unwrap();
        let _rt: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    }

    // 29
    #[test]
    fn outcome_json_roundtrip(o in arb_outcome()) {
        let json = serde_json::to_string(&o).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(o, rt);
    }

    // 30
    #[test]
    fn execution_mode_json_roundtrip(m in arb_execution_mode()) {
        let json = serde_json::to_string(&m).unwrap();
        let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(m, rt);
    }

    // 31
    #[test]
    fn capability_json_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cap, rt);
    }

    // 32
    #[test]
    fn capability_manifest_json_roundtrip(manifest in arb_capability_manifest()) {
        let json = serde_json::to_string(&manifest).unwrap();
        let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), rt.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Policy evaluation: consistent, never panics
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 33
    #[test]
    fn policy_default_never_panics(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let _ = engine.can_use_tool(&tool);
    }

    // 34
    #[test]
    fn policy_default_allows_any_tool(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    // 35
    #[test]
    fn policy_with_wildcard_allows_all(tool in arb_short_string()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    // 36
    #[test]
    fn policy_deny_read_blocks(filename in "[a-z]{1,8}") {
        let policy = PolicyProfile {
            deny_read: vec![format!("**/{}*", &filename)],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_read_path(Path::new(&filename)).allowed);
    }

    // 37
    #[test]
    fn policy_deny_write_blocks(filename in "[a-z]{1,8}") {
        let policy = PolicyProfile {
            deny_write: vec![format!("**/{}*", &filename)],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_write_path(Path::new(&filename)).allowed);
    }

    // 38
    #[test]
    fn policy_decision_is_consistent(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d1 = engine.can_use_tool(&tool);
        let d2 = engine.can_use_tool(&tool);
        prop_assert_eq!(d1.allowed, d2.allowed);
    }

    // 39
    #[test]
    fn policy_read_decision_is_consistent(path in "[a-z]{1,12}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d1 = engine.can_read_path(Path::new(&path));
        let d2 = engine.can_read_path(Path::new(&path));
        prop_assert_eq!(d1.allowed, d2.allowed);
    }

    // 40
    #[test]
    fn policy_write_decision_is_consistent(path in "[a-z]{1,12}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let d1 = engine.can_write_path(Path::new(&path));
        let d2 = engine.can_write_path(Path::new(&path));
        prop_assert_eq!(d1.allowed, d2.allowed);
    }

    // 41
    #[test]
    fn policy_glob_patterns_construct(
        allowed in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..4),
        denied in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..4),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..PolicyProfile::default()
        };
        prop_assert!(PolicyEngine::new(&policy).is_ok());
    }

    // 42
    #[test]
    fn policy_deny_path_patterns_construct(
        deny_read in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
        deny_write in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..4),
    ) {
        let policy = PolicyProfile {
            deny_read,
            deny_write,
            ..PolicyProfile::default()
        };
        prop_assert!(PolicyEngine::new(&policy).is_ok());
    }

    // 43
    #[test]
    fn policy_serde_roundtrip(policy in arb_policy_profile()) {
        let json = serde_json::to_string(&policy).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&policy.allowed_tools, &rt.allowed_tools);
        prop_assert_eq!(&policy.disallowed_tools, &rt.disallowed_tools);
    }

    // 44
    #[test]
    fn policy_default_allows_any_read_path(path in "[a-z]{1,8}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
    }

    // 45
    #[test]
    fn policy_default_allows_any_write_path(path in "[a-z]{1,8}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Glob matching: deterministic include/exclude
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 46
    #[test]
    fn glob_universal_include_allows_all(path in "[a-zA-Z0-9_/]{1,20}") {
        let globs = IncludeExcludeGlobs::new(&["**".into()], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }

    // 47
    #[test]
    fn glob_empty_rules_allow_all(path in "[a-zA-Z0-9_/]{1,20}") {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::Allowed);
    }

    // 48
    #[test]
    fn glob_exclude_star_denies_all(path in "[a-zA-Z0-9_]{1,20}") {
        let globs = IncludeExcludeGlobs::new(&[], &["**".into()]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), MatchDecision::DeniedByExclude);
    }

    // 49
    #[test]
    fn glob_decision_is_deterministic(path in "[a-zA-Z0-9_/]{1,20}") {
        let globs = IncludeExcludeGlobs::new(&["**/*.rs".into()], &["**/test*".into()]).unwrap();
        let d1 = globs.decide_str(&path);
        let d2 = globs.decide_str(&path);
        prop_assert_eq!(d1, d2);
    }

    // 50
    #[test]
    fn glob_decide_path_matches_decide_str(name in "[a-z]{1,10}") {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        let str_result = globs.decide_str(&name);
        let path_result = globs.decide_path(Path::new(&name));
        prop_assert_eq!(str_result, path_result);
    }

    // 51
    #[test]
    fn glob_include_requires_match(name in "[a-z]{3,10}") {
        let globs = IncludeExcludeGlobs::new(&["*.rs".into()], &[]).unwrap();
        let decision = globs.decide_str(&name);
        // name without extension won't match *.rs
        prop_assert_eq!(decision, MatchDecision::DeniedByMissingInclude);
    }

    // 52
    #[test]
    fn glob_exclude_overrides_include(name in "[a-z]{1,8}") {
        let full = format!("{}.rs", name);
        let globs = IncludeExcludeGlobs::new(&["*.rs".into()], &["*.rs".into()]).unwrap();
        let decision = globs.decide_str(&full);
        prop_assert_eq!(decision, MatchDecision::DeniedByExclude);
    }

    // 53
    #[test]
    fn glob_is_allowed_consistent(path in "[a-zA-Z0-9_]{1,12}") {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        let d = globs.decide_str(&path);
        prop_assert_eq!(d.is_allowed(), d == MatchDecision::Allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Envelope encoding: JSONL encode/decode roundtrips
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 54
    #[test]
    fn envelope_hello_roundtrip(env in arb_envelope_hello()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(encoded, re_encoded);
    }

    // 55
    #[test]
    fn envelope_run_roundtrip(env in arb_envelope_run()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(encoded, re_encoded);
    }

    // 56
    #[test]
    fn envelope_event_roundtrip(env in arb_envelope_event()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(encoded, re_encoded);
    }

    // 57
    #[test]
    fn envelope_fatal_roundtrip(env in arb_envelope_fatal()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(&encoded).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(encoded, re_encoded);
    }

    // 58
    #[test]
    fn envelope_encode_is_single_line(env in arb_envelope()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let trimmed = encoded.trim_end_matches('\n');
        prop_assert!(!trimmed.contains('\n'), "JSONL line must not contain interior newlines");
    }

    // 59
    #[test]
    fn envelope_encode_is_valid_json(env in arb_envelope()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        prop_assert!(parsed.is_object());
    }

    // 60
    #[test]
    fn envelope_has_t_discriminator(env in arb_envelope()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        prop_assert!(parsed.get("t").is_some(), "envelope must have 't' field");
    }

    // 61
    #[test]
    fn envelope_stream_decode(env in arb_envelope()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let line = format!("{}\n", encoded);
        let reader = BufReader::new(line.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        prop_assert_eq!(results.len(), 1);
        prop_assert!(results[0].is_ok());
    }

    // 62
    #[test]
    fn envelope_multi_stream_roundtrip(
        envs in prop::collection::vec(arb_envelope(), 1..5)
    ) {
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .filter_map(|r| r.ok())
            .collect();
        prop_assert_eq!(envs.len(), decoded.len());
    }

    // 63
    #[test]
    fn envelope_writer_roundtrip(env in arb_envelope()) {
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        let line = String::from_utf8(buf).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        let orig_encoded = JsonlCodec::encode(&env).unwrap();
        prop_assert_eq!(orig_encoded, re_encoded);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Error taxonomy: codes map to valid categories
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 64
    #[test]
    fn error_code_has_valid_category(code in arb_error_code_full()) {
        let cat = code.category();
        // Category must be one of the known variants
        let _ = format!("{}", cat);
    }

    // 65
    #[test]
    fn error_code_as_str_is_nonempty(code in arb_error_code_full()) {
        prop_assert!(!code.as_str().is_empty());
    }

    // 66
    #[test]
    fn error_code_message_is_nonempty(code in arb_error_code_full()) {
        prop_assert!(!code.message().is_empty());
    }

    // 67
    #[test]
    fn error_code_serde_roundtrip(code in arb_error_code_full()) {
        let json = serde_json::to_string(&code).unwrap();
        let rt: ErrorCode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(code, rt);
    }

    // 68
    #[test]
    fn error_code_category_serde_roundtrip(code in arb_error_code_full()) {
        let cat = code.category();
        let json = serde_json::to_string(&cat).unwrap();
        let rt: ErrorCategory = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cat, rt);
    }

    // 69
    #[test]
    fn error_code_category_deterministic(code in arb_error_code_full()) {
        let c1 = code.category();
        let c2 = code.category();
        prop_assert_eq!(c1, c2);
    }

    // 70
    #[test]
    fn error_code_retryable_is_deterministic(code in arb_error_code_full()) {
        prop_assert_eq!(code.is_retryable(), code.is_retryable());
    }

    // 71
    #[test]
    fn error_info_serde_roundtrip(info in arb_error_info()) {
        let json = serde_json::to_string(&info).unwrap();
        let rt: ErrorInfo = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(info.code, rt.code);
        prop_assert_eq!(&info.message, &rt.message);
    }

    // 72
    #[test]
    fn error_info_retryable_matches_code(info in arb_error_info()) {
        prop_assert_eq!(info.is_retryable, info.code.is_retryable());
    }

    // 73
    #[test]
    fn abp_error_category_matches_code(code in arb_error_code_full(), msg in arb_safe_string()) {
        let err = AbpError::new(code, msg);
        prop_assert_eq!(err.category(), code.category());
    }

    // 74
    #[test]
    fn error_code_as_str_is_snake_case(code in arb_error_code_full()) {
        let s = code.as_str();
        prop_assert!(s.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Config: merge idempotency and properties
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 75
    #[test]
    fn config_merge_with_default_is_identity(cfg in arb_backplane_config()) {
        let merged = merge_configs(cfg.clone(), BackplaneConfig::default());
        prop_assert_eq!(&cfg.default_backend, &merged.default_backend);
        prop_assert_eq!(&cfg.workspace_dir, &merged.workspace_dir);
        prop_assert_eq!(&cfg.receipts_dir, &merged.receipts_dir);
        prop_assert_eq!(cfg.port, merged.port);
    }

    // 76
    #[test]
    fn config_merge_idempotent(cfg in arb_backplane_config()) {
        let once = merge_configs(BackplaneConfig::default(), cfg.clone());
        let twice = merge_configs(BackplaneConfig::default(), cfg.clone());
        prop_assert_eq!(once, twice);
    }

    // 77
    #[test]
    fn config_overlay_wins_for_scalars(
        base in arb_backplane_config(),
        overlay in arb_backplane_config()
    ) {
        let merged = merge_configs(base.clone(), overlay.clone());
        // overlay fields take precedence when Some
        if overlay.default_backend.is_some() {
            prop_assert_eq!(&merged.default_backend, &overlay.default_backend);
        }
        if overlay.port.is_some() {
            prop_assert_eq!(merged.port, overlay.port);
        }
        if overlay.log_level.is_some() {
            prop_assert_eq!(&merged.log_level, &overlay.log_level);
        }
    }

    // 78
    #[test]
    fn config_merge_base_preserved_when_overlay_none(base in arb_backplane_config()) {
        let overlay = BackplaneConfig {
            default_backend: None,
            workspace_dir: None,
            log_level: None,
            receipts_dir: None,
            bind_address: None,
            port: None,
            policy_profiles: vec![],
            backends: BTreeMap::new(),
        };
        let merged = merge_configs(base.clone(), overlay);
        prop_assert_eq!(&base.default_backend, &merged.default_backend);
        prop_assert_eq!(&base.workspace_dir, &merged.workspace_dir);
        prop_assert_eq!(&base.receipts_dir, &merged.receipts_dir);
    }

    // 79
    #[test]
    fn config_serde_json_roundtrip(cfg in arb_backplane_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cfg, rt);
    }

    // 80
    #[test]
    fn config_backends_merge_combines(
        name1 in arb_short_string(),
        name2 in arb_short_string(),
    ) {
        let mut base = BackplaneConfig::default();
        base.backends.insert(name1.clone(), BackendEntry::Mock {});
        let mut overlay = BackplaneConfig::default();
        overlay.backends.insert(name2.clone(), BackendEntry::Mock {});
        let merged = merge_configs(base, overlay);
        prop_assert!(merged.backends.contains_key(&name1) || merged.backends.contains_key(&name2));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  IR mapping: identity roundtrip preserves fields
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 81
    #[test]
    fn ir_role_serde_roundtrip(role in arb_ir_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let rt: IrRole = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(role, rt);
    }

    // 82
    #[test]
    fn ir_content_block_serde_roundtrip(block in arb_ir_content_block()) {
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, rt);
    }

    // 83
    #[test]
    fn ir_message_serde_roundtrip(msg in arb_ir_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let rt: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(msg, rt);
    }

    // 84
    #[test]
    fn ir_conversation_serde_roundtrip(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv, rt);
    }

    // 85
    #[test]
    fn ir_conversation_length_preserved(conv in arb_ir_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let rt: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(conv.len(), rt.len());
    }

    // 86
    #[test]
    fn ir_tool_definition_serde_roundtrip(td in arb_ir_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let rt: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(td, rt);
    }

    // 87
    #[test]
    fn ir_usage_serde_roundtrip(u in arb_ir_usage()) {
        let json = serde_json::to_string(&u).unwrap();
        let rt: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u, rt);
    }

    // 88
    #[test]
    fn ir_usage_merge_commutative(a in arb_ir_usage(), b in arb_ir_usage()) {
        prop_assert_eq!(a.merge(b), b.merge(a));
    }

    // 89
    #[test]
    fn ir_usage_from_io_total(inp in 0u64..100_000, out in 0u64..100_000) {
        let u = IrUsage::from_io(inp, out);
        prop_assert_eq!(u.total_tokens, inp + out);
    }

    // 90
    #[test]
    fn ir_message_text_preserves_role(role in arb_ir_role(), text in arb_safe_string()) {
        let msg = IrMessage::text(role, &text);
        prop_assert_eq!(msg.role, role);
    }

    // 91
    #[test]
    fn ir_message_text_only_flag(text in arb_safe_string()) {
        let msg = IrMessage::text(IrRole::User, &text);
        prop_assert!(msg.is_text_only());
        prop_assert_eq!(msg.text_content(), text);
    }

    // 92
    #[test]
    fn ir_conversation_value_roundtrip(conv in arb_ir_conversation()) {
        let v = serde_json::to_value(&conv).unwrap();
        let rt: IrConversation = serde_json::from_value(v).unwrap();
        prop_assert_eq!(conv, rt);
    }

    // 93
    #[test]
    fn ir_conversation_push_increases_len(
        conv in arb_ir_conversation(),
        msg in arb_ir_message()
    ) {
        let original_len = conv.len();
        let extended = conv.push(msg);
        prop_assert_eq!(extended.len(), original_len + 1);
    }

    // 94
    #[test]
    fn ir_conversation_from_messages_preserves_count(msgs in prop::collection::vec(arb_ir_message(), 1..6)) {
        let count = msgs.len();
        let conv = IrConversation::from_messages(msgs);
        prop_assert_eq!(conv.len(), count);
    }

    // 95
    #[test]
    fn ir_usage_merge_with_zero_is_identity(u in arb_ir_usage()) {
        let zero = IrUsage::default();
        prop_assert_eq!(u.merge(zero), u);
    }

    // 96
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
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  BTreeMap determinism & capability set algebra
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 97
    #[test]
    fn btreemap_insertion_order_irrelevant(
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

    // 98
    #[test]
    fn capability_set_union_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let u1: BTreeSet<_> = sa.union(&sb).cloned().collect();
        let u2: BTreeSet<_> = sb.union(&sa).cloned().collect();
        prop_assert_eq!(u1, u2);
    }

    // 99
    #[test]
    fn capability_set_intersection_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let i1: BTreeSet<_> = sa.intersection(&sb).cloned().collect();
        let i2: BTreeSet<_> = sb.intersection(&sa).cloned().collect();
        prop_assert_eq!(i1, i2);
    }

    // 100
    #[test]
    fn capability_set_inclusion_exclusion(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.iter().cloned().collect();
        let sb: BTreeSet<Capability> = b.iter().cloned().collect();
        let union: BTreeSet<_> = sa.union(&sb).cloned().collect();
        let inter: BTreeSet<_> = sa.intersection(&sb).cloned().collect();
        prop_assert_eq!(union.len(), sa.len() + sb.len() - inter.len());
    }
}
