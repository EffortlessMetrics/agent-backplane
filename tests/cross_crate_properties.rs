// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-crate property-based tests verifying invariants spanning multiple crates.

use std::path::Path;

use abp_cli::config::{merge_configs, BackplaneConfig};
use abp_core::chain::ReceiptChain;
use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{
    canonical_json, receipt_hash, AgentEvent, AgentEventKind, BackendIdentity,
    CapabilityManifest, CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode,
    MinSupport, Outcome, PolicyProfile, Receipt, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_policy::PolicyEngine;
use abp_protocol::version::{negotiate_version, ProtocolVersion};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Arbitrary strategies for contract types
// ---------------------------------------------------------------------------

fn arb_safe_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_.-]{1,20}"
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![Just(ExecutionMode::Passthrough), Just(ExecutionMode::Mapped)]
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

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    (1_577_836_800i64..1_893_456_000i64)
        .prop_map(|secs| Utc.timestamp_opt(secs, 0).single().unwrap())
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
        (arb_safe_string(), arb_safe_string()).prop_map(|(p, s)| AgentEventKind::FileChanged {
            path: p,
            summary: s,
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

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (
        any::<u128>(),
        any::<u128>(),
        arb_outcome(),
        arb_datetime(),
        arb_datetime(),
        arb_backend_identity(),
        arb_execution_mode(),
        arb_usage(),
        prop::collection::vec(arb_agent_event(), 0..3),
    )
        .prop_map(
            |(run_id, wo_id, outcome, t1, t2, backend, mode, usage, trace)| {
                let (started, finished) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
                let dur = (finished - started).num_milliseconds().max(0) as u64;
                Receipt {
                    meta: RunMetadata {
                        run_id: Uuid::from_u128(run_id),
                        work_order_id: Uuid::from_u128(wo_id),
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
                    verification: VerificationReport::default(),
                    outcome,
                    receipt_sha256: None,
                }
            },
        )
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        any::<u128>(),
        arb_safe_string(),
        prop_oneof![
            Just(ExecutionLane::PatchFirst),
            Just(ExecutionLane::WorkspaceFirst),
        ],
        prop_oneof![
            Just(WorkspaceMode::PassThrough),
            Just(WorkspaceMode::Staged),
        ],
        arb_safe_string(),
    )
        .prop_map(|(id, task, lane, ws_mode, root)| WorkOrder {
            id: Uuid::from_u128(id),
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
        (arb_safe_string(), arb_safe_string()).prop_map(|(ref_id, error)| Envelope::Fatal {
            ref_id: Some(ref_id),
            error,
        }),
    ]
}

// ---------------------------------------------------------------------------
// 1. WorkOrder→Envelope→WorkOrder: Any work order survives protocol
//    envelope wrapping/unwrapping
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn work_order_envelope_roundtrip(wo in arb_work_order()) {
        let original = serde_json::to_value(&wo).unwrap();
        let envelope = Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        };
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

        if let Envelope::Run { work_order, .. } = decoded {
            let roundtripped = serde_json::to_value(&work_order).unwrap();
            prop_assert_eq!(original, roundtripped,
                "WorkOrder must survive Envelope wrapping/unwrapping");
        } else {
            prop_assert!(false, "decoded envelope should be Run variant");
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Receipt hash determinism: Same receipt content always produces same hash
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn receipt_hash_determinism(receipt in arb_receipt()) {
        let h1 = receipt_hash(&receipt).unwrap();
        let h2 = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(h1, h2, "hash must be deterministic");
    }
}

// ---------------------------------------------------------------------------
// 3. Receipt hash sensitivity: Changing any field changes the hash
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn receipt_hash_sensitivity(receipt in arb_receipt()) {
        let h_original = receipt_hash(&receipt).unwrap();

        let mut modified = receipt.clone();
        modified.outcome = match modified.outcome {
            Outcome::Complete => Outcome::Failed,
            Outcome::Partial => Outcome::Complete,
            Outcome::Failed => Outcome::Partial,
        };
        let h_modified = receipt_hash(&modified).unwrap();
        prop_assert_ne!(h_original, h_modified,
            "changing outcome must change the hash");
    }
}

// ---------------------------------------------------------------------------
// 4. PolicyProfile→PolicyEngine→check roundtrip: Policy decisions are
//    consistent with profile
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn policy_profile_engine_consistency(
        denied_tool in "[A-Za-z]{3,8}",
        other_tool in "[A-Za-z]{3,8}",
    ) {
        prop_assume!(denied_tool != other_tool);
        let policy = PolicyProfile {
            disallowed_tools: vec![denied_tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        // A tool in the deny list must be denied
        prop_assert!(!engine.can_use_tool(&denied_tool).allowed,
            "denied tool '{}' should not be allowed", denied_tool);
        // A different tool (with no allowlist) should be allowed
        prop_assert!(engine.can_use_tool(&other_tool).allowed,
            "tool '{}' should be allowed when not denied", other_tool);
    }
}

// ---------------------------------------------------------------------------
// 5. Glob compilation idempotence: Compiling same globs twice produces
//    same decisions
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn glob_compilation_idempotence(
        prefix in "[a-z]{1,5}",
        filename in "[a-z]{1,5}",
    ) {
        let include_pat = format!("{prefix}/**");
        let candidate = format!("{prefix}/{filename}.txt");

        let globs1 = IncludeExcludeGlobs::new(&[include_pat.clone()], &[]).unwrap();
        let globs2 = IncludeExcludeGlobs::new(&[include_pat], &[]).unwrap();

        prop_assert_eq!(
            globs1.decide_str(&candidate),
            globs2.decide_str(&candidate),
            "compiling identical globs must yield identical decisions"
        );
    }
}

// ---------------------------------------------------------------------------
// 6. Envelope serialization size: Envelope JSON size is bounded relative
//    to content size
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn envelope_size_bounded(envelope in arb_envelope()) {
        let full_json = JsonlCodec::encode(&envelope).unwrap();
        let content_size = match &envelope {
            Envelope::Run { work_order, .. } => serde_json::to_string(work_order).unwrap().len(),
            Envelope::Event { event, .. } => serde_json::to_string(event).unwrap().len(),
            Envelope::Final { receipt, .. } => serde_json::to_string(receipt).unwrap().len(),
            Envelope::Hello { backend, .. } => serde_json::to_string(backend).unwrap().len(),
            Envelope::Fatal { error, .. } => error.len(),
        };
        prop_assert!(
            full_json.len() >= content_size,
            "envelope ({} bytes) must be >= content ({} bytes)",
            full_json.len(),
            content_size
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Event ordering preserved: Events in order through stream→filter→collect
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn event_order_preserved(events in prop::collection::vec(arb_agent_event(), 0..10)) {
        let stream = EventStream::new(events.clone());
        let filter = EventFilter::include_kinds(&["warning", "error"]);
        let filtered = stream.filter(&filter);

        let expected: Vec<String> = events
            .iter()
            .filter(|e| filter.matches(e))
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        let actual: Vec<String> = filtered
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();

        prop_assert_eq!(expected, actual, "filter must preserve event order");
    }
}

// ---------------------------------------------------------------------------
// 8. Capability satisfaction transitivity: If a level satisfies Native,
//    it must also satisfy Emulated (monotonicity)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn capability_satisfaction_monotone(level_idx in 0u32..4) {
        let level = match level_idx {
            0 => SupportLevel::Native,
            1 => SupportLevel::Emulated,
            2 => SupportLevel::Restricted { reason: "test".into() },
            _ => SupportLevel::Unsupported,
        };

        if level.satisfies(&MinSupport::Native) {
            prop_assert!(
                level.satisfies(&MinSupport::Emulated),
                "satisfying Native must imply satisfying Emulated (transitivity)"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 9. Version negotiation reflexivity: negotiate(v, v) == Ok(v)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn version_negotiation_reflexive(major in 0u32..10, minor in 0u32..20) {
        let v = ProtocolVersion { major, minor };
        let result = negotiate_version(&v, &v).unwrap();
        prop_assert_eq!(result, v, "negotiate(v, v) must equal v");
    }
}

// ---------------------------------------------------------------------------
// 10. Version negotiation symmetry: negotiate(a, b) == negotiate(b, a)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn version_negotiation_symmetric(
        maj1 in 0u32..10, min1 in 0u32..20,
        maj2 in 0u32..10, min2 in 0u32..20,
    ) {
        let v1 = ProtocolVersion { major: maj1, minor: min1 };
        let v2 = ProtocolVersion { major: maj2, minor: min2 };

        let r1 = negotiate_version(&v1, &v2);
        let r2 = negotiate_version(&v2, &v1);

        match (r1, r2) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b, "negotiation must be symmetric"),
            (Err(_), Err(_)) => {} // both fail — symmetric
            _ => prop_assert!(false, "negotiation symmetry violated: one succeeded, other failed"),
        }
    }
}

// ---------------------------------------------------------------------------
// 11. Config merge associativity: merge(a, merge(b, c)) == merge(merge(a, b), c)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn config_merge_associative(
        a_default in proptest::option::of("[a-z]{1,5}"),
        b_default in proptest::option::of("[a-z]{1,5}"),
        c_default in proptest::option::of("[a-z]{1,5}"),
        a_log in proptest::option::of("[a-z]{1,5}"),
        b_log in proptest::option::of("[a-z]{1,5}"),
        c_log in proptest::option::of("[a-z]{1,5}"),
    ) {
        let a = BackplaneConfig {
            default_backend: a_default.clone(),
            log_level: a_log.clone(),
            ..Default::default()
        };
        let b = BackplaneConfig {
            default_backend: b_default.clone(),
            log_level: b_log.clone(),
            ..Default::default()
        };
        let c = BackplaneConfig {
            default_backend: c_default.clone(),
            log_level: c_log.clone(),
            ..Default::default()
        };

        let left = merge_configs(a.clone(), merge_configs(b.clone(), c.clone()));
        let right = merge_configs(merge_configs(a, b), c);

        let left_val = serde_json::to_value(&left).unwrap();
        let right_val = serde_json::to_value(&right).unwrap();
        prop_assert_eq!(left_val, right_val, "config merge must be associative");
    }
}

// ---------------------------------------------------------------------------
// 12. ReceiptChain monotonicity: Chain length always increases with append
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn receipt_chain_monotonic(receipts in prop::collection::vec(arb_receipt(), 1..6)) {
        let mut chain = ReceiptChain::new();
        let mut prev_len = 0;

        for r in receipts {
            let hashed = r.with_hash().unwrap();
            if chain.push(hashed).is_ok() {
                let new_len = chain.len();
                prop_assert!(
                    new_len > prev_len,
                    "chain length must strictly increase after successful push"
                );
                prev_len = new_len;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 13. Serde JSON roundtrip for all contract types
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn serde_roundtrip_work_order(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let decoded: WorkOrder = serde_json::from_str(&json).unwrap();
        let v1 = serde_json::to_value(&wo).unwrap();
        let v2 = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(v1, v2, "WorkOrder must survive JSON roundtrip");
    }

    #[test]
    fn serde_roundtrip_receipt(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let decoded: Receipt = serde_json::from_str(&json).unwrap();
        let v1 = serde_json::to_value(&r).unwrap();
        let v2 = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(v1, v2, "Receipt must survive JSON roundtrip");
    }

    #[test]
    fn serde_roundtrip_agent_event(e in arb_agent_event()) {
        let json = serde_json::to_string(&e).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        let v1 = serde_json::to_value(&e).unwrap();
        let v2 = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(v1, v2, "AgentEvent must survive JSON roundtrip");
    }

    #[test]
    fn serde_roundtrip_backend_identity(bi in arb_backend_identity()) {
        let json = serde_json::to_string(&bi).unwrap();
        let decoded: BackendIdentity = serde_json::from_str(&json).unwrap();
        let v1 = serde_json::to_value(&bi).unwrap();
        let v2 = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(v1, v2, "BackendIdentity must survive JSON roundtrip");
    }
}

// ---------------------------------------------------------------------------
// 14. Policy deny overrides allow: If both match, deny wins
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn policy_deny_overrides_allow(tool in "[A-Za-z]{3,8}") {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(
            !engine.can_use_tool(&tool).allowed,
            "deny must override allow for tool '{}'",
            tool
        );
    }
}

// ---------------------------------------------------------------------------
// 15. Canonical JSON determinism: Multiple serializations of same value
//     are byte-identical
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn canonical_json_deterministic(receipt in arb_receipt()) {
        let j1 = canonical_json(&receipt).unwrap();
        let j2 = canonical_json(&receipt).unwrap();
        prop_assert_eq!(
            j1.as_bytes(),
            j2.as_bytes(),
            "canonical JSON must be byte-identical across calls"
        );
    }
}

// ---------------------------------------------------------------------------
// 16. Deny-write policy spans glob+policy crates: paths matching deny_write
//     globs are denied by PolicyEngine
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn deny_write_policy_glob_integration(
        dir in "[a-z]{1,5}",
        file in "[a-z]{1,5}",
    ) {
        let deny_pattern = format!("{dir}/**");
        let policy = PolicyProfile {
            deny_write: vec![deny_pattern.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        // A path matching the deny glob must be denied
        let denied_path = format!("{dir}/{file}.txt");
        prop_assert!(
            !engine.can_write_path(Path::new(&denied_path)).allowed,
            "path '{}' matching deny_write '{}' must be denied",
            denied_path,
            deny_pattern
        );

        // Verify the raw glob agrees with the policy engine
        let raw_globs = IncludeExcludeGlobs::new(&[], &[deny_pattern]).unwrap();
        let glob_decision = raw_globs.decide_str(&denied_path);
        prop_assert_eq!(
            glob_decision,
            MatchDecision::DeniedByExclude,
            "raw glob must agree with PolicyEngine"
        );
    }
}

// ---------------------------------------------------------------------------
// 17. Envelope→Receipt roundtrip through Final variant
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn receipt_envelope_roundtrip(receipt in arb_receipt()) {
        let original_val = serde_json::to_value(&receipt).unwrap();
        let envelope = Envelope::Final {
            ref_id: receipt.meta.run_id.to_string(),
            receipt,
        };
        let encoded = JsonlCodec::encode(&envelope).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

        if let Envelope::Final { receipt: rt_receipt, .. } = decoded {
            let roundtripped_val = serde_json::to_value(&rt_receipt).unwrap();
            prop_assert_eq!(original_val, roundtripped_val,
                "Receipt must survive Envelope::Final wrapping/unwrapping");
        } else {
            prop_assert!(false, "decoded envelope should be Final variant");
        }
    }
}

// ---------------------------------------------------------------------------
// 18. Glob exclude + PolicyEngine deny_read agreement: raw glob exclude
//     and policy deny_read produce consistent results
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn deny_read_policy_glob_agreement(
        dir in "[a-z]{1,5}",
        file in "[a-z]{1,5}",
    ) {
        let deny_pattern = format!("{dir}/**");
        let policy = PolicyProfile {
            deny_read: vec![deny_pattern.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();

        let test_path = format!("{dir}/{file}.rs");
        let engine_denied = !engine.can_read_path(Path::new(&test_path)).allowed;

        let raw_globs = IncludeExcludeGlobs::new(&[], &[deny_pattern]).unwrap();
        let glob_denied = raw_globs.decide_str(&test_path) == MatchDecision::DeniedByExclude;

        prop_assert_eq!(
            engine_denied, glob_denied,
            "PolicyEngine deny_read and raw glob must agree for path '{}'",
            test_path
        );
    }
}
