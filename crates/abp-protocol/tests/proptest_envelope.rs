// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for the `Envelope` wire type in `abp-protocol`.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{DateTime, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ── Leaf strategies ─────────────────────────────────────────────────────

fn arb_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_ .-]{0,20}"
}

fn arb_nonempty_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_.-]{1,20}"
}

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<[u8; 16]>().prop_map(Uuid::from_bytes)
}

fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
    // Range covers 1970-01-01 to ~2099-12-31, whole seconds only.
    (0i64..4_102_444_800i64).prop_map(|secs| DateTime::from_timestamp(secs, 0).unwrap())
}

fn arb_json_value_simple() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        arb_string().prop_map(serde_json::Value::String),
        (-1000i64..1000).prop_map(|n| serde_json::Value::Number(n.into())),
    ]
}

// ── Core-type strategies (bottom-up) ────────────────────────────────────

fn arb_capability() -> impl Strategy<Value = Capability> {
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
    ]
}

fn arb_support_level() -> impl Strategy<Value = SupportLevel> {
    prop_oneof![
        Just(SupportLevel::Native),
        Just(SupportLevel::Emulated),
        Just(SupportLevel::Unsupported),
        arb_string().prop_map(|reason| SupportLevel::Restricted { reason }),
    ]
}

fn arb_capability_manifest() -> impl Strategy<Value = CapabilityManifest> {
    prop::collection::btree_map(arb_capability(), arb_support_level(), 0..5)
}

fn arb_backend_identity() -> impl Strategy<Value = BackendIdentity> {
    (
        arb_nonempty_string(),
        prop::option::of(arb_string()),
        prop::option::of(arb_string()),
    )
        .prop_map(|(id, backend_version, adapter_version)| BackendIdentity {
            id,
            backend_version,
            adapter_version,
        })
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![Just(ExecutionMode::Passthrough), Just(ExecutionMode::Mapped)]
}

fn arb_execution_lane() -> impl Strategy<Value = ExecutionLane> {
    prop_oneof![
        Just(ExecutionLane::PatchFirst),
        Just(ExecutionLane::WorkspaceFirst),
    ]
}

fn arb_workspace_mode() -> impl Strategy<Value = WorkspaceMode> {
    prop_oneof![
        Just(WorkspaceMode::PassThrough),
        Just(WorkspaceMode::Staged),
    ]
}

fn arb_workspace_spec() -> impl Strategy<Value = WorkspaceSpec> {
    (
        arb_nonempty_string(),
        arb_workspace_mode(),
        prop::collection::vec(arb_string(), 0..3),
        prop::collection::vec(arb_string(), 0..3),
    )
        .prop_map(|(root, mode, include, exclude)| WorkspaceSpec {
            root,
            mode,
            include,
            exclude,
        })
}

fn arb_context_packet() -> impl Strategy<Value = ContextPacket> {
    (
        prop::collection::vec(arb_string(), 0..3),
        prop::collection::vec(
            (arb_nonempty_string(), arb_string())
                .prop_map(|(name, content)| ContextSnippet { name, content }),
            0..3,
        ),
    )
        .prop_map(|(files, snippets)| ContextPacket { files, snippets })
}

fn arb_policy_profile() -> impl Strategy<Value = PolicyProfile> {
    (
        prop::collection::vec(arb_string(), 0..2),
        prop::collection::vec(arb_string(), 0..2),
    )
        .prop_map(|(allowed_tools, disallowed_tools)| PolicyProfile {
            allowed_tools,
            disallowed_tools,
            ..PolicyProfile::default()
        })
}

fn arb_capability_requirements() -> impl Strategy<Value = CapabilityRequirements> {
    prop::collection::vec(
        (
            arb_capability(),
            prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)],
        )
            .prop_map(|(capability, min_support)| CapabilityRequirement {
                capability,
                min_support,
            }),
        0..3,
    )
    .prop_map(|required| CapabilityRequirements { required })
}

fn arb_runtime_config() -> impl Strategy<Value = RuntimeConfig> {
    (
        prop::option::of(arb_string()),
        prop::option::of(0u32..1000),
    )
        .prop_map(|(model, max_turns)| RuntimeConfig {
            model,
            max_turns,
            ..RuntimeConfig::default()
        })
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        arb_uuid(),
        arb_nonempty_string(),
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
}

fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        arb_string().prop_map(|message| AgentEventKind::RunStarted { message }),
        arb_string().prop_map(|message| AgentEventKind::RunCompleted { message }),
        arb_string().prop_map(|text| AgentEventKind::AssistantDelta { text }),
        arb_string().prop_map(|text| AgentEventKind::AssistantMessage { text }),
        (
            arb_nonempty_string(),
            prop::option::of(arb_string()),
            prop::option::of(arb_string()),
            arb_json_value_simple(),
        )
            .prop_map(
                |(tool_name, tool_use_id, parent_tool_use_id, input)| {
                    AgentEventKind::ToolCall {
                        tool_name,
                        tool_use_id,
                        parent_tool_use_id,
                        input,
                    }
                },
            ),
        (
            arb_nonempty_string(),
            prop::option::of(arb_string()),
            arb_json_value_simple(),
            any::<bool>(),
        )
            .prop_map(|(tool_name, tool_use_id, output, is_error)| {
                AgentEventKind::ToolResult {
                    tool_name,
                    tool_use_id,
                    output,
                    is_error,
                }
            }),
        (arb_string(), arb_string())
            .prop_map(|(path, summary)| AgentEventKind::FileChanged { path, summary }),
        (
            arb_string(),
            prop::option::of(-128i32..128),
            prop::option::of(arb_string()),
        )
            .prop_map(|(command, exit_code, output_preview)| {
                AgentEventKind::CommandExecuted {
                    command,
                    exit_code,
                    output_preview,
                }
            }),
        arb_string().prop_map(|message| AgentEventKind::Warning { message }),
        arb_string().prop_map(|message| AgentEventKind::Error { message }),
    ]
}

fn arb_agent_event() -> impl Strategy<Value = AgentEvent> {
    (arb_datetime(), arb_agent_event_kind()).prop_map(|(ts, kind)| AgentEvent {
        ts,
        kind,
        ext: None,
    })
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (
        arb_uuid(),
        arb_uuid(),
        arb_datetime(),
        arb_datetime(),
        0u64..100_000,
        arb_backend_identity(),
        arb_capability_manifest(),
        arb_execution_mode(),
        arb_outcome(),
    )
        .prop_map(
            |(run_id, wo_id, started, finished, dur, backend, caps, mode, outcome)| Receipt {
                meta: RunMetadata {
                    run_id,
                    work_order_id: wo_id,
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: started,
                    finished_at: finished,
                    duration_ms: dur,
                },
                backend,
                capabilities: caps,
                mode,
                usage_raw: serde_json::json!({}),
                usage: UsageNormalized::default(),
                trace: vec![],
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome,
                receipt_sha256: None,
            },
        )
}

// ── Envelope strategy ───────────────────────────────────────────────────

fn arb_envelope() -> impl Strategy<Value = Envelope> {
    prop_oneof![
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
            }),
        (arb_nonempty_string(), arb_work_order())
            .prop_map(|(id, work_order)| Envelope::Run { id, work_order }),
        (arb_nonempty_string(), arb_agent_event())
            .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event }),
        (arb_nonempty_string(), arb_receipt())
            .prop_map(|(ref_id, receipt)| Envelope::Final { ref_id, receipt }),
        (prop::option::of(arb_nonempty_string()), arb_string())
            .prop_map(|(ref_id, error)| Envelope::Fatal { ref_id, error }),
    ]
}

// ── Property tests ──────────────────────────────────────────────────────

proptest! {
    /// Any `Envelope` serialized to JSON and deserialized back should produce
    /// an identical JSON value (round-trip).
    #[test]
    fn roundtrip_json(envelope in arb_envelope()) {
        let json_str = serde_json::to_string(&envelope).unwrap();
        let decoded: Envelope = serde_json::from_str(&json_str).unwrap();
        let json_str2 = serde_json::to_string(&decoded).unwrap();

        let v1: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&json_str2).unwrap();
        prop_assert_eq!(v1, v2);
    }

    /// JSONL-encoded envelopes should each be a single line of valid JSON.
    #[test]
    fn jsonl_lines_are_valid_json(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();

        // Must end with exactly one newline.
        prop_assert!(encoded.ends_with('\n'));
        prop_assert_eq!(encoded.matches('\n').count(), 1);

        // The trimmed content must parse as a JSON object.
        let parsed: serde_json::Value = serde_json::from_str(encoded.trim_end()).unwrap();
        prop_assert!(parsed.is_object());
    }

    /// Multiple envelopes concatenated as JSONL should all parse back to
    /// identical JSON values.
    #[test]
    fn jsonl_multi_roundtrip(envelopes in prop::collection::vec(arb_envelope(), 1..6)) {
        let mut buf = String::new();
        for env in &envelopes {
            buf.push_str(&JsonlCodec::encode(env).unwrap());
        }

        let lines: Vec<&str> = buf.lines().collect();
        prop_assert_eq!(lines.len(), envelopes.len());

        for (i, line) in lines.iter().enumerate() {
            let decoded = JsonlCodec::decode(line).unwrap();
            let original_val = serde_json::to_value(&envelopes[i]).unwrap();
            let decoded_val = serde_json::to_value(&decoded).unwrap();
            prop_assert_eq!(original_val, decoded_val);
        }
    }

    /// `ref_id` on the `Event` variant must survive a round-trip unchanged.
    #[test]
    fn ref_id_preserved_event(ref_id in arb_nonempty_string(), event in arb_agent_event()) {
        let env = Envelope::Event { ref_id: ref_id.clone(), event };
        let json_str = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json_str).unwrap();

        match decoded {
            Envelope::Event { ref_id: got, .. } => prop_assert_eq!(ref_id, got),
            other => prop_assert!(false, "expected Event, got {:?}", other),
        }
    }

    /// `ref_id` on the `Final` variant must survive a round-trip unchanged.
    #[test]
    fn ref_id_preserved_final(ref_id in arb_nonempty_string(), receipt in arb_receipt()) {
        let env = Envelope::Final { ref_id: ref_id.clone(), receipt };
        let json_str = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json_str).unwrap();

        match decoded {
            Envelope::Final { ref_id: got, .. } => prop_assert_eq!(ref_id, got),
            other => prop_assert!(false, "expected Final, got {:?}", other),
        }
    }

    /// `ref_id` on the `Fatal` variant (when `Some`) must survive a round-trip.
    #[test]
    fn ref_id_preserved_fatal(ref_id in arb_nonempty_string(), error in arb_string()) {
        let env = Envelope::Fatal { ref_id: Some(ref_id.clone()), error };
        let json_str = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json_str).unwrap();

        match decoded {
            Envelope::Fatal { ref_id: got, .. } => prop_assert_eq!(Some(ref_id), got),
            other => prop_assert!(false, "expected Fatal, got {:?}", other),
        }
    }
}
