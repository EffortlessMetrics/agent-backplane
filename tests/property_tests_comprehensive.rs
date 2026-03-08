#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive property-based tests covering ABP core, protocol, config,
//! receipt hashing, envelope codec, canonical JSON, and receipt chains.

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;

use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use serde_json::json;
use uuid::Uuid;

use abp_config::{BackendEntry, BackplaneConfig};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{ChainBuilder, ReceiptChain, compute_hash, verify_hash};

// ═══════════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════════

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 40,
        ..ProptestConfig::default()
    }
}

fn arb_safe_string() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_ ./-]{1,32}".boxed()
}

fn arb_short_string() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9_]{0,15}".boxed()
}

fn arb_uuid() -> BoxedStrategy<Uuid> {
    any::<u128>().prop_map(Uuid::from_u128).boxed()
}

fn arb_datetime() -> BoxedStrategy<chrono::DateTime<Utc>> {
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
        Just(Capability::Checkpointing),
        Just(Capability::McpClient),
        Just(Capability::ToolUse),
        Just(Capability::ExtendedThinking),
        Just(Capability::ImageInput),
        Just(Capability::CodeExecution),
        Just(Capability::Logprobs),
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

fn arb_context_packet() -> BoxedStrategy<ContextPacket> {
    (
        prop::collection::vec(arb_safe_string(), 0..3),
        prop::collection::vec(
            (arb_safe_string(), arb_safe_string())
                .prop_map(|(name, content)| ContextSnippet { name, content }),
            0..3,
        ),
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

fn arb_capability_requirements() -> BoxedStrategy<CapabilityRequirements> {
    prop::collection::vec(
        (arb_capability(), arb_min_support()).prop_map(|(capability, min_support)| {
            CapabilityRequirement {
                capability,
                min_support,
            }
        }),
        0..4,
    )
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

fn arb_envelope() -> BoxedStrategy<Envelope> {
    prop_oneof![
        (
            arb_backend_identity(),
            arb_capability_manifest(),
            arb_execution_mode()
        )
            .prop_map(|(backend, capabilities, mode)| {
                Envelope::Hello {
                    contract_version: CONTRACT_VERSION.to_string(),
                    backend,
                    capabilities,
                    mode,
                }
            }),
        (arb_short_string(), arb_work_order())
            .prop_map(|(id, work_order)| Envelope::Run { id, work_order }),
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

fn arb_backend_entry() -> BoxedStrategy<BackendEntry> {
    prop_oneof![
        Just(BackendEntry::Mock {}),
        (
            arb_short_string(),
            prop::collection::vec(arb_short_string(), 0..3)
        )
            .prop_map(|(command, args)| BackendEntry::Sidecar {
                command,
                args,
                timeout_secs: None,
            }),
    ]
    .boxed()
}

fn arb_backplane_config() -> BoxedStrategy<BackplaneConfig> {
    (
        prop::option::of(arb_short_string()),
        prop::option::of(arb_safe_string()),
        prop::option::of(prop_oneof![
            Just("debug".to_string()),
            Just("info".to_string()),
            Just("warn".to_string()),
            Just("error".to_string()),
            Just("trace".to_string()),
        ]),
        prop::option::of(arb_safe_string()),
        prop::option::of(arb_safe_string()),
        prop::option::of(1024u16..65535u16),
        prop::collection::vec((arb_short_string(), arb_backend_entry()), 0..3),
    )
        .prop_map(
            |(
                default_backend,
                workspace_dir,
                log_level,
                receipts_dir,
                bind_address,
                port,
                backends_vec,
            )| {
                BackplaneConfig {
                    default_backend,
                    workspace_dir,
                    log_level,
                    receipts_dir,
                    bind_address,
                    port,
                    policy_profiles: vec![],
                    backends: backends_vec.into_iter().collect(),
                }
            },
        )
        .boxed()
}

/// Build a hashed receipt with a fixed timestamp so chain ordering works.
fn make_hashed_receipt(seq: u64) -> Receipt {
    let ts = Utc
        .timestamp_opt(1_700_000_000 + (seq as i64) * 100, 0)
        .unwrap();
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(ts)
        .finished_at(ts)
        .work_order_id(Uuid::from_u128(seq as u128 + 1))
        .with_hash()
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  WorkOrder serde roundtrip (tests 1–8)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 1
    #[test]
    fn wo_full_serde_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
        prop_assert_eq!(wo.context.files.len(), rt.context.files.len());
        prop_assert_eq!(wo.context.snippets.len(), rt.context.snippets.len());
        prop_assert_eq!(&wo.workspace.root, &rt.workspace.root);
        prop_assert_eq!(&wo.workspace.include, &rt.workspace.include);
        prop_assert_eq!(&wo.workspace.exclude, &rt.workspace.exclude);
        prop_assert_eq!(&wo.config.model, &rt.config.model);
        prop_assert_eq!(wo.config.max_turns, rt.config.max_turns);
        prop_assert_eq!(wo.requirements.required.len(), rt.requirements.required.len());
    }

    // 2
    #[test]
    fn wo_value_roundtrip(wo in arb_work_order()) {
        let v = serde_json::to_value(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_value(v).unwrap();
        prop_assert_eq!(wo.id, rt.id);
        prop_assert_eq!(&wo.task, &rt.task);
    }

    // 3
    #[test]
    fn wo_id_preserved(id in arb_uuid()) {
        let wo = WorkOrderBuilder::new("test").build();
        let mut wo2 = wo;
        wo2.id = id;
        let json = serde_json::to_string(&wo2).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(id, rt.id);
    }

    // 4
    #[test]
    fn wo_task_preserved(task in arb_safe_string()) {
        let wo = WorkOrderBuilder::new(task.clone()).build();
        let json = serde_json::to_string(&wo).unwrap();
        let rt: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(task, rt.task);
    }

    // 5
    #[test]
    fn wo_lane_roundtrip(lane in arb_execution_lane()) {
        let json = serde_json::to_string(&lane).unwrap();
        let rt: ExecutionLane = serde_json::from_str(&json).unwrap();
        let j1 = serde_json::to_string(&lane).unwrap();
        let j2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 6
    #[test]
    fn wo_workspace_spec_roundtrip(ws in arb_workspace_spec()) {
        let json = serde_json::to_string(&ws).unwrap();
        let rt: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ws.root, &rt.root);
        prop_assert_eq!(&ws.include, &rt.include);
        prop_assert_eq!(&ws.exclude, &rt.exclude);
    }

    // 7
    #[test]
    fn wo_context_packet_roundtrip(ctx in arb_context_packet()) {
        let json = serde_json::to_string(&ctx).unwrap();
        let rt: ContextPacket = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ctx.files, rt.files);
        prop_assert_eq!(ctx.snippets.len(), rt.snippets.len());
    }

    // 8
    #[test]
    fn wo_runtime_config_roundtrip(cfg in arb_runtime_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: RuntimeConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cfg.model, &rt.model);
        prop_assert_eq!(cfg.max_turns, rt.max_turns);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Receipt hash + roundtrip (tests 9–19)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 9
    #[test]
    fn receipt_with_hash_then_verify(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(verify_hash(&hashed));
    }

    // 10
    #[test]
    fn receipt_hash_deterministic(r in arb_receipt()) {
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
        r2.receipt_sha256 = Some("old_hash".into());
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_eq!(h1, h2);
    }

    // 13
    #[test]
    fn receipt_serde_roundtrip_preserves_fields(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(r.meta.run_id, rt.meta.run_id);
        prop_assert_eq!(r.meta.work_order_id, rt.meta.work_order_id);
        prop_assert_eq!(&r.backend.id, &rt.backend.id);
        prop_assert_eq!(r.trace.len(), rt.trace.len());
        prop_assert_eq!(r.artifacts.len(), rt.artifacts.len());
    }

    // 14
    #[test]
    fn receipt_hash_stable_after_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let rt: Receipt = serde_json::from_str(&json).unwrap();
        let h_orig = compute_hash(&r).unwrap();
        let h_rt = compute_hash(&rt).unwrap();
        prop_assert_eq!(h_orig, h_rt);
    }

    // 15
    #[test]
    fn receipt_hash_changes_on_outcome_mutation(r in arb_receipt()) {
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
    fn receipt_hash_changes_on_backend_mutation(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.backend.id = format!("{}_X", r2.backend.id);
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 17
    #[test]
    fn receipt_hash_changes_on_run_id_mutation(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.meta.run_id = Uuid::from_u128(r.meta.run_id.as_u128().wrapping_add(1));
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 18
    #[test]
    fn receipt_hash_changes_on_trace_append(r in arb_receipt(), evt in arb_agent_event()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.trace.push(evt);
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 19
    #[test]
    fn receipt_outcome_roundtrip(o in arb_outcome()) {
        let json = serde_json::to_string(&o).unwrap();
        let rt: Outcome = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(o, rt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  AgentEvent serde roundtrip (tests 20–30)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 20
    #[test]
    fn event_serde_roundtrip(evt in arb_agent_event()) {
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    // 21
    #[test]
    fn event_value_roundtrip(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_value(v).unwrap();
        prop_assert_eq!(evt.ts, rt.ts);
    }

    // 22
    #[test]
    fn event_has_type_tag(evt in arb_agent_event()) {
        let v = serde_json::to_value(&evt).unwrap();
        prop_assert!(v.get("type").is_some(), "AgentEvent must have 'type' discriminator");
    }

    // 23
    #[test]
    fn event_run_started_preserves_message(msg in arb_safe_string()) {
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

    // 24
    #[test]
    fn event_run_completed_preserves_message(msg in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::RunCompleted { message } = &rt.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 25
    #[test]
    fn event_assistant_delta_preserves_text(text in arb_safe_string()) {
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

    // 26
    #[test]
    fn event_assistant_message_preserves_text(text in arb_safe_string()) {
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

    // 27
    #[test]
    fn event_tool_call_preserves_fields(name in arb_short_string(), val in arb_json_value()) {
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

    // 28
    #[test]
    fn event_tool_result_preserves_fields(
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

    // 29
    #[test]
    fn event_file_changed_preserves_fields(p in arb_safe_string(), s in arb_safe_string()) {
        let evt = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged { path: p.clone(), summary: s.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
            prop_assert_eq!(&p, path);
            prop_assert_eq!(&s, summary);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 30
    #[test]
    fn event_warning_error_roundtrip(msg in arb_safe_string()) {
        let w = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning { message: msg.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&w).unwrap();
        let rt: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::Warning { message } = &rt.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Envelope encode → decode = identity (tests 31–38)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 31
    #[test]
    fn envelope_encode_decode_roundtrip(env in arb_envelope()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();
        prop_assert_eq!(encoded, re_encoded);
    }

    // 32
    #[test]
    fn envelope_encoded_ends_with_newline(env in arb_envelope()) {
        let encoded = JsonlCodec::encode(&env).unwrap();
        prop_assert!(encoded.ends_with('\n'));
    }

    // 33
    #[test]
    fn envelope_hello_has_t_tag(bi in arb_backend_identity(), caps in arb_capability_manifest()) {
        let env = Envelope::hello(bi, caps);
        let encoded = JsonlCodec::encode(&env).unwrap();
        prop_assert!(encoded.contains("\"t\":\"hello\""));
    }

    // 34
    #[test]
    fn envelope_fatal_roundtrip(
        ref_id in prop::option::of(arb_short_string()),
        error in arb_safe_string()
    ) {
        let env = Envelope::Fatal { ref_id: ref_id.clone(), error: error.clone(), error_code: None };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Fatal { ref_id: ri, error: e, .. } = decoded {
            prop_assert_eq!(ref_id, ri);
            prop_assert_eq!(error, e);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 35
    #[test]
    fn envelope_run_roundtrip(id in arb_short_string(), wo in arb_work_order()) {
        let env = Envelope::Run { id: id.clone(), work_order: wo.clone() };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Run { id: rid, work_order } = decoded {
            prop_assert_eq!(id, rid);
            prop_assert_eq!(wo.id, work_order.id);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 36
    #[test]
    fn envelope_event_roundtrip(ref_id in arb_short_string(), evt in arb_agent_event()) {
        let env = Envelope::Event { ref_id: ref_id.clone(), event: evt.clone() };
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        if let Envelope::Event { ref_id: ri, event } = decoded {
            prop_assert_eq!(ref_id, ri);
            prop_assert_eq!(evt.ts, event.ts);
        } else {
            prop_assert!(false, "wrong variant");
        }
    }

    // 37
    #[test]
    fn envelope_stream_roundtrip(envs in prop::collection::vec(arb_envelope(), 1..5)) {
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        prop_assert_eq!(envs.len(), decoded.len());
    }

    // 38
    #[test]
    fn envelope_json_deterministic(env in arb_envelope()) {
        let e1 = JsonlCodec::encode(&env).unwrap();
        let e2 = JsonlCodec::encode(&env).unwrap();
        prop_assert_eq!(e1, e2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Config serialize → deserialize = identity (tests 39–44)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 39
    #[test]
    fn config_json_roundtrip(cfg in arb_backplane_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cfg, rt);
    }

    // 40
    #[test]
    fn config_toml_roundtrip(cfg in arb_backplane_config()) {
        let toml_str = toml::to_string(&cfg).unwrap();
        let rt: BackplaneConfig = toml::from_str(&toml_str).unwrap();
        prop_assert_eq!(cfg, rt);
    }

    // 41
    #[test]
    fn config_default_backend_preserved(name in prop::option::of(arb_short_string())) {
        let mut cfg = BackplaneConfig::default();
        cfg.default_backend = name.clone();
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(name, rt.default_backend);
    }

    // 42
    #[test]
    fn config_port_preserved(port in prop::option::of(1024u16..65535u16)) {
        let mut cfg = BackplaneConfig::default();
        cfg.port = port;
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(port, rt.port);
    }

    // 43
    #[test]
    fn config_backends_count_preserved(
        entries in prop::collection::vec((arb_short_string(), arb_backend_entry()), 0..5)
    ) {
        let mut cfg = BackplaneConfig::default();
        cfg.backends = entries.into_iter().collect();
        let json = serde_json::to_string(&cfg).unwrap();
        let rt: BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cfg.backends.len(), rt.backends.len());
    }

    // 44
    #[test]
    fn config_backend_entry_roundtrip(entry in arb_backend_entry()) {
        let json = serde_json::to_string(&entry).unwrap();
        let rt: BackendEntry = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(entry, rt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Canonical JSON / BTreeMap ordering (tests 45–50)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 45
    #[test]
    fn btreemap_insertion_order_irrelevant(
        pairs in prop::collection::vec((arb_safe_string(), arb_safe_string()), 0..10),
    ) {
        // Property only holds for unique keys — duplicate keys cause
        // last-writer-wins semantics that depend on insertion order.
        let mut seen = BTreeSet::new();
        prop_assume!(pairs.iter().all(|(k, _)| seen.insert(k.clone())));

        let m1: BTreeMap<String, String> = pairs.iter().cloned().collect();
        let mut m2 = BTreeMap::new();
        for (k, v) in pairs.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 46
    #[test]
    fn btreemap_capability_manifest_order_independent(
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

    // 47
    #[test]
    fn btreemap_json_value_roundtrip(
        pairs in prop::collection::vec((arb_short_string(), arb_json_value()), 0..8),
    ) {
        let map: BTreeMap<String, serde_json::Value> = pairs.into_iter().collect();
        let json = serde_json::to_string(&map).unwrap();
        let rt: BTreeMap<String, serde_json::Value> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(map, rt);
    }

    // 48
    #[test]
    fn btreemap_keys_sorted_in_json(
        pairs in prop::collection::vec((arb_short_string(), arb_safe_string()), 2..10),
    ) {
        let map: BTreeMap<String, String> = pairs.into_iter().collect();
        let keys: Vec<_> = map.keys().cloned().collect();
        // BTreeMap keys are always sorted
        let mut sorted = keys.clone();
        sorted.sort();
        prop_assert_eq!(keys, sorted);
    }

    // 49
    #[test]
    fn btreemap_env_deterministic_serialization(
        pairs in prop::collection::vec((arb_short_string(), arb_safe_string()), 0..10),
    ) {
        let deduped: BTreeMap<String, String> = pairs.iter().cloned().collect();
        let j1 = serde_json::to_string(&deduped).unwrap();
        let mut m2 = BTreeMap::new();
        for (k, v) in deduped.iter().rev() {
            m2.insert(k.clone(), v.clone());
        }
        let j2 = serde_json::to_string(&m2).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 50
    #[test]
    fn canonical_receipt_json_deterministic(r in arb_receipt()) {
        let c1 = abp_receipt::canonicalize(&r).unwrap();
        let c2 = abp_receipt::canonicalize(&r).unwrap();
        prop_assert_eq!(c1, c2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Receipt chain (tests 51–55)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig { cases: 20, ..ProptestConfig::default() })]

    // 51
    #[test]
    fn receipt_chain_push_and_verify(n in 1u64..6u64) {
        let mut chain = ReceiptChain::new();
        for i in 0..n {
            let r = make_hashed_receipt(i);
            chain.push(r).unwrap();
        }
        prop_assert!(chain.verify().is_ok());
        prop_assert_eq!(chain.len(), n as usize);
    }

    // 52
    #[test]
    fn receipt_chain_verify_chain_passes(n in 1u64..6u64) {
        let mut chain = ReceiptChain::new();
        for i in 0..n {
            let r = make_hashed_receipt(i);
            chain.push(r).unwrap();
        }
        prop_assert!(chain.verify_chain().is_ok());
    }

    // 53
    #[test]
    fn receipt_chain_no_tampering_detected(n in 1u64..6u64) {
        let mut chain = ReceiptChain::new();
        for i in 0..n {
            chain.push(make_hashed_receipt(i)).unwrap();
        }
        let evidence = chain.detect_tampering();
        prop_assert!(evidence.is_empty(), "Expected no tampering, got: {:?}", evidence);
    }

    // 54
    #[test]
    fn receipt_chain_summary_counts_match(n in 1u64..6u64) {
        let mut chain = ReceiptChain::new();
        for i in 0..n {
            chain.push(make_hashed_receipt(i)).unwrap();
        }
        let summary = chain.chain_summary();
        prop_assert_eq!(summary.total_receipts, n as usize);
        prop_assert_eq!(summary.complete_count, n as usize);
        prop_assert_eq!(summary.failed_count, 0);
        prop_assert!(summary.all_hashes_valid);
    }

    // 55
    #[test]
    fn receipt_chain_rejects_duplicate_ids(_dummy in 0u8..1u8) {
        let r = make_hashed_receipt(0);
        let mut chain = ReceiptChain::new();
        chain.push(r.clone()).unwrap();
        let result = chain.push(r);
        prop_assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Miscellaneous properties (tests 56–60)
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    // 56
    #[test]
    fn capability_serde_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let rt: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cap, rt);
    }

    // 57
    #[test]
    fn support_level_serde_roundtrip(level in arb_support_level()) {
        let json = serde_json::to_string(&level).unwrap();
        let rt: SupportLevel = serde_json::from_str(&json).unwrap();
        let j1 = serde_json::to_string(&level).unwrap();
        let j2 = serde_json::to_string(&rt).unwrap();
        prop_assert_eq!(j1, j2);
    }

    // 58
    #[test]
    fn receipt_builder_produces_valid_hash(outcome in arb_outcome()) {
        let r = ReceiptBuilder::new("test-backend")
            .outcome(outcome)
            .with_hash()
            .unwrap();
        prop_assert!(r.receipt_sha256.is_some());
        prop_assert!(verify_hash(&r));
    }

    // 59
    #[test]
    fn receipt_hash_changes_on_harness_flip(r in arb_receipt()) {
        let h1 = compute_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.verification.harness_ok = !r.verification.harness_ok;
        let h2 = compute_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2);
    }

    // 60
    #[test]
    fn contract_version_present_in_receipt_meta(r in arb_receipt()) {
        prop_assert_eq!(&r.meta.contract_version, CONTRACT_VERSION);
    }
}
