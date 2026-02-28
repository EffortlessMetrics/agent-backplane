// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for core contract invariants.

use std::path::Path;

use abp_core::{
    receipt_hash, AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest,
    ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RunMetadata, RuntimeConfig, UsageNormalized, VerificationReport,
    WorkOrderBuilder, WorkOrder, WorkspaceMode, WorkspaceSpec, CapabilityRequirements,
    CONTRACT_VERSION,
};
use abp_core::chain::ReceiptChain;
use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_policy::PolicyEngine;
use abp_protocol::version::{ProtocolVersion, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{DateTime, TimeZone, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Arbitrary strategies for contract types
// ---------------------------------------------------------------------------

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<u128>().prop_map(Uuid::from_u128)
}

fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
    // Range of valid timestamps (2020-01-01 to 2030-01-01)
    (1_577_836_800i64..1_893_456_000i64).prop_map(|secs| {
        Utc.timestamp_opt(secs, 0).single().unwrap()
    })
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![Just(ExecutionMode::Passthrough), Just(ExecutionMode::Mapped),]
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

fn arb_safe_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_ /.-]{1,30}"
}

fn arb_backend_identity() -> impl Strategy<Value = BackendIdentity> {
    (arb_safe_string(), any::<bool>(), arb_safe_string()).prop_map(|(id, has_ver, ver)| {
        BackendIdentity {
            id,
            backend_version: if has_ver { Some(ver) } else { None },
            adapter_version: None,
        }
    })
}

fn arb_usage() -> impl Strategy<Value = UsageNormalized> {
    (
        any::<Option<u64>>(),
        any::<Option<u64>>(),
        any::<Option<u64>>(),
        any::<Option<u64>>(),
    )
        .prop_map(|(inp, out, cr, cw)| UsageNormalized {
            input_tokens: inp,
            output_tokens: out,
            cache_read_tokens: cr,
            cache_write_tokens: cw,
            request_units: None,
            estimated_cost_usd: None,
        })
}

fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        arb_safe_string().prop_map(|m| AgentEventKind::RunStarted { message: m }),
        arb_safe_string().prop_map(|m| AgentEventKind::RunCompleted { message: m }),
        arb_safe_string().prop_map(|t| AgentEventKind::AssistantDelta { text: t }),
        arb_safe_string().prop_map(|t| AgentEventKind::AssistantMessage { text: t }),
        arb_safe_string().prop_map(|m| AgentEventKind::Warning { message: m }),
        arb_safe_string().prop_map(|m| AgentEventKind::Error { message: m }),
        (arb_safe_string(), arb_safe_string()).prop_map(|(p, su)| AgentEventKind::FileChanged {
            path: p,
            summary: su,
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

fn arb_verification() -> impl Strategy<Value = VerificationReport> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(diff, status, harness)| {
        VerificationReport {
            git_diff: if diff { Some("diff".into()) } else { None },
            git_status: if status {
                Some("M file.rs".into())
            } else {
                None
            },
            harness_ok: harness,
        }
    })
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (
        arb_uuid(),
        arb_uuid(),
        arb_datetime(),
        arb_datetime(),
        arb_backend_identity(),
        arb_outcome(),
        arb_execution_mode(),
        arb_usage(),
        prop::collection::vec(arb_agent_event(), 0..5),
        arb_verification(),
    )
        .prop_map(
            |(run_id, wo_id, t1, t2, backend, outcome, mode, usage, trace, verification)| {
                let (started, finished) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
                let dur = (finished - started).num_milliseconds().max(0) as u64;
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
                    capabilities: CapabilityManifest::new(),
                    mode,
                    usage_raw: serde_json::json!({}),
                    usage,
                    trace,
                    artifacts: vec![],
                    verification,
                    outcome,
                    receipt_sha256: None,
                }
            },
        )
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        arb_uuid(),
        arb_safe_string(),
        arb_execution_lane(),
        arb_workspace_mode(),
        arb_safe_string(),
    )
        .prop_map(|(id, task, lane, ws_mode, root)| WorkOrder {
            id,
            task,
            lane,
            workspace: WorkspaceSpec {
                root,
                mode: ws_mode,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        })
}

fn arb_envelope() -> impl Strategy<Value = Envelope> {
    prop_oneof![
        arb_backend_identity().prop_map(|bi| Envelope::hello(bi, CapabilityManifest::new())),
        (arb_safe_string(), arb_work_order()).prop_map(|(id, wo)| Envelope::Run {
            id,
            work_order: wo,
        }),
        (arb_safe_string(), arb_agent_event())
            .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event }),
        (arb_safe_string(), arb_safe_string())
            .prop_map(|(ref_id, error)| Envelope::Fatal {
                ref_id: Some(ref_id),
                error,
            }),
    ]
}

fn arb_protocol_version() -> impl Strategy<Value = ProtocolVersion> {
    (0u32..10, 0u32..20).prop_map(|(major, minor)| ProtocolVersion { major, minor })
}

// ---------------------------------------------------------------------------
// 1. Hash determinism: receipt_hash(r) == receipt_hash(r)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn hash_determinism(receipt in arb_receipt()) {
        let h1 = receipt_hash(&receipt).unwrap();
        let h2 = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(h1, h2, "hash must be deterministic");
    }
}

// ---------------------------------------------------------------------------
// 2. Hash self-exclusion: hash field is always null in hash computation
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn hash_self_exclusion(receipt in arb_receipt()) {
        // Hash with receipt_sha256 = None
        let h_none = receipt_hash(&receipt).unwrap();

        // Set receipt_sha256 to some value, hash should still be the same
        let mut receipt_with_hash = receipt.clone();
        receipt_with_hash.receipt_sha256 = Some("decafbad".to_string());
        let h_some = receipt_hash(&receipt_with_hash).unwrap();

        prop_assert_eq!(h_none, h_some,
            "receipt_sha256 must not influence the hash");
    }
}

// ---------------------------------------------------------------------------
// 3. Serde roundtrip preservation: serialize → deserialize == identity
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn work_order_serde_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let decoded: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, decoded.id);
        prop_assert_eq!(&wo.task, &decoded.task);
    }

    #[test]
    fn receipt_serde_roundtrip(receipt in arb_receipt()) {
        let json = serde_json::to_string(&receipt).unwrap();
        let decoded: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(receipt.meta.run_id, decoded.meta.run_id);
        prop_assert_eq!(&receipt.backend.id, &decoded.backend.id);
        prop_assert_eq!(&receipt.outcome, &decoded.outcome);
        prop_assert_eq!(receipt.trace.len(), decoded.trace.len());
    }
}

// ---------------------------------------------------------------------------
// 4. Envelope roundtrip: Envelope → JSONL → parse → same Envelope
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn envelope_jsonl_roundtrip(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        prop_assert!(encoded.ends_with('\n'), "JSONL line must end with newline");
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

        // Verify the discriminant tag is preserved
        let orig_json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        let rt_json: serde_json::Value = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(&orig_json["t"], &rt_json["t"],
            "envelope tag must survive roundtrip");
    }
}

// ---------------------------------------------------------------------------
// 5. Version ordering: antisymmetry — if a < b then !(b < a)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn version_ordering_antisymmetry(a in arb_protocol_version(), b in arb_protocol_version()) {
        if a < b {
            prop_assert!(b >= a, "antisymmetry violated: a < b but b < a");
        }
        if a == b {
            prop_assert!(a >= b, "equal versions must not be ordered");
        }
    }
}

// ---------------------------------------------------------------------------
// 6. Version compatibility: reflexive — v is compatible with v
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn version_compatibility_reflexive(v in arb_protocol_version()) {
        prop_assert!(v.is_compatible(&v),
            "a version must be compatible with itself");
    }
}

// ---------------------------------------------------------------------------
// 7. Chain ordering: push preserves insertion order
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn chain_preserves_insertion_order(receipts in prop::collection::vec(arb_receipt(), 1..6)) {
        let mut chain = ReceiptChain::new();
        let mut ids = Vec::new();

        for r in receipts {
            let hashed = r.with_hash().unwrap();
            let id = hashed.meta.run_id;
            // IDs are Uuid::from_u128 from random u128, so duplicates are
            // extremely unlikely. If push fails due to duplicate, skip it.
            if chain.push(hashed).is_ok() {
                ids.push(id);
            }
        }

        let chain_ids: Vec<Uuid> = chain.iter().map(|r| r.meta.run_id).collect();
        prop_assert_eq!(ids, chain_ids, "insertion order must be preserved");
    }
}

// ---------------------------------------------------------------------------
// 8. EventStream filter: filtered stream ⊆ original stream
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn filtered_stream_is_subset(events in prop::collection::vec(arb_agent_event(), 0..10)) {
        let stream = EventStream::new(events);
        let filter = EventFilter::include_kinds(&["warning", "error"]);
        let filtered = stream.filter(&filter);

        // Every event in the filtered stream must pass the filter
        for event in filtered.iter() {
            prop_assert!(filter.matches(event),
                "filtered event must match the filter");
        }
    }
}

// ---------------------------------------------------------------------------
// 9. EventStream length: filter(events).len() <= events.len()
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn filtered_stream_length(events in prop::collection::vec(arb_agent_event(), 0..10)) {
        let stream = EventStream::new(events);
        let original_len = stream.len();

        let filter = EventFilter::include_kinds(&["warning"]);
        let filtered = stream.filter(&filter);

        prop_assert!(filtered.len() <= original_len,
            "filtered length {} must be <= original length {}",
            filtered.len(), original_len);
    }
}

// ---------------------------------------------------------------------------
// 10. Receipt chain verification: valid chain always passes verify()
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn valid_chain_passes_verify(receipts in prop::collection::vec(arb_receipt(), 1..5)) {
        let mut chain = ReceiptChain::new();
        for r in receipts {
            let hashed = r.with_hash().unwrap();
            // Skip duplicates
            let _ = chain.push(hashed);
        }
        if !chain.is_empty() {
            prop_assert!(chain.verify().is_ok(),
                "a chain of correctly hashed receipts must verify");
        }
    }
}

// ---------------------------------------------------------------------------
// 11. WorkOrderBuilder: builder always produces valid WorkOrder
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn work_order_builder_produces_valid(
        task in arb_safe_string(),
        root in arb_safe_string(),
        model in arb_safe_string(),
        turns in 1u32..100,
    ) {
        let wo = WorkOrderBuilder::new(task.clone())
            .root(root.clone())
            .model(model.clone())
            .max_turns(turns)
            .build();

        prop_assert_eq!(&wo.task, &task);
        prop_assert_eq!(&wo.workspace.root, &root);
        prop_assert_eq!(wo.config.model.as_deref(), Some(model.as_str()));
        prop_assert_eq!(wo.config.max_turns, Some(turns));
        // UUID must be non-nil (v4 random)
        prop_assert_ne!(wo.id, Uuid::nil());
    }
}

// ---------------------------------------------------------------------------
// 12. ReceiptBuilder: builder produces hashable receipt
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn receipt_builder_produces_hashable(
        backend_id in arb_safe_string(),
        outcome in arb_outcome(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id.clone())
            .outcome(outcome.clone())
            .build();

        // Must be hashable without error
        let hash = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(hash.len(), 64, "SHA-256 hex digest must be 64 chars");

        // with_hash must set the field
        let hashed = receipt.with_hash().unwrap();
        prop_assert!(hashed.receipt_sha256.is_some());
        prop_assert_eq!(&hashed.backend.id, &backend_id);
        prop_assert_eq!(&hashed.outcome, &outcome);
    }
}

// ---------------------------------------------------------------------------
// 13. PolicyEngine consistency: same input → same decision
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn policy_engine_deterministic(
        tool in "[A-Za-z][A-Za-z0-9]{0,11}",
        path_seg in "[a-z][a-z0-9]{0,7}",
    ) {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into()],
            disallowed_tools: vec!["Bash*".into()],
            deny_read: vec!["secret/**".into()],
            deny_write: vec!["locked/**".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        let d1 = engine.can_use_tool(&tool).allowed;
        let d2 = engine.can_use_tool(&tool).allowed;
        prop_assert_eq!(d1, d2, "tool decision must be deterministic");

        let p = format!("dir/{path_seg}.txt");
        let r1 = engine.can_read_path(Path::new(&p)).allowed;
        let r2 = engine.can_read_path(Path::new(&p)).allowed;
        prop_assert_eq!(r1, r2, "read decision must be deterministic");

        let w1 = engine.can_write_path(Path::new(&p)).allowed;
        let w2 = engine.can_write_path(Path::new(&p)).allowed;
        prop_assert_eq!(w1, w2, "write decision must be deterministic");
    }
}

// ---------------------------------------------------------------------------
// 14. JSONL codec: encode(decode(s)) preserves valid lines
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn jsonl_codec_roundtrip(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        let re_encoded = JsonlCodec::encode(&decoded).unwrap();

        // Decode the re-encoded value to compare structurally
        let v1: serde_json::Value = serde_json::from_str(encoded.trim()).unwrap();
        let v2: serde_json::Value = serde_json::from_str(re_encoded.trim()).unwrap();
        prop_assert_eq!(v1, v2, "encode(decode(line)) must preserve content");
    }
}

// ---------------------------------------------------------------------------
// 15. UTF-8 safety: all serialized output is valid UTF-8
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn serialized_output_is_valid_utf8(receipt in arb_receipt()) {
        let json = serde_json::to_string(&receipt).unwrap();
        // String type in Rust guarantees UTF-8, but let's verify the bytes too
        prop_assert!(std::str::from_utf8(json.as_bytes()).is_ok(),
            "receipt JSON must be valid UTF-8");
    }

    #[test]
    fn envelope_output_is_valid_utf8(envelope in arb_envelope()) {
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        prop_assert!(std::str::from_utf8(encoded.as_bytes()).is_ok(),
            "envelope JSONL must be valid UTF-8");
    }

    #[test]
    fn work_order_output_is_valid_utf8(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        prop_assert!(std::str::from_utf8(json.as_bytes()).is_ok(),
            "work order JSON must be valid UTF-8");
    }
}

// ---------------------------------------------------------------------------
// 16. Version negotiation: same-major negotiation succeeds
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn version_negotiation_same_major(major in 0u32..10, m1 in 0u32..20, m2 in 0u32..20) {
        let v1 = ProtocolVersion { major, minor: m1 };
        let v2 = ProtocolVersion { major, minor: m2 };

        let result = negotiate_version(&v1, &v2);
        prop_assert!(result.is_ok(), "same-major versions must negotiate");

        let negotiated = result.unwrap();
        prop_assert_eq!(negotiated.major, major);
        prop_assert_eq!(negotiated.minor, m1.min(m2),
            "negotiated version must be the minimum minor");
    }
}

// ---------------------------------------------------------------------------
// 17. Version negotiation: different-major negotiation fails
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn version_negotiation_different_major(
        maj1 in 0u32..10,
        maj2 in 0u32..10,
        min1 in 0u32..20,
        min2 in 0u32..20,
    ) {
        prop_assume!(maj1 != maj2);
        let v1 = ProtocolVersion { major: maj1, minor: min1 };
        let v2 = ProtocolVersion { major: maj2, minor: min2 };

        let result = negotiate_version(&v1, &v2);
        prop_assert!(result.is_err(),
            "different-major versions must fail negotiation");
    }
}

// ---------------------------------------------------------------------------
// 18. Hash length invariant: always exactly 64 hex characters
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn hash_is_64_hex_chars(receipt in arb_receipt()) {
        let hash = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(hash.len(), 64, "SHA-256 hex digest must be 64 characters");
        prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must contain only hex digits");
    }
}

// ---------------------------------------------------------------------------
// 19. EventStream exclude filter: complement of include
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn exclude_filter_complement(events in prop::collection::vec(arb_agent_event(), 0..10)) {
        let stream = EventStream::new(events);
        let kinds = &["warning", "error"];

        let include = EventFilter::include_kinds(kinds);
        let exclude = EventFilter::exclude_kinds(kinds);

        let included = stream.filter(&include);
        let excluded = stream.filter(&exclude);

        // The two partitions should cover the entire stream
        prop_assert_eq!(
            included.len() + excluded.len(),
            stream.len(),
            "include + exclude must partition the stream"
        );
    }
}
