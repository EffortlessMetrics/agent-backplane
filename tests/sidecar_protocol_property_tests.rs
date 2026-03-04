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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for the sidecar protocol layer.
//!
//! Covers envelope serde roundtrips, AgentEvent serde properties,
//! WorkOrder/Receipt canonical hashing, protocol state machine validation,
//! and streaming codec properties.
#![allow(
    clippy::useless_vec,
    clippy::needless_borrows_for_generic_args,
    clippy::collapsible_if
)]

use std::collections::BTreeMap;

use abp_core::*;
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::{EnvelopeValidator, SequenceError};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{DateTime, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Arbitrary strategies
// ═══════════════════════════════════════════════════════════════════════

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
    prop_oneof![
        Just(ExecutionMode::Passthrough),
        Just(ExecutionMode::Mapped),
    ]
}

fn arb_execution_lane() -> impl Strategy<Value = ExecutionLane> {
    prop_oneof![
        Just(ExecutionLane::PatchFirst),
        Just(ExecutionLane::WorkspaceFirst),
    ]
}

fn arb_workspace_spec() -> impl Strategy<Value = WorkspaceSpec> {
    (
        arb_nonempty_string(),
        prop_oneof![
            Just(WorkspaceMode::PassThrough),
            Just(WorkspaceMode::Staged),
        ],
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
        prop::collection::btree_map(arb_nonempty_string(), arb_json_value_simple(), 0..3),
    )
        .prop_map(|(model, max_turns, vendor)| RuntimeConfig {
            model,
            max_turns,
            vendor,
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
            .prop_map(|(tool_name, tool_use_id, parent_tool_use_id, input)| {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    parent_tool_use_id,
                    input,
                }
            }),
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
        arb_string().prop_map(|message| AgentEventKind::Error {
            message,
            error_code: None,
        }),
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
        prop::collection::vec(arb_agent_event(), 0..3),
        arb_outcome(),
    )
        .prop_map(
            |(run_id, wo_id, started, finished, dur, backend, caps, mode, trace, outcome)| {
                Receipt {
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
                    trace,
                    artifacts: vec![],
                    verification: VerificationReport::default(),
                    outcome,
                    receipt_sha256: None,
                }
            },
        )
}

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
        (prop::option::of(arb_nonempty_string()), arb_string()).prop_map(|(ref_id, error)| {
            Envelope::Fatal {
                ref_id,
                error,
                error_code: None,
            }
        }),
    ]
}

fn arb_error_code() -> impl Strategy<Value = abp_error::ErrorCode> {
    prop_oneof![
        Just(abp_error::ErrorCode::ProtocolInvalidEnvelope),
        Just(abp_error::ErrorCode::ProtocolUnexpectedMessage),
        Just(abp_error::ErrorCode::ProtocolVersionMismatch),
        Just(abp_error::ErrorCode::BackendNotFound),
        Just(abp_error::ErrorCode::BackendTimeout),
        Just(abp_error::ErrorCode::BackendCrashed),
        Just(abp_error::ErrorCode::CapabilityUnsupported),
        Just(abp_error::ErrorCode::PolicyDenied),
        Just(abp_error::ErrorCode::Internal),
    ]
}

fn arb_ext_map() -> impl Strategy<Value = Option<BTreeMap<String, serde_json::Value>>> {
    prop::option::of(prop::collection::btree_map(
        arb_nonempty_string(),
        arb_json_value_simple(),
        1..4,
    ))
}

// ═══════════════════════════════════════════════════════════════════════
// (1) Envelope serde roundtrip properties (15 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// 1.1 Any valid Envelope serializes to JSON and deserializes back to identical value.
    #[test]
    fn envelope_json_roundtrip(envelope in arb_envelope()) {
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
        let v2 = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(v1, v2);
    }

    /// 1.2 The "t" field is always present in serialized JSON.
    #[test]
    fn envelope_t_field_present(envelope in arb_envelope()) {
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        prop_assert!(parsed.as_object().unwrap().contains_key("t"));
    }

    /// 1.3 ref_id is preserved across serde for Event envelopes.
    #[test]
    fn envelope_event_ref_id_preserved(ref_id in arb_nonempty_string(), event in arb_agent_event()) {
        let env = Envelope::Event { ref_id: ref_id.clone(), event };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Event { ref_id: got, .. } = decoded {
            prop_assert_eq!(ref_id, got);
        } else {
            prop_assert!(false, "expected Event variant");
        }
    }

    /// 1.4 ref_id is preserved across serde for Final envelopes.
    #[test]
    fn envelope_final_ref_id_preserved(ref_id in arb_nonempty_string(), receipt in arb_receipt()) {
        let env = Envelope::Final { ref_id: ref_id.clone(), receipt };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Final { ref_id: got, .. } = decoded {
            prop_assert_eq!(ref_id, got);
        } else {
            prop_assert!(false, "expected Final variant");
        }
    }

    /// 1.5 ref_id is preserved across serde for Fatal envelopes (Some).
    #[test]
    fn envelope_fatal_ref_id_preserved(ref_id in arb_nonempty_string(), error in arb_string()) {
        let env = Envelope::Fatal { ref_id: Some(ref_id.clone()), error, error_code: None };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Fatal { ref_id: got, .. } = decoded {
            prop_assert_eq!(Some(ref_id), got);
        } else {
            prop_assert!(false, "expected Fatal variant");
        }
    }

    /// 1.6 Hello variant roundtrips correctly.
    #[test]
    fn envelope_hello_roundtrip(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        mode in arb_execution_mode(),
    ) {
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities: caps,
            mode,
        };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&env).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 1.7 Run variant roundtrips correctly.
    #[test]
    fn envelope_run_roundtrip(id in arb_nonempty_string(), wo in arb_work_order()) {
        let env = Envelope::Run { id, work_order: wo };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&env).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 1.8 Event variant roundtrips correctly.
    #[test]
    fn envelope_event_roundtrip(ref_id in arb_nonempty_string(), event in arb_agent_event()) {
        let env = Envelope::Event { ref_id, event };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&env).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 1.9 Final variant roundtrips correctly.
    #[test]
    fn envelope_final_roundtrip(ref_id in arb_nonempty_string(), receipt in arb_receipt()) {
        let env = Envelope::Final { ref_id, receipt };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&env).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 1.10 Fatal variant roundtrips correctly.
    #[test]
    fn envelope_fatal_roundtrip(ref_id in prop::option::of(arb_nonempty_string()), error in arb_string()) {
        let env = Envelope::Fatal { ref_id, error, error_code: None };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&env).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 1.11 JSONL codec encode/decode roundtrip.
    #[test]
    fn envelope_jsonl_codec_roundtrip(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 1.12 Encoded JSONL is a single line ending with newline.
    #[test]
    fn envelope_single_jsonl_line(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        prop_assert!(encoded.ends_with('\n'));
        prop_assert_eq!(encoded.matches('\n').count(), 1);
    }

    /// 1.13 All envelope variants have distinct "t" values.
    #[test]
    fn envelope_distinct_t_values(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        mode in arb_execution_mode(),
        wo in arb_work_order(),
        event in arb_agent_event(),
        receipt in arb_receipt(),
    ) {
        let envelopes = [
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode,
            },
            Envelope::Run { id: "r1".into(), work_order: wo },
            Envelope::Event { ref_id: "r1".into(), event },
            Envelope::Final { ref_id: "r1".into(), receipt },
            Envelope::Fatal { ref_id: Some("r1".into()), error: "boom".into(), error_code: None },
        ];
        let t_values: Vec<String> = envelopes.iter().map(|e| {
            let v: serde_json::Value = serde_json::to_value(e).unwrap();
            v.get("t").unwrap().as_str().unwrap().to_string()
        }).collect();
        let unique: std::collections::HashSet<_> = t_values.iter().collect();
        prop_assert_eq!(unique.len(), t_values.len());
    }

    /// 1.14 Fatal envelope with error_code roundtrips the code.
    #[test]
    fn envelope_fatal_error_code_roundtrip(code in arb_error_code()) {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "something failed".into(),
            error_code: Some(code),
        };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Fatal { error_code: got, .. } = decoded {
            prop_assert_eq!(Some(code), got);
        } else {
            prop_assert!(false, "expected Fatal variant");
        }
    }

    /// 1.15 Contract version in Hello envelope is preserved.
    #[test]
    fn envelope_hello_contract_version(backend in arb_backend_identity(), caps in arb_capability_manifest()) {
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities: caps,
            mode: ExecutionMode::default(),
        };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        if let Envelope::Hello { contract_version, .. } = decoded {
            prop_assert_eq!(CONTRACT_VERSION, &contract_version);
        } else {
            prop_assert!(false, "expected Hello variant");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (2) AgentEvent serde properties (15 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// 2.1 Any AgentEvent roundtrips through JSON.
    #[test]
    fn agent_event_json_roundtrip(event in arb_agent_event()) {
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 2.2 The "type" field discriminator is present in serialized AgentEvent.
    #[test]
    fn agent_event_type_discriminator(event in arb_agent_event()) {
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        prop_assert!(parsed.as_object().unwrap().contains_key("type"));
    }

    /// 2.3 RunStarted variant roundtrips.
    #[test]
    fn agent_event_run_started_roundtrip(msg in arb_string(), ts in arb_datetime()) {
        let event = AgentEvent { ts, kind: AgentEventKind::RunStarted { message: msg.clone() }, ext: None };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::RunStarted { message } = &decoded.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "expected RunStarted");
        }
    }

    /// 2.4 RunCompleted variant roundtrips.
    #[test]
    fn agent_event_run_completed_roundtrip(msg in arb_string(), ts in arb_datetime()) {
        let event = AgentEvent { ts, kind: AgentEventKind::RunCompleted { message: msg.clone() }, ext: None };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::RunCompleted { message } = &decoded.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "expected RunCompleted");
        }
    }

    /// 2.5 AssistantDelta text is preserved exactly.
    #[test]
    fn agent_event_assistant_delta_roundtrip(text in arb_string(), ts in arb_datetime()) {
        let event = AgentEvent { ts, kind: AgentEventKind::AssistantDelta { text: text.clone() }, ext: None };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantDelta { text: got } = &decoded.kind {
            prop_assert_eq!(&text, got);
        } else {
            prop_assert!(false, "expected AssistantDelta");
        }
    }

    /// 2.6 AssistantMessage text is preserved.
    #[test]
    fn agent_event_assistant_message_roundtrip(text in arb_string(), ts in arb_datetime()) {
        let event = AgentEvent { ts, kind: AgentEventKind::AssistantMessage { text: text.clone() }, ext: None };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantMessage { text: got } = &decoded.kind {
            prop_assert_eq!(&text, got);
        } else {
            prop_assert!(false, "expected AssistantMessage");
        }
    }

    /// 2.7 ToolCall structure roundtrips faithfully.
    #[test]
    fn agent_event_tool_call_roundtrip(
        tool_name in arb_nonempty_string(),
        tool_use_id in prop::option::of(arb_string()),
        parent_tool_use_id in prop::option::of(arb_string()),
        input in arb_json_value_simple(),
        ts in arb_datetime(),
    ) {
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::ToolCall {
                tool_name: tool_name.clone(),
                tool_use_id: tool_use_id.clone(),
                parent_tool_use_id: parent_tool_use_id.clone(),
                input: input.clone(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::ToolCall {
            tool_name: n, tool_use_id: u, parent_tool_use_id: p, input: i,
        } = &decoded.kind {
            prop_assert_eq!(&tool_name, n);
            prop_assert_eq!(&tool_use_id, u);
            prop_assert_eq!(&parent_tool_use_id, p);
            prop_assert_eq!(&input, i);
        } else {
            prop_assert!(false, "expected ToolCall");
        }
    }

    /// 2.8 ToolResult structure roundtrips.
    #[test]
    fn agent_event_tool_result_roundtrip(
        tool_name in arb_nonempty_string(),
        tool_use_id in prop::option::of(arb_string()),
        output in arb_json_value_simple(),
        is_error in any::<bool>(),
        ts in arb_datetime(),
    ) {
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::ToolResult {
                tool_name: tool_name.clone(),
                tool_use_id: tool_use_id.clone(),
                output: output.clone(),
                is_error,
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::ToolResult {
            tool_name: n, tool_use_id: u, output: o, is_error: e,
        } = &decoded.kind {
            prop_assert_eq!(&tool_name, n);
            prop_assert_eq!(&tool_use_id, u);
            prop_assert_eq!(&output, o);
            prop_assert_eq!(&is_error, e);
        } else {
            prop_assert!(false, "expected ToolResult");
        }
    }

    /// 2.9 FileChanged roundtrips.
    #[test]
    fn agent_event_file_changed_roundtrip(path in arb_string(), summary in arb_string(), ts in arb_datetime()) {
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::FileChanged { path: path.clone(), summary: summary.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::FileChanged { path: p, summary: s } = &decoded.kind {
            prop_assert_eq!(&path, p);
            prop_assert_eq!(&summary, s);
        } else {
            prop_assert!(false, "expected FileChanged");
        }
    }

    /// 2.10 CommandExecuted roundtrips.
    #[test]
    fn agent_event_command_executed_roundtrip(
        command in arb_string(),
        exit_code in prop::option::of(-128i32..128),
        output_preview in prop::option::of(arb_string()),
        ts in arb_datetime(),
    ) {
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::CommandExecuted {
                command: command.clone(),
                exit_code,
                output_preview: output_preview.clone(),
            },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::CommandExecuted { command: c, exit_code: e, output_preview: o } = &decoded.kind {
            prop_assert_eq!(&command, c);
            prop_assert_eq!(&exit_code, e);
            prop_assert_eq!(&output_preview, o);
        } else {
            prop_assert!(false, "expected CommandExecuted");
        }
    }

    /// 2.11 Warning and Error variants roundtrip.
    #[test]
    fn agent_event_warning_error_roundtrip(msg in arb_string(), ts in arb_datetime()) {
        let warning = AgentEvent { ts, kind: AgentEventKind::Warning { message: msg.clone() }, ext: None };
        let json_w = serde_json::to_string(&warning).unwrap();
        let dec_w: AgentEvent = serde_json::from_str(&json_w).unwrap();
        if let AgentEventKind::Warning { message } = &dec_w.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "expected Warning");
        }

        let error = AgentEvent { ts, kind: AgentEventKind::Error { message: msg.clone(), error_code: None }, ext: None };
        let json_e = serde_json::to_string(&error).unwrap();
        let dec_e: AgentEvent = serde_json::from_str(&json_e).unwrap();
        if let AgentEventKind::Error { message, .. } = &dec_e.kind {
            prop_assert_eq!(&msg, message);
        } else {
            prop_assert!(false, "expected Error");
        }
    }

    /// 2.12 Timestamps survive roundtrip exactly.
    #[test]
    fn agent_event_timestamp_preserved(event in arb_agent_event()) {
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(event.ts, decoded.ts);
    }

    /// 2.13 Content strings with Unicode survive roundtrip.
    #[test]
    fn agent_event_unicode_content(ts in arb_datetime()) {
        let texts = vec!["héllo", "日本語", "emoji: 🚀✨", "Ω≈ç√∫"];
        for text in texts {
            let event = AgentEvent {
                ts,
                kind: AgentEventKind::AssistantDelta { text: text.to_string() },
                ext: None,
            };
            let json = serde_json::to_string(&event).unwrap();
            let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
            if let AgentEventKind::AssistantDelta { text: got } = &decoded.kind {
                prop_assert_eq!(text, got.as_str());
            } else {
                prop_assert!(false, "expected AssistantDelta");
            }
        }
    }

    /// 2.14 Extension fields survive roundtrip.
    #[test]
    fn agent_event_ext_roundtrip(ts in arb_datetime(), kind in arb_agent_event_kind(), ext in arb_ext_map()) {
        let event = AgentEvent { ts, kind, ext: ext.clone() };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::to_value(&decoded).unwrap()
        );
    }

    /// 2.15 Large payload serializes correctly.
    #[test]
    fn agent_event_large_payload(ts in arb_datetime()) {
        let large_text = "x".repeat(100_000);
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage { text: large_text.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantMessage { text } = &decoded.kind {
            prop_assert_eq!(large_text.len(), text.len());
            prop_assert_eq!(&large_text, text);
        } else {
            prop_assert!(false, "expected AssistantMessage");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (3) WorkOrder/Receipt canonical hashing properties (10 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// 3.1 canonical_json produces deterministic output for any WorkOrder.
    #[test]
    fn canonical_json_work_order_determinism(wo in arb_work_order()) {
        let s1 = canonical_json(&wo).unwrap();
        let s2 = canonical_json(&wo).unwrap();
        prop_assert_eq!(s1, s2);
    }

    /// 3.2 canonical_json produces deterministic output for any Receipt.
    #[test]
    fn canonical_json_receipt_determinism(r in arb_receipt()) {
        let s1 = canonical_json(&r).unwrap();
        let s2 = canonical_json(&r).unwrap();
        prop_assert_eq!(s1, s2);
    }

    /// 3.3 receipt_hash produces consistent hashes.
    #[test]
    fn receipt_hash_consistency(r in arb_receipt()) {
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    /// 3.4 with_hash is idempotent: calling twice gives same result.
    #[test]
    fn receipt_with_hash_idempotent(r in arb_receipt()) {
        let r1 = r.clone().with_hash().unwrap();
        let r2 = r1.clone().with_hash().unwrap();
        prop_assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    }

    /// 3.5 receipt_hash produces valid SHA-256 hex strings (64 chars).
    #[test]
    fn receipt_hash_valid_format(r in arb_receipt()) {
        let h = receipt_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// 3.6 Hash ignores existing receipt_sha256 field (self-referential prevention).
    #[test]
    fn receipt_hash_ignores_self(r in arb_receipt()) {
        let h_clean = receipt_hash(&r).unwrap();
        let mut r_dirty = r;
        r_dirty.receipt_sha256 = Some("deadbeef".repeat(8));
        let h_dirty = receipt_hash(&r_dirty).unwrap();
        prop_assert_eq!(h_clean, h_dirty);
    }

    /// 3.7 Hash changes when task field in work order changes (sensitivity via receipt trace change).
    #[test]
    fn receipt_hash_sensitive_to_outcome(r in arb_receipt()) {
        let mut r_changed = r.clone();
        r_changed.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r_changed).unwrap();
        prop_assert_ne!(h1, h2);
    }

    /// 3.8 Hash changes when backend id changes.
    #[test]
    fn receipt_hash_sensitive_to_backend(r in arb_receipt()) {
        let mut r_changed = r.clone();
        r_changed.backend.id = format!("{}_modified", r.backend.id);
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r_changed).unwrap();
        prop_assert_ne!(h1, h2);
    }

    /// 3.9 Hash changes when duration_ms changes.
    #[test]
    fn receipt_hash_sensitive_to_duration(r in arb_receipt()) {
        let mut r_changed = r.clone();
        r_changed.meta.duration_ms = r.meta.duration_ms.wrapping_add(1);
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r_changed).unwrap();
        prop_assert_ne!(h1, h2);
    }

    /// 3.10 canonical_json output is valid JSON that round-trips.
    #[test]
    fn canonical_json_valid_json(wo in arb_work_order()) {
        let canonical = canonical_json(&wo).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&canonical).unwrap();
        let reparsed = serde_json::to_string(&parsed).unwrap();
        prop_assert_eq!(canonical, reparsed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (4) Protocol state machine properties (10 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// 4.1 Hello must precede run: sequence without Hello is invalid.
    #[test]
    fn state_machine_missing_hello(wo in arb_work_order(), receipt in arb_receipt()) {
        let seq = vec![
            Envelope::Run { id: "r1".into(), work_order: wo },
            Envelope::Final { ref_id: "r1".into(), receipt },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        prop_assert!(errors.iter().any(|e| matches!(e, SequenceError::MissingHello)));
    }

    /// 4.2 Events before run are out of order.
    #[test]
    fn state_machine_event_before_run(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        event in arb_agent_event(),
        receipt in arb_receipt(),
    ) {
        let seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Event { ref_id: "r1".into(), event },
            Envelope::Final { ref_id: "r1".into(), receipt },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        prop_assert!(errors.iter().any(|e| matches!(e, SequenceError::OutOfOrderEvents)));
    }

    /// 4.3 Final is terminal: valid sequence Hello → Run → Final.
    #[test]
    fn state_machine_valid_hello_run_final(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
        receipt in arb_receipt(),
    ) {
        let seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Run { id: "r1".into(), work_order: wo },
            Envelope::Final { ref_id: "r1".into(), receipt },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        let errors: Vec<_> = errors.into_iter().filter(|e| {
            !matches!(e, SequenceError::RefIdMismatch { .. })
        }).collect();
        prop_assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    /// 4.4 Fatal is terminal: valid sequence Hello → Run → Fatal.
    #[test]
    fn state_machine_valid_hello_run_fatal(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
    ) {
        let seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Run { id: "r1".into(), work_order: wo },
            Envelope::Fatal { ref_id: Some("r1".into()), error: "boom".into(), error_code: None },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        prop_assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    /// 4.5 Multiple terminals are rejected.
    #[test]
    fn state_machine_multiple_terminals(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
        receipt in arb_receipt(),
    ) {
        let seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Run { id: "r1".into(), work_order: wo },
            Envelope::Final { ref_id: "r1".into(), receipt },
            Envelope::Fatal { ref_id: Some("r1".into()), error: "extra".into(), error_code: None },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        prop_assert!(errors.iter().any(|e| matches!(e, SequenceError::MultipleTerminals)));
    }

    /// 4.6 ref_id correlation: mismatched ref_id is detected.
    #[test]
    fn state_machine_ref_id_mismatch(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
        event in arb_agent_event(),
        receipt in arb_receipt(),
    ) {
        let seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Run { id: "run-abc".into(), work_order: wo },
            Envelope::Event { ref_id: "wrong-id".into(), event },
            Envelope::Final { ref_id: "run-abc".into(), receipt },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        let has_mismatch = errors.iter().any(|e| matches!(e, SequenceError::RefIdMismatch { expected: _, found: _ }));
        prop_assert!(has_mismatch);
    }

    /// 4.7 Valid sequence with events: Hello → Run → Event* → Final.
    #[test]
    fn state_machine_valid_with_events(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
        events in prop::collection::vec(arb_agent_event(), 1..5),
        receipt in arb_receipt(),
    ) {
        let mut seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Run { id: "r1".into(), work_order: wo },
        ];
        for event in events {
            seq.push(Envelope::Event { ref_id: "r1".into(), event });
        }
        seq.push(Envelope::Final { ref_id: "r1".into(), receipt });
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        let errors: Vec<_> = errors.into_iter().filter(|e| {
            !matches!(e, SequenceError::RefIdMismatch { .. })
        }).collect();
        prop_assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    /// 4.8 Hello not at position 0 is detected.
    #[test]
    fn state_machine_hello_not_first(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
        receipt in arb_receipt(),
    ) {
        let seq = vec![
            Envelope::Run { id: "r1".into(), work_order: wo },
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Final { ref_id: "r1".into(), receipt },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        let has_not_first = errors.iter().any(|e| matches!(e, SequenceError::HelloNotFirst { position: _ }));
        prop_assert!(has_not_first);
    }

    /// 4.9 Empty sequence gives MissingHello and MissingTerminal.
    #[test]
    fn state_machine_empty_sequence(_unused in Just(())) {
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[]);
        prop_assert!(errors.iter().any(|e| matches!(e, SequenceError::MissingHello)));
        prop_assert!(errors.iter().any(|e| matches!(e, SequenceError::MissingTerminal)));
    }

    /// 4.10 Missing terminal is detected.
    #[test]
    fn state_machine_missing_terminal(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        wo in arb_work_order(),
    ) {
        let seq = vec![
            Envelope::Hello {
                contract_version: CONTRACT_VERSION.to_string(),
                backend,
                capabilities: caps,
                mode: ExecutionMode::default(),
            },
            Envelope::Run { id: "r1".into(), work_order: wo },
        ];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        prop_assert!(errors.iter().any(|e| matches!(e, SequenceError::MissingTerminal)));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (5) Streaming codec properties (10 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// 5.1 Splitting valid JSONL at any byte boundary produces same parse result.
    #[test]
    fn streaming_split_any_boundary(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let bytes = encoded.as_bytes();
        let expected_val = serde_json::to_value(&envelope).unwrap();

        for split_at in 0..=bytes.len() {
            let (first, second) = bytes.split_at(split_at);
            let mut parser = StreamParser::new();
            let mut results: Vec<Result<Envelope, _>> = parser.push(first);
            results.extend(parser.push(second));
            let parsed: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
            prop_assert_eq!(parsed.len(), 1, "split_at={}", split_at);
            let got_val = serde_json::to_value(&parsed[0]).unwrap();
            prop_assert_eq!(&expected_val, &got_val, "split_at={}", split_at);
        }
    }

    /// 5.2 Multiple JSONL messages concatenated parse correctly.
    #[test]
    fn streaming_multiple_messages(envelopes in prop::collection::vec(arb_envelope(), 1..6)) {
        let mut buf = Vec::new();
        for env in &envelopes {
            buf.extend(JsonlCodec::encode(env).unwrap().as_bytes());
        }
        let mut parser = StreamParser::new();
        let results = parser.push(&buf);
        let parsed: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
        prop_assert_eq!(parsed.len(), envelopes.len());
        for (orig, got) in envelopes.iter().zip(parsed.iter()) {
            prop_assert_eq!(
                serde_json::to_value(orig).unwrap(),
                serde_json::to_value(got).unwrap()
            );
        }
    }

    /// 5.3 Partial lines don't produce events until completed.
    #[test]
    fn streaming_partial_line_buffered(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let bytes = encoded.as_bytes();
        // Remove the trailing newline to simulate a partial line.
        let partial = &bytes[..bytes.len() - 1];
        let mut parser = StreamParser::new();
        let results = parser.push(partial);
        prop_assert!(results.is_empty(), "partial line should not produce results");
        prop_assert!(!parser.is_empty(), "parser should have buffered data");
    }

    /// 5.4 Finishing a partial line produces the envelope.
    #[test]
    fn streaming_finish_produces_envelope(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let bytes = encoded.as_bytes();
        let partial = &bytes[..bytes.len() - 1];
        let mut parser = StreamParser::new();
        let _ = parser.push(partial);
        let results = parser.finish();
        let parsed: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
        prop_assert_eq!(parsed.len(), 1);
        prop_assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            serde_json::to_value(&parsed[0]).unwrap()
        );
    }

    /// 5.5 Byte-by-byte feeding produces same result.
    #[test]
    fn streaming_byte_by_byte(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let bytes = encoded.as_bytes();
        let mut parser = StreamParser::new();
        let mut all_results = Vec::new();
        for &byte in bytes {
            all_results.extend(parser.push(&[byte]));
        }
        let parsed: Vec<_> = all_results.into_iter().filter_map(|r| r.ok()).collect();
        prop_assert_eq!(parsed.len(), 1);
        prop_assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            serde_json::to_value(&parsed[0]).unwrap()
        );
    }

    /// 5.6 Empty input produces no events.
    #[test]
    fn streaming_empty_input(_unused in Just(())) {
        let mut parser = StreamParser::new();
        let results = parser.push(b"");
        prop_assert!(results.is_empty());
        prop_assert!(parser.is_empty());
    }

    /// 5.7 Blank lines are skipped in the stream.
    #[test]
    fn streaming_blank_lines_skipped(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let with_blanks = format!("\n\n{}\n\n", encoded.trim_end());
        let mut parser = StreamParser::new();
        let results = parser.push(with_blanks.as_bytes());
        let parsed: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
        prop_assert_eq!(parsed.len(), 1);
        prop_assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            serde_json::to_value(&parsed[0]).unwrap()
        );
    }

    /// 5.8 Multiple envelopes fed in two chunks at arbitrary split.
    #[test]
    fn streaming_two_messages_split(
        env1 in arb_envelope(),
        env2 in arb_envelope(),
        split_frac in 0.0f64..1.0,
    ) {
        let mut buf = Vec::new();
        buf.extend(JsonlCodec::encode(&env1).unwrap().as_bytes());
        buf.extend(JsonlCodec::encode(&env2).unwrap().as_bytes());
        let split_at = (split_frac * buf.len() as f64) as usize;
        let split_at = split_at.min(buf.len());
        let (first, second) = buf.split_at(split_at);

        let mut parser = StreamParser::new();
        let mut results: Vec<Result<Envelope, _>> = parser.push(first);
        results.extend(parser.push(second));
        let parsed: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
        prop_assert_eq!(parsed.len(), 2);
        prop_assert_eq!(
            serde_json::to_value(&env1).unwrap(),
            serde_json::to_value(&parsed[0]).unwrap()
        );
        prop_assert_eq!(
            serde_json::to_value(&env2).unwrap(),
            serde_json::to_value(&parsed[1]).unwrap()
        );
    }

    /// 5.9 Reset clears buffered data.
    #[test]
    fn streaming_reset_clears_buffer(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let partial = &encoded.as_bytes()[..encoded.len() / 2];
        let mut parser = StreamParser::new();
        let _ = parser.push(partial);
        prop_assert!(!parser.is_empty());
        parser.reset();
        prop_assert!(parser.is_empty());
    }

    /// 5.10 decode_stream from BufRead matches JSONL line-by-line decode.
    #[test]
    fn streaming_decode_stream_matches_line_decode(envelopes in prop::collection::vec(arb_envelope(), 1..6)) {
        let mut buf = String::new();
        for env in &envelopes {
            buf.push_str(&JsonlCodec::encode(env).unwrap());
        }
        let reader = std::io::BufReader::new(buf.as_bytes());
        let stream_results: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .filter_map(|r| r.ok())
            .collect();
        prop_assert_eq!(stream_results.len(), envelopes.len());
        for (orig, got) in envelopes.iter().zip(stream_results.iter()) {
            prop_assert_eq!(
                serde_json::to_value(orig).unwrap(),
                serde_json::to_value(got).unwrap()
            );
        }
    }
}
