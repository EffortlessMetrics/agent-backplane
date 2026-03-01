// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive property-based tests for the sidecar JSONL protocol.
//!
//! Covers envelope roundtrips, WorkOrder properties, AgentEvent properties,
//! Receipt properties, and error handling.

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

// ── Core-type strategies ────────────────────────────────────────────────

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

// ═══════════════════════════════════════════════════════════════════════
// (a) Envelope roundtrip properties
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// Any Envelope serde roundtrips through JSON.
    #[test]
    fn envelope_json_roundtrip(envelope in arb_envelope()) {
        let json_str = serde_json::to_string(&envelope).unwrap();
        let decoded: Envelope = serde_json::from_str(&json_str).unwrap();
        let json_str2 = serde_json::to_string(&decoded).unwrap();

        let v1: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&json_str2).unwrap();
        prop_assert_eq!(v1, v2);
    }

    /// Any Envelope serializes to exactly one JSONL line (no embedded newlines).
    #[test]
    fn envelope_single_jsonl_line(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        // Must end with exactly one newline.
        prop_assert!(encoded.ends_with('\n'));
        prop_assert_eq!(encoded.matches('\n').count(), 1);
        // The body before the newline must not contain any newlines or carriage returns.
        let body = &encoded[..encoded.len() - 1];
        prop_assert!(!body.contains('\n'));
        prop_assert!(!body.contains('\r'));
    }

    /// The "t" field is always present in serialized output.
    #[test]
    fn envelope_has_t_field(envelope in arb_envelope()) {
        let json_str = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let obj = parsed.as_object().unwrap();
        prop_assert!(obj.contains_key("t"), "missing 't' field in: {}", json_str);
    }

    /// ref_id correlation: events and final envelopes carry ref_id through roundtrip.
    #[test]
    fn envelope_ref_id_correlation(
        ref_id in arb_nonempty_string(),
        event in arb_agent_event(),
        receipt in arb_receipt(),
    ) {
        // Event envelope preserves ref_id.
        let event_env = Envelope::Event { ref_id: ref_id.clone(), event };
        let event_json = serde_json::to_string(&event_env).unwrap();
        let event_decoded: Envelope = serde_json::from_str(&event_json).unwrap();
        if let Envelope::Event { ref_id: got, .. } = event_decoded {
            prop_assert_eq!(&ref_id, &got);
        } else {
            prop_assert!(false, "expected Event variant");
        }

        // Final envelope preserves ref_id.
        let final_env = Envelope::Final { ref_id: ref_id.clone(), receipt };
        let final_json = serde_json::to_string(&final_env).unwrap();
        let final_decoded: Envelope = serde_json::from_str(&final_json).unwrap();
        if let Envelope::Final { ref_id: got, .. } = final_decoded {
            prop_assert_eq!(&ref_id, &got);
        } else {
            prop_assert!(false, "expected Final variant");
        }
    }

    /// All envelope variants have distinct "t" values.
    #[test]
    fn envelope_distinct_t_values(
        backend in arb_backend_identity(),
        caps in arb_capability_manifest(),
        mode in arb_execution_mode(),
        work_order in arb_work_order(),
        event in arb_agent_event(),
        receipt in arb_receipt(),
    ) {
        let hello = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities: caps,
            mode,
        };
        let run = Envelope::Run { id: "r1".into(), work_order };
        let evt = Envelope::Event { ref_id: "r1".into(), event };
        let fin = Envelope::Final { ref_id: "r1".into(), receipt };
        let fatal = Envelope::Fatal { ref_id: Some("r1".into()), error: "boom".into() };

        let envelopes = [hello, run, evt, fin, fatal];
        let mut t_values = Vec::new();
        for env in &envelopes {
            let json = serde_json::to_string(env).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            let t = parsed.get("t").unwrap().as_str().unwrap().to_string();
            t_values.push(t);
        }
        // All t values should be distinct.
        let unique: std::collections::HashSet<_> = t_values.iter().collect();
        prop_assert_eq!(unique.len(), t_values.len(),
            "expected distinct t values, got: {:?}", t_values);
    }

    /// JSONL codec roundtrip: encode then decode.
    #[test]
    fn envelope_jsonl_codec_roundtrip(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        let original = serde_json::to_value(&envelope).unwrap();
        let result = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(original, result);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (b) WorkOrder property tests
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// Any WorkOrder embedded in a Run envelope roundtrips.
    #[test]
    fn work_order_in_run_roundtrip(id in arb_nonempty_string(), wo in arb_work_order()) {
        let env = Envelope::Run { id: id.clone(), work_order: wo };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        let original_val = serde_json::to_value(&env).unwrap();
        let decoded_val = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(original_val, decoded_val);
    }

    /// WorkOrder.id (a UUID) is always a non-empty string in serialized form.
    #[test]
    fn work_order_id_non_empty(wo in arb_work_order()) {
        let id_str = wo.id.to_string();
        prop_assert!(!id_str.is_empty());
        // UUID string should be 36 chars (8-4-4-4-12).
        prop_assert_eq!(id_str.len(), 36);
    }

    /// WorkOrder.task is preserved through serde.
    #[test]
    fn work_order_task_preserved(wo in arb_work_order()) {
        let original_task = wo.task.clone();
        let json = serde_json::to_string(&wo).unwrap();
        let decoded: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(original_task, decoded.task);
    }

    /// Config vendor fields (BTreeMap) maintain key ordering.
    #[test]
    fn work_order_vendor_key_ordering(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo.config).unwrap();
        let decoded: RuntimeConfig = serde_json::from_str(&json).unwrap();
        let orig_keys: Vec<_> = wo.config.vendor.keys().collect();
        let decoded_keys: Vec<_> = decoded.vendor.keys().collect();
        prop_assert_eq!(orig_keys, decoded_keys);
    }

    /// Capabilities requirements list maintains order.
    #[test]
    fn work_order_requirements_order(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo.requirements).unwrap();
        let decoded: CapabilityRequirements = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.requirements.required.len(), decoded.required.len());
        // Serialize both to values to compare element-by-element.
        let orig_val = serde_json::to_value(&wo.requirements).unwrap();
        let dec_val = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(orig_val, dec_val);
    }

    /// WorkOrder workspace spec roundtrips faithfully.
    #[test]
    fn work_order_workspace_preserved(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let decoded: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.workspace.root, decoded.workspace.root);
        prop_assert_eq!(wo.workspace.include, decoded.workspace.include);
        prop_assert_eq!(wo.workspace.exclude, decoded.workspace.exclude);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (c) AgentEvent property tests
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// Any AgentEvent in an Event envelope roundtrips.
    #[test]
    fn agent_event_in_envelope_roundtrip(ref_id in arb_nonempty_string(), event in arb_agent_event()) {
        let env = Envelope::Event { ref_id, event };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        let orig_val = serde_json::to_value(&env).unwrap();
        let dec_val = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(orig_val, dec_val);
    }

    /// Timestamps are preserved through serde.
    #[test]
    fn agent_event_timestamp_preserved(event in arb_agent_event()) {
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(event.ts, decoded.ts);
    }

    /// AssistantDelta text content is preserved exactly.
    #[test]
    fn assistant_delta_text_preserved(text in arb_string(), ts in arb_datetime()) {
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta { text: text.clone() },
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEventKind::AssistantDelta { text: got } = decoded.kind {
            prop_assert_eq!(text, got);
        } else {
            prop_assert!(false, "expected AssistantDelta");
        }
    }

    /// ToolCall maintains its structure through serde.
    #[test]
    fn tool_call_structure_preserved(
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
            tool_name: got_name,
            tool_use_id: got_id,
            parent_tool_use_id: got_parent,
            input: got_input,
        } = decoded.kind {
            prop_assert_eq!(tool_name, got_name);
            prop_assert_eq!(tool_use_id, got_id);
            prop_assert_eq!(parent_tool_use_id, got_parent);
            prop_assert_eq!(input, got_input);
        } else {
            prop_assert!(false, "expected ToolCall");
        }
    }

    /// ToolResult maintains its structure through serde.
    #[test]
    fn tool_result_structure_preserved(
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
            tool_name: got_name,
            tool_use_id: got_id,
            output: got_output,
            is_error: got_error,
        } = decoded.kind {
            prop_assert_eq!(tool_name, got_name);
            prop_assert_eq!(tool_use_id, got_id);
            prop_assert_eq!(output, got_output);
            prop_assert_eq!(is_error, got_error);
        } else {
            prop_assert!(false, "expected ToolResult");
        }
    }

    /// Event sequences can be concatenated without ambiguity — each line decodes independently.
    #[test]
    fn event_sequence_unambiguous(events in prop::collection::vec(arb_agent_event(), 1..8)) {
        let ref_id = "run-abc";
        let mut buf = String::new();
        for event in &events {
            let env = Envelope::Event { ref_id: ref_id.into(), event: event.clone() };
            buf.push_str(&JsonlCodec::encode(&env).unwrap());
        }

        let lines: Vec<&str> = buf.lines().collect();
        prop_assert_eq!(lines.len(), events.len());

        for (i, line) in lines.iter().enumerate() {
            let decoded = JsonlCodec::decode(line).unwrap();
            if let Envelope::Event { ref_id: got_ref, event: got_event } = decoded {
                prop_assert_eq!(ref_id, &got_ref);
                let orig_val = serde_json::to_value(&events[i]).unwrap();
                let dec_val = serde_json::to_value(&got_event).unwrap();
                prop_assert_eq!(orig_val, dec_val);
            } else {
                prop_assert!(false, "expected Event envelope at line {}", i);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (d) Receipt property tests
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// Any Receipt in a Final envelope roundtrips.
    #[test]
    fn receipt_in_final_roundtrip(ref_id in arb_nonempty_string(), receipt in arb_receipt()) {
        let env = Envelope::Final { ref_id, receipt };
        let json = serde_json::to_string(&env).unwrap();
        let decoded: Envelope = serde_json::from_str(&json).unwrap();
        let orig_val = serde_json::to_value(&env).unwrap();
        let dec_val = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(orig_val, dec_val);
    }

    /// Receipt hash is deterministic for same input.
    #[test]
    fn receipt_hash_deterministic(receipt in arb_receipt()) {
        let hash1 = receipt_hash(&receipt).unwrap();
        let hash2 = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(hash1, hash2);
    }

    /// Receipt with_hash produces valid SHA-256 (64 hex chars).
    #[test]
    fn receipt_with_hash_valid_sha256(receipt in arb_receipt()) {
        let hashed = receipt.with_hash().unwrap();
        let sha = hashed.receipt_sha256.as_ref().unwrap();
        // SHA-256 hex digest is 64 characters.
        prop_assert_eq!(sha.len(), 64);
        // All characters should be valid lowercase hex.
        prop_assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Receipt hash ignores the receipt_sha256 field (self-referential prevention).
    #[test]
    fn receipt_hash_ignores_existing_hash(receipt in arb_receipt()) {
        let hash_without = receipt_hash(&receipt).unwrap();

        let mut receipt_with = receipt;
        receipt_with.receipt_sha256 = Some("aaaa".repeat(16));
        let hash_with = receipt_hash(&receipt_with).unwrap();

        prop_assert_eq!(hash_without, hash_with,
            "receipt_hash should ignore receipt_sha256 field");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// (e) Error handling properties
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// Invalid JSON doesn't panic (returns Err).
    #[test]
    fn invalid_json_returns_err(garbage in "[^{}\\[\\]]{1,50}") {
        let result = JsonlCodec::decode(&garbage);
        prop_assert!(result.is_err());
    }

    /// Missing "t" field returns appropriate error.
    #[test]
    fn missing_t_field_returns_err(
        key in arb_nonempty_string(),
        val in arb_string(),
    ) {
        // Construct valid JSON without a "t" field.
        let json = format!(r#"{{"{key}":"{val}"}}"#);
        // Only test if the key is not "t" itself.
        if key != "t" {
            let result = JsonlCodec::decode(&json);
            prop_assert!(result.is_err());
        }
    }

    /// Empty and whitespace-only input is not valid.
    #[test]
    fn empty_input_returns_err(spaces in "[ \\t]{0,10}") {
        // serde_json::from_str on whitespace-only gives an error.
        if spaces.trim().is_empty() && !spaces.is_empty() {
            let result = JsonlCodec::decode(&spaces);
            prop_assert!(result.is_err());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional property: BTreeMap determinism in config vendor fields
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    /// BTreeMap vendor fields serialize in consistent key order across roundtrips.
    #[test]
    fn btreemap_vendor_deterministic_serialization(
        vendor in prop::collection::btree_map(arb_nonempty_string(), arb_json_value_simple(), 0..5),
    ) {
        let config = RuntimeConfig {
            vendor: vendor.clone(),
            ..RuntimeConfig::default()
        };
        let json1 = serde_json::to_string(&config).unwrap();
        let decoded: RuntimeConfig = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        // Byte-for-byte equal — BTreeMap guarantees order.
        prop_assert_eq!(json1, json2);
    }
}
