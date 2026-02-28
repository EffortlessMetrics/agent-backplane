// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `JsonlCodec`, version parsing, and version
//! compatibility in `abp-protocol`.

use abp_core::*;
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use chrono::DateTime;
use proptest::prelude::*;
use std::io::BufReader;
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

fn arb_datetime() -> impl Strategy<Value = DateTime<chrono::Utc>> {
    (0i64..4_102_444_800i64).prop_map(|secs| DateTime::from_timestamp(secs, 0).unwrap())
}

// ── Core-type strategies────────────────────────────────────────────────

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

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        arb_uuid(),
        arb_nonempty_string(),
        prop_oneof![
            Just(ExecutionLane::PatchFirst),
            Just(ExecutionLane::WorkspaceFirst),
        ],
        (
            arb_nonempty_string(),
            prop_oneof![
                Just(WorkspaceMode::PassThrough),
                Just(WorkspaceMode::Staged),
            ],
        )
            .prop_map(|(root, mode)| WorkspaceSpec {
                root,
                mode,
                include: vec![],
                exclude: vec![],
            }),
    )
        .prop_map(|(id, task, lane, workspace)| WorkOrder {
            id,
            task,
            lane,
            workspace,
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        })
}

fn arb_agent_event() -> impl Strategy<Value = AgentEvent> {
    (
        arb_datetime(),
        prop_oneof![
            arb_string().prop_map(|message| AgentEventKind::RunStarted { message }),
            arb_string().prop_map(|text| AgentEventKind::AssistantDelta { text }),
            arb_string().prop_map(|message| AgentEventKind::Warning { message }),
            arb_string().prop_map(|message| AgentEventKind::Error { message }),
        ],
    )
        .prop_map(|(ts, kind)| AgentEvent {
            ts,
            kind,
            ext: None,
        })
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (
        arb_uuid(),
        arb_uuid(),
        arb_datetime(),
        arb_datetime(),
        0u64..100_000,
        arb_backend_identity(),
        arb_execution_mode(),
        prop_oneof![
            Just(Outcome::Complete),
            Just(Outcome::Partial),
            Just(Outcome::Failed),
        ],
    )
        .prop_map(
            |(run_id, wo_id, started, finished, dur, backend, mode, outcome)| Receipt {
                meta: RunMetadata {
                    run_id,
                    work_order_id: wo_id,
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: started,
                    finished_at: finished,
                    duration_ms: dur,
                },
                backend,
                capabilities: std::collections::BTreeMap::new(),
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
            prop::collection::btree_map(arb_capability(), arb_support_level(), 0..5),
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
    /// Any valid envelope survives encode → decode through `JsonlCodec`.
    #[test]
    fn codec_encode_decode_roundtrip(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();

        let original = serde_json::to_value(&envelope).unwrap();
        let result = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(original, result);
    }

    /// Multiple envelopes survive `encode_many_to_writer` → `decode_stream`.
    #[test]
    fn codec_many_writer_stream_roundtrip(envelopes in prop::collection::vec(arb_envelope(), 1..8)) {
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

        let reader = BufReader::new(buf.as_slice());
        let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
            .collect::<Result<_, _>>()
            .unwrap();

        prop_assert_eq!(envelopes.len(), decoded.len());

        for (orig, dec) in envelopes.iter().zip(decoded.iter()) {
            let v1 = serde_json::to_value(orig).unwrap();
            let v2 = serde_json::to_value(dec).unwrap();
            prop_assert_eq!(v1, v2);
        }
    }

    /// `parse_version` is consistent with the `"abp/vMAJOR.MINOR"` format:
    /// formatting a parsed pair back must yield the same parse result.
    #[test]
    fn version_parse_format_consistency(major in 0u32..1000, minor in 0u32..1000) {
        let version_str = format!("abp/v{major}.{minor}");
        let parsed = parse_version(&version_str);
        prop_assert_eq!(parsed, Some((major, minor)));
    }

    /// Compatible versions are reflexive: any valid version is compatible
    /// with itself.
    #[test]
    fn version_compatibility_reflexive(major in 0u32..100, minor in 0u32..100) {
        let v = format!("abp/v{major}.{minor}");
        prop_assert!(is_compatible_version(&v, &v));
    }

    /// Incompatible versions are symmetric: if A is incompatible with B,
    /// then B is incompatible with A.
    #[test]
    fn version_incompatibility_symmetric(
        major_a in 0u32..100,
        minor_a in 0u32..100,
        major_b in 0u32..100,
        minor_b in 0u32..100,
    ) {
        let a = format!("abp/v{major_a}.{minor_a}");
        let b = format!("abp/v{major_b}.{minor_b}");
        let ab = is_compatible_version(&a, &b);
        let ba = is_compatible_version(&b, &a);
        // Compatibility (and thus incompatibility) must be symmetric.
        prop_assert_eq!(ab, ba);
    }
}
