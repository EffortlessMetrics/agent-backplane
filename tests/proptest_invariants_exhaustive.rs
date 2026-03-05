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
//! Exhaustive property-based tests for core ABP invariants.
//!
//! 50+ property tests organized into 5 categories:
//!
//! 1. **Receipt hashing invariants** (10) — determinism, sensitivity, self-referential prevention
//! 2. **Serde roundtrip invariants** (10) — WorkOrder, Receipt, AgentEvent, PolicyProfile, Envelope
//! 3. **Policy invariants** (10) — deny overrides, empty allows all, allow-* permits all
//! 4. **Mapping invariants** (10) — identity preservation, chain validation, fidelity
//! 5. **Capability invariants** (10) — negotiation commutativity, registry, emulation plans

use std::collections::BTreeMap;
use std::path::Path;

use proptest::prelude::*;
use serde_json::json;

use abp_capability::{
    check_capability, generate_report, negotiate, negotiate_capabilities, negotiate_dialects,
    CapabilityRegistry, CompatibilityReport, NegotiationResult,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    canonical_json, receipt_hash, AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity,
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_dialect::Dialect;
use abp_emulation::{
    can_emulate, default_strategy, EmulationConfig, EmulationEngine,
    EmulationStrategy as EmuStrategy,
};
use abp_mapper::{
    default_ir_mapper, supported_ir_pairs, DialectRequest, IdentityMapper, IrIdentityMapper,
    IrMapper, Mapper,
};
use abp_mapping::{
    features, known_rules, validate_mapping, Fidelity, MappingRegistry, MappingRule,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{compute_hash, verify_hash};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Config
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategies — primitives
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

// ═══════════════════════════════════════════════════════════════════════════
// Strategies — enums
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
    prop_oneof![
        Just(MinSupport::Native),
        Just(MinSupport::Emulated),
        Just(MinSupport::Any),
    ]
    .boxed()
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

fn arb_known_feature() -> BoxedStrategy<String> {
    prop_oneof![
        Just(features::TOOL_USE.to_owned()),
        Just(features::STREAMING.to_owned()),
        Just(features::THINKING.to_owned()),
        Just(features::IMAGE_INPUT.to_owned()),
        Just(features::CODE_EXEC.to_owned()),
    ]
    .boxed()
}

fn arb_tool_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_]{0,14}".boxed()
}

fn arb_deny_glob() -> BoxedStrategy<String> {
    prop_oneof![
        Just("**/.git/**".to_owned()),
        Just("**/.env".to_owned()),
        Just("secret*".to_owned()),
        Just("**/node_modules/**".to_owned()),
    ]
    .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategies — compound types
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
    (prop::option::of(arb_short_string()), arb_safe_string())
        .prop_map(|(ref_id, error)| Envelope::Fatal {
            ref_id,
            error,
            error_code: None,
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
// §1  Receipt hashing invariants (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// RH-1: hash(r) == hash(r) — deterministic for the same receipt.
    #[test]
    fn rh01_hash_deterministic(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        prop_assert_eq!(&h1, &h2, "same receipt must produce identical hashes");
    }

    /// RH-2: hash(r1) != hash(r2) when backend id differs.
    #[test]
    fn rh02_hash_sensitive_to_backend(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_x", r2.backend.id);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// RH-3: hash excludes receipt_sha256 field (self-referential prevention).
    #[test]
    fn rh03_hash_excludes_receipt_sha256(r in arb_receipt()) {
        let h_before = compute_hash(&r).unwrap();
        let mut with_sha = r.clone();
        with_sha.receipt_sha256 = Some("deadbeef".repeat(8));
        let h_after = compute_hash(&with_sha).unwrap();
        prop_assert_eq!(h_before, h_after, "receipt_sha256 must not affect hash");
    }

    /// RH-4: hash is a hex string of exactly length 64.
    #[test]
    fn rh04_hash_is_64_char_hex(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        let sha = hashed.receipt_sha256.as_ref().unwrap();
        prop_assert_eq!(sha.len(), 64, "SHA-256 hex must be 64 chars");
        prop_assert!(sha.chars().all(|c| c.is_ascii_hexdigit()), "must be hex only");
    }

    /// RH-5: hash changes when outcome differs.
    #[test]
    fn rh05_hash_sensitive_to_outcome(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// RH-6: hash changes when run_id differs.
    #[test]
    fn rh06_hash_sensitive_to_run_id(r in arb_receipt()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.run_id = Uuid::from_u128(r.meta.run_id.as_u128().wrapping_add(1));
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// RH-7: hash changes when trace is appended.
    #[test]
    fn rh07_hash_sensitive_to_trace(r in arb_receipt(), evt in arb_agent_event()) {
        let h_orig = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.trace.push(evt);
        prop_assert_ne!(h_orig, compute_hash(&r2).unwrap());
    }

    /// RH-8: with_hash() → verify_hash() always passes.
    #[test]
    fn rh08_with_hash_then_verify(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(verify_hash(&hashed));
    }

    /// RH-9: hash is idempotent — clear and rehash produces same result.
    #[test]
    fn rh09_hash_idempotent_cycle(r in arb_receipt()) {
        let hashed1 = r.clone().with_hash().unwrap();
        let hash1 = hashed1.receipt_sha256.clone().unwrap();
        let mut cleared = hashed1;
        cleared.receipt_sha256 = None;
        let hashed2 = cleared.with_hash().unwrap();
        prop_assert_eq!(hash1, hashed2.receipt_sha256.unwrap());
    }

    /// RH-10: hash changes when execution mode differs.
    #[test]
    fn rh10_hash_sensitive_to_mode(r in arb_receipt()) {
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
// §2  Serde roundtrip invariants (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// SR-1: WorkOrder JSON roundtrip is identity (via double-serialization).
    #[test]
    fn sr01_work_order_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(&json, &json2, "double-serialization must be identical");
    }

    /// SR-2: Receipt JSON roundtrip is identity.
    #[test]
    fn sr02_receipt_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(&json, &json2);
    }

    /// SR-3: AgentEvent JSON roundtrip preserves timestamps.
    #[test]
    fn sr03_agent_event_roundtrip(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
        let json2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(&json, &json2);
    }

    /// SR-4: PolicyProfile JSON roundtrip is identity.
    #[test]
    fn sr04_policy_profile_roundtrip(p in arb_policy_profile()) {
        let json = serde_json::to_string(&p).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&p.allowed_tools, &rt.allowed_tools);
        prop_assert_eq!(&p.disallowed_tools, &rt.disallowed_tools);
        prop_assert_eq!(&p.deny_read, &rt.deny_read);
        prop_assert_eq!(&p.deny_write, &rt.deny_write);
    }

    /// SR-5: Envelope JSON roundtrip is identity (via re-encoding).
    #[test]
    fn sr05_envelope_roundtrip(envelope in arb_envelope()) {
        let json1 = serde_json::to_string(&envelope).unwrap();
        let parsed: Envelope = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        prop_assert_eq!(&json1, &json2);
    }

    /// SR-6: Envelope JSONL encode/decode roundtrip.
    #[test]
    fn sr06_envelope_jsonl_codec_roundtrip(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(&encoded, &re_encoded);
    }

    /// SR-7: AgentEvent with extension data roundtrips.
    #[test]
    fn sr07_agent_event_ext_roundtrip(evt in arb_agent_event_with_ext()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&evt.ext, &rt.ext);
    }

    /// SR-8: WorkOrder preserves task field exactly.
    #[test]
    fn sr08_work_order_task_preserved(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.task, &rt.task);
    }

    /// SR-9: Capability manifest BTreeMap ordering survives roundtrip.
    #[test]
    fn sr09_capability_manifest_ordering(entries in arb_capability_manifest()) {
        let json = serde_json::to_string(&entries).unwrap();
        let parsed: CapabilityManifest = serde_json::from_str(&json).unwrap();
        let keys_orig: Vec<_> = entries.keys().collect();
        let keys_parsed: Vec<_> = parsed.keys().collect();
        prop_assert_eq!(keys_orig, keys_parsed, "BTreeMap key order must survive roundtrip");
    }

    /// SR-10: Fidelity enum roundtrip (Lossless, LossyLabeled, Unsupported).
    #[test]
    fn sr10_fidelity_roundtrip(
        f in prop_oneof![
            Just(Fidelity::Lossless),
            "[a-z ]{1,20}".prop_map(|w| Fidelity::LossyLabeled { warning: w }),
            "[a-z ]{1,20}".prop_map(|r| Fidelity::Unsupported { reason: r }),
        ]
    ) {
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(f, f2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Policy invariants (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// PO-1: Default policy allows all tools.
    #[test]
    fn po01_default_allows_tools(tool in arb_tool_name()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    /// PO-2: Default policy allows all read paths.
    #[test]
    fn po02_default_allows_reads(path in "[a-z/]{1,30}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
    }

    /// PO-3: Default policy allows all write paths.
    #[test]
    fn po03_default_allows_writes(path in "[a-z/]{1,30}") {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }

    /// PO-4: Deny overrides allow — a tool on both allow and deny lists is denied.
    #[test]
    fn po04_deny_overrides_allow(tool in arb_tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![tool.clone()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }

    /// PO-5: Allow-* with no deny permits all tools.
    #[test]
    fn po05_allow_star_permits_all(tool in arb_tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    /// PO-6: Denied read paths are enforced.
    #[test]
    fn po06_deny_read_enforced(glob in arb_deny_glob()) {
        let policy = PolicyProfile {
            deny_read: vec![glob.clone()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let test_path = match glob.as_str() {
            "**/.git/**" => ".git/config",
            "**/.env" => ".env",
            "secret*" => "secret.txt",
            "**/node_modules/**" => "node_modules/pkg/index.js",
            _ => return Ok(()),
        };
        prop_assert!(!engine.can_read_path(Path::new(test_path)).allowed);
    }

    /// PO-7: Denied write paths are enforced.
    #[test]
    fn po07_deny_write_enforced(glob in arb_deny_glob()) {
        let policy = PolicyProfile {
            deny_write: vec![glob.clone()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let test_path = match glob.as_str() {
            "**/.git/**" => ".git/config",
            "**/.env" => ".env",
            "secret*" => "secret.txt",
            "**/node_modules/**" => "node_modules/pkg/index.js",
            _ => return Ok(()),
        };
        prop_assert!(!engine.can_write_path(Path::new(test_path)).allowed);
    }

    /// PO-8: Policy compilation never panics on valid tool name inputs.
    #[test]
    fn po08_compilation_no_panic(
        allowed in prop::collection::vec(arb_tool_name(), 0..4),
        denied in prop::collection::vec(arb_tool_name(), 0..4),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..Default::default()
        };
        let result = PolicyEngine::new(&policy);
        prop_assert!(result.is_ok());
    }

    /// PO-9: Policy decisions are deterministic — same inputs produce same results.
    #[test]
    fn po09_deterministic(tool in arb_tool_name()) {
        let policy = PolicyProfile {
            disallowed_tools: vec!["bash".into()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let d1 = engine.can_use_tool(&tool);
        let d2 = engine.can_use_tool(&tool);
        prop_assert_eq!(d1.allowed, d2.allowed);
    }

    /// PO-10: Deny list + allow list intersection: tool in deny always denied
    /// regardless of whether it matches allow list.
    #[test]
    fn po10_deny_trumps_allow_intersection(tool in arb_tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec![tool.clone()],
            disallowed_tools: vec![tool.clone()],
            ..Default::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Mapping invariants (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// MA-1: Identity mapping preserves all fields (IrIdentityMapper).
    #[test]
    fn ma01_identity_ir_mapper_preserves(conv in arb_ir_conversation(), d in arb_dialect()) {
        let mapper = IrIdentityMapper;
        let mapped = mapper.map_request(d, d, &conv).unwrap();
        prop_assert_eq!(conv.messages.len(), mapped.messages.len());
        for (orig, m) in conv.messages.iter().zip(mapped.messages.iter()) {
            prop_assert_eq!(orig.role, m.role);
            prop_assert_eq!(orig.content.len(), m.content.len());
        }
    }

    /// MA-2: Identity JSON mapper preserves request body.
    #[test]
    fn ma02_identity_json_mapper_preserves(body in arb_json_value(), d in arb_dialect()) {
        let mapper = IdentityMapper;
        let req = DialectRequest { dialect: d, body: body.clone() };
        let mapped = mapper.map_request(&req).unwrap();
        prop_assert_eq!(body, mapped);
    }

    /// MA-3: Self-mapping is always Lossless in known_rules().
    #[test]
    fn ma03_self_mapping_lossless(d in arb_dialect(), f in arb_known_feature()) {
        let reg = known_rules();
        let rule = reg.lookup(d, d, &f);
        prop_assert!(rule.is_some(), "self-mapping must exist for {:?}/{}", d, f);
        prop_assert!(rule.unwrap().fidelity.is_lossless(), "self-mapping must be lossless");
    }

    /// MA-4: Mapping registry lookup is deterministic.
    #[test]
    fn ma04_lookup_deterministic(src in arb_dialect(), tgt in arb_dialect(), f in arb_known_feature()) {
        let reg = known_rules();
        let r1 = reg.lookup(src, tgt, &f).map(|r| r.fidelity.clone());
        let r2 = reg.lookup(src, tgt, &f).map(|r| r.fidelity.clone());
        prop_assert_eq!(r1, r2);
    }

    /// MA-5: validate_mapping covers all features (one result per feature).
    #[test]
    fn ma05_validate_covers_all_features(
        src in arb_dialect(),
        tgt in arb_dialect(),
        feats in prop::collection::vec(arb_known_feature(), 1..6),
    ) {
        let reg = known_rules();
        let results = validate_mapping(&reg, src, tgt, &feats);
        prop_assert_eq!(results.len(), feats.len());
        for (result, feat) in results.iter().zip(&feats) {
            prop_assert_eq!(&result.feature, feat);
        }
    }

    /// MA-6: Inserted rules are retrievable via lookup.
    #[test]
    fn ma06_insert_then_lookup(src in arb_dialect(), tgt in arb_dialect(), feat in arb_known_feature()) {
        let mut reg = MappingRegistry::new();
        let rule = MappingRule {
            source_dialect: src,
            target_dialect: tgt,
            feature: feat.clone(),
            fidelity: Fidelity::Lossless,
        };
        reg.insert(rule.clone());
        let found = reg.lookup(src, tgt, &feat);
        prop_assert!(found.is_some());
        prop_assert_eq!(found.unwrap(), &rule);
    }

    /// MA-7: Chain A→A is always Lossless for known features.
    #[test]
    fn ma07_identity_chain_lossless(d in arb_dialect(), f in arb_known_feature()) {
        let reg = known_rules();
        let cv = reg.validate_chain(&[d, d], &f);
        prop_assert!(cv.overall_fidelity.is_lossless(),
            "identity chain {:?}→{:?} must be lossless for {}", d, d, f);
    }

    /// MA-8: default_ir_mapper returns Some for all supported pairs.
    #[test]
    fn ma08_supported_pairs_all_resolve(idx in 0..supported_ir_pairs().len()) {
        let pairs = supported_ir_pairs();
        let (from, to) = pairs[idx];
        let mapper = default_ir_mapper(from, to);
        prop_assert!(mapper.is_some(), "pair {:?}→{:?} must have a mapper", from, to);
    }

    /// MA-9: Identity mapper supported_pairs includes all same-dialect pairs.
    #[test]
    fn ma09_identity_mapper_covers_all_dialects(d in arb_dialect()) {
        let mapper = IrIdentityMapper;
        let pairs = mapper.supported_pairs();
        prop_assert!(pairs.contains(&(d, d)),
            "identity mapper must support {:?}→{:?}", d, d);
    }

    /// MA-10: Bidirectional report for self-mapping has both forward and reverse lossless.
    #[test]
    fn ma10_bidirectional_self_lossless(d in arb_dialect(), f in arb_known_feature()) {
        let reg = known_rules();
        let report = reg.validate_bidirectional(d, d, &f);
        prop_assert!(report.forward_fidelity.as_ref().map_or(false, |f| f.is_lossless()));
        prop_assert!(report.reverse_fidelity.as_ref().map_or(false, |f| f.is_lossless()));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Capability invariants (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// CA-1: Negotiation is deterministic.
    #[test]
    fn ca01_negotiation_deterministic(
        manifest in arb_capability_manifest(),
        reqs in arb_capability_requirements(),
    ) {
        let r1 = negotiate(&manifest, &reqs);
        let r2 = negotiate(&manifest, &reqs);
        prop_assert_eq!(r1, r2);
    }

    /// CA-2: Negotiation total matches requirement count.
    #[test]
    fn ca02_total_matches_reqs(
        manifest in arb_capability_manifest(),
        reqs in arb_capability_requirements(),
    ) {
        let result = negotiate(&manifest, &reqs);
        prop_assert_eq!(result.total(), reqs.required.len());
    }

    /// CA-3: Empty requirements always yield compatible result.
    #[test]
    fn ca03_empty_reqs_compatible(manifest in arb_capability_manifest()) {
        let reqs = CapabilityRequirements::default();
        let result = negotiate(&manifest, &reqs);
        prop_assert!(result.is_compatible());
    }

    /// CA-4: Adding caps to manifest never increases unsupported count.
    #[test]
    fn ca04_more_caps_never_reduces_compat(
        base_manifest in arb_capability_manifest(),
        extra_cap in arb_capability(),
        reqs in arb_capability_requirements(),
    ) {
        let base_result = negotiate(&base_manifest, &reqs);
        let mut expanded = base_manifest.clone();
        expanded.insert(extra_cap, SupportLevel::Native);
        let expanded_result = negotiate(&expanded, &reqs);
        prop_assert!(
            expanded_result.unsupported.len() <= base_result.unsupported.len(),
            "adding caps should never increase unsupported count"
        );
    }

    /// CA-5: Registered dialect in default registry has non-empty capabilities.
    #[test]
    fn ca05_registry_defaults_non_empty(_idx in 0..6u32) {
        let reg = CapabilityRegistry::with_defaults();
        for name in reg.names() {
            let manifest = reg.get(name).unwrap();
            prop_assert!(!manifest.is_empty(),
                "registered dialect '{}' must have non-empty capabilities", name);
        }
    }

    /// CA-6: Removing requirements never increases unsupported count.
    #[test]
    fn ca06_fewer_reqs_never_reduces_compat(
        manifest in arb_capability_manifest(),
        reqs in arb_capability_requirements(),
    ) {
        let full_result = negotiate(&manifest, &reqs);
        let mut fewer = reqs.clone();
        if !fewer.required.is_empty() {
            fewer.required.pop();
        }
        let fewer_result = negotiate(&manifest, &fewer);
        prop_assert!(
            fewer_result.unsupported.len() <= full_result.unsupported.len(),
            "removing requirements should never increase unsupported count"
        );
    }

    /// CA-7: Native satisfies both Native and Emulated minimum.
    #[test]
    fn ca07_native_satisfies_any_min(_i in 0..10u32) {
        let native = SupportLevel::Native;
        prop_assert!(native.satisfies(&MinSupport::Native));
        prop_assert!(native.satisfies(&MinSupport::Emulated));
        prop_assert!(native.satisfies(&MinSupport::Any));
    }

    /// CA-8: Unsupported never satisfies Native or Emulated minimum.
    #[test]
    fn ca08_unsupported_never_satisfies(
        min in prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)]
    ) {
        let unsupported = SupportLevel::Unsupported;
        prop_assert!(!unsupported.satisfies(&min));
    }

    /// CA-9: Emulation plan — can_emulate covers known emulatable capabilities.
    #[test]
    fn ca09_known_emulatable_caps(_i in 0..5u32) {
        // ExtendedThinking, StructuredOutputJsonSchema, ImageInput, StopSequences
        // are known to be emulatable
        prop_assert!(can_emulate(&Capability::ExtendedThinking));
        prop_assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
        prop_assert!(can_emulate(&Capability::ImageInput));
        prop_assert!(can_emulate(&Capability::StopSequences));
        // CodeExecution is known to be disabled
        prop_assert!(!can_emulate(&Capability::CodeExecution));
    }

    /// CA-10: EmulationEngine check_missing for emulatable caps produces no warnings.
    #[test]
    fn ca10_emulation_engine_check_emulatable(_i in 0..5u32) {
        let engine = EmulationEngine::with_defaults();
        let emulatable = vec![Capability::ExtendedThinking, Capability::ImageInput];
        let report = engine.check_missing(&emulatable);
        prop_assert!(!report.has_unemulatable(),
            "emulatable capabilities should have no unemulatable warnings");
        prop_assert_eq!(report.applied.len(), emulatable.len());
    }
}
