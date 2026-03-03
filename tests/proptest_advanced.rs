// SPDX-License-Identifier: MIT OR Apache-2.0
//! Advanced property-based tests using proptest.
//!
//! Categories:
//! 1. WorkOrder serializes to valid JSON and back
//! 2. Receipt hashes deterministically
//! 3. AgentEvent survives serde roundtrip
//! 4. Random Envelope sequences decode correctly
//! 5. Random glob patterns compile without panic
//! 6. Random policy profiles compile without panic
//! 7. Random capability sets are ordered deterministically
//! 8. Parallel serialization produces same output
//! 9. Large random values don't panic (stress properties)
//! 10. Contract version is always preserved through serde

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use serde_json::json;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_glob::IncludeExcludeGlobs;
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{compute_hash, verify_hash};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    }
}

fn stress_config() -> ProptestConfig {
    ProptestConfig {
        cases: 20,
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

// -- Compound types -------------------------------------------------------

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

fn arb_envelope() -> BoxedStrategy<Envelope> {
    prop_oneof![
        (arb_backend_identity(), arb_capability_manifest())
            .prop_map(|(backend, caps)| { Envelope::hello(backend, caps) }),
        (arb_short_string(), arb_work_order())
            .prop_map(|(id, wo)| Envelope::Run { id, work_order: wo }),
        (arb_short_string(), arb_agent_event())
            .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event }),
        (arb_short_string(), arb_receipt())
            .prop_map(|(ref_id, receipt)| Envelope::Final { ref_id, receipt }),
        (prop::option::of(arb_short_string()), arb_safe_string()).prop_map(|(ref_id, error)| {
            Envelope::Fatal {
                ref_id,
                error,
                error_code: None,
            }
        }),
    ]
    .boxed()
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  Any valid WorkOrder serializes to valid JSON and back
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 1
    #[test]
    fn wo_json_roundtrip_full(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    // 2
    #[test]
    fn wo_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
    }

    // 3
    #[test]
    fn wo_json_is_valid_object(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        prop_assert!(v.is_object());
    }

    // 4
    #[test]
    fn wo_preserves_all_context_files(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.context.files, &rt.context.files);
        prop_assert_eq!(wo.context.snippets.len(), rt.context.snippets.len());
    }

    // 5
    #[test]
    fn wo_preserves_workspace_spec(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.workspace.root, &rt.workspace.root);
        prop_assert_eq!(&wo.workspace.include, &rt.workspace.include);
        prop_assert_eq!(&wo.workspace.exclude, &rt.workspace.exclude);
    }

    // 6
    #[test]
    fn wo_preserves_policy(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.policy.allowed_tools, &rt.policy.allowed_tools);
        prop_assert_eq!(&wo.policy.disallowed_tools, &rt.policy.disallowed_tools);
        prop_assert_eq!(&wo.policy.deny_read, &rt.policy.deny_read);
        prop_assert_eq!(&wo.policy.deny_write, &rt.policy.deny_write);
    }

    // 7
    #[test]
    fn wo_preserves_requirements(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.requirements.required.len(), rt.requirements.required.len());
    }

    // 8
    #[test]
    fn wo_preserves_config(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&wo.config.model, &rt.config.model);
        prop_assert_eq!(wo.config.max_turns, rt.config.max_turns);
    }

    // 9
    #[test]
    fn wo_double_roundtrip_stable(wo in arb_work_order()) {
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
// §2  Any valid Receipt hashes deterministically
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 10
    #[test]
    fn receipt_hash_deterministic_twice(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // 11
    #[test]
    fn receipt_hash_is_64_hex_chars(r in arb_receipt()) {
        let h = compute_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // 12
    #[test]
    fn receipt_hash_ignores_stored_value(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.receipt_sha256 = Some("aaaa".repeat(16));
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // 13
    #[test]
    fn receipt_with_hash_verifies_ok(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(hashed.receipt_sha256.is_some());
        prop_assert!(verify_hash(&hashed));
    }

    // 14
    #[test]
    fn receipt_hash_survives_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        let h_orig = compute_hash(&r).unwrap();
        let h_rt = compute_hash(&rt).unwrap();
        prop_assert_eq!(h_orig, h_rt);
    }

    // 15
    #[test]
    fn receipt_hash_sensitive_to_outcome(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 16
    #[test]
    fn receipt_hash_sensitive_to_backend_id(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_x", r2.backend.id);
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 17
    #[test]
    fn receipt_hash_sensitive_to_mode(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.mode = match r.mode {
            ExecutionMode::Passthrough => ExecutionMode::Mapped,
            ExecutionMode::Mapped => ExecutionMode::Passthrough,
        };
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 18
    #[test]
    fn receipt_hash_triple_deterministic(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        let h3 = compute_hash(&r).unwrap();
        prop_assert_eq!(&h1, &h2);
        prop_assert_eq!(&h2, &h3);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Any valid AgentEvent survives serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 19
    #[test]
    fn agent_event_json_roundtrip(evt in arb_agent_event()) {
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
    fn agent_event_has_type_tag(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        prop_assert!(v.get("type").is_some());
    }

    // 22
    #[test]
    fn agent_event_double_roundtrip(evt in arb_agent_event()) {
        let j1 = serde_json::to_string(&evt).unwrap();
        let rt1: AgentEvent = serde_json::from_str(&j1).unwrap();
        let j2 = serde_json::to_string(&rt1).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 23
    #[test]
    fn agent_event_ts_preserved(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts.timestamp(), rt.ts.timestamp());
    }

    // 24
    #[test]
    fn agent_event_ext_none_omitted(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        // ext is None, so skip_serializing_if should omit it
        prop_assert!(v.get("ext").is_none());
    }

    // 25
    #[test]
    fn agent_event_with_ext_roundtrip(evt_base in arb_agent_event(), key in arb_short_string()) {
        let mut ext = BTreeMap::new();
        ext.insert(key.clone(), json!("test_value"));
        let evt = AgentEvent {
            ts: evt_base.ts,
            kind: evt_base.kind,
            ext: Some(ext),
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert!(rt.ext.is_some());
        prop_assert!(rt.ext.unwrap().contains_key(&key));
    }

    // 26
    #[test]
    fn agent_event_run_started_msg_preserved(msg in arb_safe_string()) {
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

    // 27
    #[test]
    fn agent_event_assistant_message_preserved(text in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantMessage { text: t } = &rt.kind {
            prop_assert_eq!(&text, t);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Random Envelope sequences decode correctly
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 28
    #[test]
    fn envelope_single_encode_decode(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        prop_assert!(line.ends_with('\n'));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        // verify tag preserved
        let orig_tag = serde_json::to_value(&env).unwrap()["t"].clone();
        let rt_tag = serde_json::to_value(&decoded).unwrap()["t"].clone();
        prop_assert_eq!(orig_tag, rt_tag);
    }

    // 29
    #[test]
    fn envelope_has_t_tag(env in arb_envelope()) {
        let v = serde_json::to_value(&env).unwrap();
        prop_assert!(v.get("t").is_some(), "Envelope must have 't' tag");
    }

    // 30
    #[test]
    fn envelope_sequence_decode_stream(envs in prop::collection::vec(arb_envelope(), 1..6)) {
        let mut buf = Vec::new();
        for env in &envs {
            JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
        }
        let reader = std::io::BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        prop_assert_eq!(envs.len(), decoded.len());
    }

    // 31
    #[test]
    fn envelope_hello_contains_version(backend in arb_backend_identity(), caps in arb_capability_manifest()) {
        let env = Envelope::hello(backend, caps);
        let line = JsonlCodec::encode(&env).unwrap();
        prop_assert!(line.contains(CONTRACT_VERSION));
    }

    // 32
    #[test]
    fn envelope_fatal_roundtrip(msg in arb_safe_string()) {
        let env = Envelope::Fatal {
            ref_id: None,
            error: msg.clone(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Fatal { error, .. } = decoded {
            prop_assert_eq!(msg, error);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 33
    #[test]
    fn envelope_run_preserves_work_order_id(id in arb_short_string(), wo in arb_work_order()) {
        let env = Envelope::Run { id: id.clone(), work_order: wo.clone() };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Run { id: rt_id, work_order } = decoded {
            prop_assert_eq!(id, rt_id);
            prop_assert_eq!(wo.id, work_order.id);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 34
    #[test]
    fn envelope_event_preserves_ref_id(ref_id in arb_short_string(), evt in arb_agent_event()) {
        let env = Envelope::Event { ref_id: ref_id.clone(), event: evt };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Event { ref_id: rt_ref, .. } = decoded {
            prop_assert_eq!(ref_id, rt_ref);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 35
    #[test]
    fn envelope_final_preserves_receipt_backend(ref_id in arb_short_string(), r in arb_receipt()) {
        let env = Envelope::Final { ref_id, receipt: r.clone() };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Final { receipt, .. } = decoded {
            prop_assert_eq!(&r.backend.id, &receipt.backend.id);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 36
    #[test]
    fn envelope_double_encode_stable(env in arb_envelope()) {
        let j1 = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(j1.trim()).unwrap();
        let j2 = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(j1, j2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Random glob patterns compile without panic
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 37
    #[test]
    fn glob_valid_patterns_compile(
        include in prop::collection::vec("[a-zA-Z0-9_*?/]{1,16}", 0..4),
        exclude in prop::collection::vec("[a-zA-Z0-9_*?/]{1,16}", 0..4),
    ) {
        let _result = IncludeExcludeGlobs::new(&include, &exclude);
        // should not panic
    }

    // 38
    #[test]
    fn glob_star_patterns_compile(pattern in "[a-zA-Z0-9_*/]{1,20}") {
        let patterns = vec![pattern];
        let _result = IncludeExcludeGlobs::new(&patterns, &[]);
    }

    // 39
    #[test]
    fn glob_double_star_patterns_compile(prefix in "[a-zA-Z]{1,8}", suffix in "[a-zA-Z]{1,8}") {
        let pattern = format!("{prefix}/**/{suffix}");
        let patterns = vec![pattern];
        let result = IncludeExcludeGlobs::new(&patterns, &[]);
        prop_assert!(result.is_ok());
    }

    // 40
    #[test]
    fn glob_extension_patterns_compile(ext in "[a-zA-Z]{1,6}") {
        let pattern = format!("*.{ext}");
        let patterns = vec![pattern];
        let result = IncludeExcludeGlobs::new(&patterns, &[]);
        prop_assert!(result.is_ok());
    }

    // 41
    #[test]
    fn glob_empty_patterns_always_allow(path in arb_safe_string()) {
        let globs = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert!(globs.decide_str(&path).is_allowed());
    }

    // 42
    #[test]
    fn glob_exclude_denies_match(name in "[a-z]{1,8}") {
        let pattern = format!("**/{name}*");
        let globs = IncludeExcludeGlobs::new(&[], &[pattern]).unwrap();
        let decision = globs.decide_str(&name);
        prop_assert!(!decision.is_allowed());
    }

    // 43
    #[test]
    fn glob_include_only_allows_match(ext in "[a-z]{2,5}") {
        let pattern = format!("*.{ext}");
        let globs = IncludeExcludeGlobs::new(&[pattern], &[]).unwrap();
        let decision = globs.decide_str(&format!("file.{ext}"));
        prop_assert!(decision.is_allowed());
    }

    // 44
    #[test]
    fn glob_exclude_takes_precedence(name in "[a-z]{2,6}") {
        let inc = format!("**/{name}*");
        let exc = format!("**/{name}*");
        let globs = IncludeExcludeGlobs::new(&[inc], &[exc]).unwrap();
        let decision = globs.decide_str(&name);
        prop_assert!(!decision.is_allowed());
    }

    // 45
    #[test]
    fn glob_build_globset_empty_returns_none(dummy in 0u8..1u8) {
        let _ = dummy;
        let result = abp_glob::build_globset(&[]).unwrap();
        prop_assert!(result.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Random policy profiles compile without panic
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 46
    #[test]
    fn policy_default_compiles(_x in 0u8..1u8) {
        let engine = PolicyEngine::new(&PolicyProfile::default());
        prop_assert!(engine.is_ok());
    }

    // 47
    #[test]
    fn policy_random_glob_tools_compile(
        allowed in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..5),
        denied in prop::collection::vec("[a-zA-Z*?]{1,8}", 0..5),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            ..PolicyProfile::default()
        };
        prop_assert!(PolicyEngine::new(&policy).is_ok());
    }

    // 48
    #[test]
    fn policy_random_deny_paths_compile(
        deny_read in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..5),
        deny_write in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..5),
    ) {
        let policy = PolicyProfile {
            deny_read,
            deny_write,
            ..PolicyProfile::default()
        };
        prop_assert!(PolicyEngine::new(&policy).is_ok());
    }

    // 49
    #[test]
    fn policy_default_allows_any_tool(tool in arb_short_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    // 50
    #[test]
    fn policy_default_allows_any_read(path in arb_safe_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_read_path(std::path::Path::new(&path)).allowed);
    }

    // 51
    #[test]
    fn policy_default_allows_any_write(path in arb_safe_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(std::path::Path::new(&path)).allowed);
    }

    // 52
    #[test]
    fn policy_disallow_denies_tool(tool in "[a-zA-Z]{2,8}") {
        let policy = PolicyProfile {
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }

    // 53
    #[test]
    fn policy_deny_read_blocks_path(filename in "[a-z]{2,8}") {
        let pattern = format!("**/{filename}*");
        let policy = PolicyProfile {
            deny_read: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_read_path(std::path::Path::new(&filename)).allowed);
    }

    // 54
    #[test]
    fn policy_deny_write_blocks_path(filename in "[a-z]{2,8}") {
        let pattern = format!("**/{filename}*");
        let policy = PolicyProfile {
            deny_write: vec![pattern],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_write_path(std::path::Path::new(&filename)).allowed);
    }

    // 55
    #[test]
    fn policy_serde_roundtrip(policy in arb_policy_profile()) {
        let json = serde_json::to_string(&policy).unwrap();
        let rt: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&policy.allowed_tools, &rt.allowed_tools);
        prop_assert_eq!(&policy.disallowed_tools, &rt.disallowed_tools);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Random capability sets are ordered deterministically
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 56
    #[test]
    fn capability_btreeset_sorted(caps in prop::collection::vec(arb_capability(), 0..10)) {
        let set: BTreeSet<Capability> = caps.into_iter().collect();
        let items: Vec<_> = set.iter().collect();
        for w in items.windows(2) {
            prop_assert!(w[0] <= w[1]);
        }
    }

    // 57
    #[test]
    fn capability_manifest_sorted_keys(manifest in arb_capability_manifest()) {
        let keys: Vec<_> = manifest.keys().collect();
        for w in keys.windows(2) {
            prop_assert!(w[0] <= w[1]);
        }
    }

    // 58
    #[test]
    fn capability_manifest_insert_order_independent(
        pairs in prop::collection::vec((arb_capability(), arb_support_level()), 0..8),
    ) {
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

    // 59
    #[test]
    fn capability_serde_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cap, rt);
    }

    // 60
    #[test]
    fn capability_manifest_serde_preserves_entries(manifest in arb_capability_manifest()) {
        let json = serde_json::to_string(&manifest).unwrap();
        let rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), rt.len());
        for k in rt.keys() {
            prop_assert!(manifest.contains_key(k));
        }
    }

    // 61
    #[test]
    fn capability_set_union_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.into_iter().collect();
        let sb: BTreeSet<Capability> = b.into_iter().collect();
        let u1: BTreeSet<_> = sa.union(&sb).cloned().collect();
        let u2: BTreeSet<_> = sb.union(&sa).cloned().collect();
        prop_assert_eq!(u1, u2);
    }

    // 62
    #[test]
    fn capability_set_intersection_commutative(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.into_iter().collect();
        let sb: BTreeSet<Capability> = b.into_iter().collect();
        let i1: BTreeSet<_> = sa.intersection(&sb).cloned().collect();
        let i2: BTreeSet<_> = sb.intersection(&sa).cloned().collect();
        prop_assert_eq!(i1, i2);
    }

    // 63
    #[test]
    fn capability_set_union_cardinality(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.into_iter().collect();
        let sb: BTreeSet<Capability> = b.into_iter().collect();
        let union: BTreeSet<_> = sa.union(&sb).cloned().collect();
        let inter: BTreeSet<_> = sa.intersection(&sb).cloned().collect();
        prop_assert_eq!(union.len(), sa.len() + sb.len() - inter.len());
    }

    // 64
    #[test]
    fn capability_set_subset_of_union(
        a in prop::collection::vec(arb_capability(), 0..8),
        b in prop::collection::vec(arb_capability(), 0..8),
    ) {
        let sa: BTreeSet<Capability> = a.into_iter().collect();
        let sb: BTreeSet<Capability> = b.into_iter().collect();
        let union: BTreeSet<_> = sa.union(&sb).cloned().collect();
        prop_assert!(sa.is_subset(&union));
        prop_assert!(sb.is_subset(&union));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Deterministic serialization produces same output
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 65
    #[test]
    fn deterministic_work_order_serde(wo in arb_work_order()) {
        let results: Vec<String> = (0..4)
            .map(|_| serde_json::to_string(&wo).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }

    // 66
    #[test]
    fn deterministic_receipt_serde(r in arb_receipt()) {
        let results: Vec<String> = (0..4)
            .map(|_| serde_json::to_string(&r).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }

    // 67
    #[test]
    fn deterministic_agent_event_serde(evt in arb_agent_event()) {
        let results: Vec<String> = (0..4)
            .map(|_| serde_json::to_string(&evt).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }

    // 68
    #[test]
    fn deterministic_envelope_serde(env in arb_envelope()) {
        let results: Vec<String> = (0..4)
            .map(|_| serde_json::to_string(&env).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }

    // 69
    #[test]
    fn deterministic_receipt_hash(r in arb_receipt()) {
        let hashes: Vec<String> = (0..4)
            .map(|_| compute_hash(&r).unwrap())
            .collect();
        for h in &hashes {
            prop_assert_eq!(&hashes[0], h);
        }
    }

    // 70
    #[test]
    fn deterministic_capability_manifest_serde(manifest in arb_capability_manifest()) {
        let results: Vec<String> = (0..4)
            .map(|_| serde_json::to_string(&manifest).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }

    // 71
    #[test]
    fn deterministic_policy_profile_serde(policy in arb_policy_profile()) {
        let results: Vec<String> = (0..4)
            .map(|_| serde_json::to_string(&policy).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }

    // 72
    #[test]
    fn deterministic_envelope_encode(env in arb_envelope()) {
        let results: Vec<String> = (0..4)
            .map(|_| JsonlCodec::encode(&env).unwrap())
            .collect();
        for r in &results {
            prop_assert_eq!(&results[0], r);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  Large random values don't panic (stress properties)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(stress_config())]

    // 73
    #[test]
    fn stress_large_trace_receipt(events in prop::collection::vec(arb_agent_event(), 0..50)) {
        let r = Receipt {
            meta: RunMetadata {
                run_id: Uuid::nil(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: Utc::now(),
                finished_at: Utc::now(),
                duration_ms: 0,
            },
            backend: BackendIdentity {
                id: "stress".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: events,
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let _rt: Receipt = serde_json::from_str(&json).unwrap();
        let _h = compute_hash(&r).unwrap();
    }

    // 74
    #[test]
    fn stress_large_context_packet(
        files in prop::collection::vec(arb_safe_string(), 0..50),
        snippets in prop::collection::vec(arb_context_snippet(), 0..20),
    ) {
        let ctx = ContextPacket { files, snippets };
        let json = serde_json::to_string(&ctx).unwrap();
        let _rt: ContextPacket = serde_json::from_str(&json).unwrap();
    }

    // 75
    #[test]
    fn stress_large_capability_manifest(
        pairs in prop::collection::vec((arb_capability(), arb_support_level()), 0..26),
    ) {
        let manifest: CapabilityManifest = pairs.into_iter().collect();
        let json = serde_json::to_string(&manifest).unwrap();
        let _rt: CapabilityManifest = serde_json::from_str(&json).unwrap();
    }

    // 76
    #[test]
    fn stress_many_artifacts(
        arts in prop::collection::vec(arb_artifact_ref(), 0..50),
    ) {
        let r = Receipt {
            meta: RunMetadata {
                run_id: Uuid::nil(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: Utc::now(),
                finished_at: Utc::now(),
                duration_ms: 0,
            },
            backend: BackendIdentity {
                id: "stress".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
            usage_raw: json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: arts,
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let _rt: Receipt = serde_json::from_str(&json).unwrap();
    }

    // 77
    #[test]
    fn stress_many_envelope_sequence(envs in prop::collection::vec(arb_envelope(), 1..20)) {
        let mut buf = Vec::new();
        for env in &envs {
            JsonlCodec::encode_to_writer(&mut buf, env).unwrap();
        }
        let reader = std::io::BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        prop_assert_eq!(envs.len(), decoded.len());
    }

    // 78
    #[test]
    fn stress_many_policy_rules(
        allowed in prop::collection::vec("[a-zA-Z*]{1,8}", 0..20),
        denied in prop::collection::vec("[a-zA-Z*]{1,8}", 0..20),
        deny_read in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..20),
        deny_write in prop::collection::vec("[a-zA-Z0-9_.*?/]{1,16}", 0..20),
    ) {
        let policy = PolicyProfile {
            allowed_tools: allowed,
            disallowed_tools: denied,
            deny_read,
            deny_write,
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let _engine = PolicyEngine::new(&policy);
    }

    // 79
    #[test]
    fn stress_large_work_order_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        prop_assert!(!json.is_empty());
        let _rt: WorkOrder = serde_json::from_str(&json).unwrap();
    }

    // 80
    #[test]
    fn stress_many_requirements(
        reqs in prop::collection::vec(arb_capability_requirement(), 0..30),
    ) {
        let cr = CapabilityRequirements { required: reqs };
        let json = serde_json::to_string(&cr).unwrap();
        let _rt: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Contract version is always preserved through serde
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 81
    #[test]
    fn contract_version_in_receipt_meta(r in arb_receipt()) {
        prop_assert_eq!(&r.meta.contract_version, CONTRACT_VERSION);
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&rt.meta.contract_version, CONTRACT_VERSION);
    }

    // 82
    #[test]
    fn contract_version_in_hello_envelope(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
    ) {
        let env = Envelope::hello(backend, caps);
        if let Envelope::Hello { contract_version, .. } = &env {
            prop_assert_eq!(contract_version, CONTRACT_VERSION);
        } else {
            prop_assert!(false, "wrong variant");
        }
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Hello { contract_version, .. } = &decoded {
            prop_assert_eq!(contract_version, CONTRACT_VERSION);
        } else {
            prop_assert!(false, "wrong variant after decode");
        }
    }

    // 83
    #[test]
    fn contract_version_constant_is_stable(_x in 0u8..1u8) {
        prop_assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    // 84
    #[test]
    fn contract_version_survives_receipt_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        prop_assert!(json.contains(CONTRACT_VERSION));
    }

    // 85
    #[test]
    fn contract_version_in_hashed_receipt(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert_eq!(&hashed.meta.contract_version, CONTRACT_VERSION);
    }

    // 86
    #[test]
    fn contract_version_in_hello_with_mode(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        mode in arb_execution_mode(),
    ) {
        let env = Envelope::hello_with_mode(backend, caps, mode);
        if let Envelope::Hello { contract_version, .. } = &env {
            prop_assert_eq!(contract_version, CONTRACT_VERSION);
        }
    }

    // 87
    #[test]
    fn contract_version_roundtrip_via_value(r in arb_receipt()) {
        let v = serde_json::to_value(&r).unwrap();
        let meta = v.get("meta").unwrap();
        let cv = meta.get("contract_version").unwrap().as_str().unwrap();
        prop_assert_eq!(cv, CONTRACT_VERSION);
    }

    // 88
    #[test]
    fn contract_version_not_mutated_by_hash(r in arb_receipt()) {
        let hashed = r.clone().with_hash().unwrap();
        prop_assert_eq!(&r.meta.contract_version, &hashed.meta.contract_version);
    }

    // 89
    #[test]
    fn envelope_hello_double_roundtrip_preserves_version(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
    ) {
        let env = Envelope::hello(backend, caps);
        let j1 = JsonlCodec::encode(&env).unwrap();
        let d1 = JsonlCodec::decode(j1.trim()).unwrap();
        let j2 = JsonlCodec::encode(&d1).unwrap();
        let d2 = JsonlCodec::decode(j2.trim()).unwrap();
        if let Envelope::Hello { contract_version, .. } = d2 {
            prop_assert_eq!(&contract_version, CONTRACT_VERSION);
        }
    }

    // 90
    #[test]
    fn receipt_builder_sets_contract_version(backend_id in arb_short_string()) {
        let receipt = abp_core::ReceiptBuilder::new(backend_id)
            .outcome(Outcome::Complete)
            .build();
        prop_assert_eq!(&receipt.meta.contract_version, CONTRACT_VERSION);
    }
}
