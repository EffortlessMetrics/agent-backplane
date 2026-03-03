// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for ABP core types.

use proptest::prelude::*;
use std::path::Path;

use abp_core::negotiate::{CapabilityNegotiator, NegotiationRequest};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig,
    SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, canonical_json,
    receipt_hash,
};
use abp_policy::PolicyEngine;
use abp_protocol::{Envelope, JsonlCodec};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn nonempty_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{1,32}".prop_map(|s| s.to_string())
}

fn arb_printable() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.,!?/-]{0,64}".prop_map(|s| s.to_string())
}

fn arb_execution_lane() -> impl Strategy<Value = ExecutionLane> {
    prop_oneof![
        Just(ExecutionLane::PatchFirst),
        Just(ExecutionLane::WorkspaceFirst),
    ]
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![
        Just(ExecutionMode::Passthrough),
        Just(ExecutionMode::Mapped),
    ]
}

fn arb_workspace_mode() -> impl Strategy<Value = WorkspaceMode> {
    prop_oneof![
        Just(WorkspaceMode::PassThrough),
        Just(WorkspaceMode::Staged),
    ]
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
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
        Just(Capability::McpClient),
        Just(Capability::McpServer),
        Just(Capability::ToolUse),
        Just(Capability::ExtendedThinking),
        Just(Capability::ImageInput),
    ]
}

fn arb_support_level() -> impl Strategy<Value = SupportLevel> {
    prop_oneof![
        Just(SupportLevel::Native),
        Just(SupportLevel::Emulated),
        Just(SupportLevel::Unsupported),
        nonempty_string().prop_map(|r| SupportLevel::Restricted { reason: r }),
    ]
}

fn arb_min_support() -> impl Strategy<Value = MinSupport> {
    prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated),]
}

fn arb_capability_manifest() -> impl Strategy<Value = CapabilityManifest> {
    prop::collection::btree_map(arb_capability(), arb_support_level(), 0..6)
}

fn arb_backend_identity() -> impl Strategy<Value = BackendIdentity> {
    (
        nonempty_string(),
        prop::option::of(nonempty_string()),
        prop::option::of(nonempty_string()),
    )
        .prop_map(|(id, bv, av)| BackendIdentity {
            id,
            backend_version: bv,
            adapter_version: av,
        })
}

fn arb_policy_profile() -> impl Strategy<Value = PolicyProfile> {
    (
        prop::collection::vec(nonempty_string(), 0..4),
        prop::collection::vec(nonempty_string(), 0..4),
        prop::collection::vec(nonempty_string(), 0..3),
        prop::collection::vec(nonempty_string(), 0..3),
    )
        .prop_map(|(at, dt, dr, dw)| PolicyProfile {
            allowed_tools: at,
            disallowed_tools: dt,
            deny_read: dr,
            deny_write: dw,
            ..PolicyProfile::default()
        })
}

fn arb_context_packet() -> impl Strategy<Value = ContextPacket> {
    (
        prop::collection::vec(nonempty_string(), 0..4),
        prop::collection::vec(
            (nonempty_string(), arb_printable())
                .prop_map(|(name, content)| ContextSnippet { name, content }),
            0..3,
        ),
    )
        .prop_map(|(files, snippets)| ContextPacket { files, snippets })
}

fn arb_runtime_config() -> impl Strategy<Value = RuntimeConfig> {
    (
        prop::option::of(nonempty_string()),
        prop::collection::btree_map(nonempty_string(), Just(serde_json::json!("v")), 0..3),
        prop::collection::btree_map(nonempty_string(), nonempty_string(), 0..3),
        prop::option::of(1.0..1000.0_f64),
        prop::option::of(1u32..100),
    )
        .prop_map(|(model, vendor, env, budget, turns)| RuntimeConfig {
            model,
            vendor,
            env,
            max_budget_usd: budget,
            max_turns: turns,
        })
}

fn arb_workspace_spec() -> impl Strategy<Value = WorkspaceSpec> {
    (
        nonempty_string(),
        arb_workspace_mode(),
        prop::collection::vec(nonempty_string(), 0..3),
        prop::collection::vec(nonempty_string(), 0..3),
    )
        .prop_map(|(root, mode, include, exclude)| WorkspaceSpec {
            root,
            mode,
            include,
            exclude,
        })
}

fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        nonempty_string().prop_map(|m| AgentEventKind::RunStarted { message: m }),
        nonempty_string().prop_map(|m| AgentEventKind::RunCompleted { message: m }),
        nonempty_string().prop_map(|text| AgentEventKind::AssistantDelta { text }),
        nonempty_string().prop_map(|text| AgentEventKind::AssistantMessage { text }),
        (nonempty_string(), nonempty_string())
            .prop_map(|(path, summary)| { AgentEventKind::FileChanged { path, summary } }),
        nonempty_string().prop_map(|m| AgentEventKind::Warning { message: m }),
        nonempty_string().prop_map(|m| AgentEventKind::Error {
            message: m,
            error_code: None,
        }),
    ]
}

fn arb_agent_event() -> impl Strategy<Value = AgentEvent> {
    arb_agent_event_kind().prop_map(|kind| AgentEvent {
        ts: chrono::Utc::now(),
        kind,
        ext: None,
    })
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        nonempty_string(),
        arb_execution_lane(),
        arb_workspace_spec(),
        arb_context_packet(),
        arb_runtime_config(),
    )
        .prop_map(|(task, lane, workspace, context, config)| WorkOrder {
            id: uuid::Uuid::new_v4(),
            task,
            lane,
            workspace,
            context,
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config,
        })
}

fn arb_receipt() -> impl Strategy<Value = Receipt> {
    (arb_outcome(), nonempty_string(), arb_capability_manifest()).prop_map(
        |(outcome, backend_id, caps)| {
            ReceiptBuilder::new(backend_id)
                .outcome(outcome)
                .capabilities(caps)
                .build()
        },
    )
}

fn arb_artifact_ref() -> impl Strategy<Value = ArtifactRef> {
    (nonempty_string(), nonempty_string()).prop_map(|(kind, path)| ArtifactRef { kind, path })
}

fn arb_error_code() -> impl Strategy<Value = abp_error::ErrorCode> {
    prop_oneof![
        Just(abp_error::ErrorCode::ProtocolInvalidEnvelope),
        Just(abp_error::ErrorCode::BackendNotFound),
        Just(abp_error::ErrorCode::BackendTimeout),
        Just(abp_error::ErrorCode::PolicyDenied),
        Just(abp_error::ErrorCode::CapabilityUnsupported),
        Just(abp_error::ErrorCode::Internal),
        Just(abp_error::ErrorCode::ReceiptHashMismatch),
        Just(abp_error::ErrorCode::ConfigInvalid),
    ]
}

fn arb_envelope() -> impl Strategy<Value = Envelope> {
    prop_oneof![
        // Hello
        (
            arb_backend_identity(),
            arb_capability_manifest(),
            arb_execution_mode()
        )
            .prop_map(|(backend, caps, mode)| { Envelope::hello_with_mode(backend, caps, mode) }),
        // Run
        (nonempty_string(), arb_work_order())
            .prop_map(|(id, wo)| Envelope::Run { id, work_order: wo }),
        // Event
        (nonempty_string(), arb_agent_event())
            .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event }),
        // Fatal
        (
            prop::option::of(nonempty_string()),
            nonempty_string(),
            prop::option::of(arb_error_code()),
        )
            .prop_map(|(ref_id, error, error_code)| Envelope::Fatal {
                ref_id,
                error,
                error_code,
            }),
    ]
}

// ===========================================================================
// 1. Serialization roundtrip properties (17 tests)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // --- WorkOrder ---

    #[test]
    fn work_order_serde_roundtrip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let back: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.id, back.id);
        prop_assert_eq!(&wo.task, &back.task);
    }

    #[test]
    fn work_order_json_is_utf8(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        // String in Rust is always valid UTF-8, but verify bytes round-trip.
        let bytes = json.as_bytes();
        prop_assert!(std::str::from_utf8(bytes).is_ok());
    }

    #[test]
    fn work_order_builder_preserves_task(task in nonempty_string()) {
        let wo = WorkOrderBuilder::new(&task).build();
        prop_assert_eq!(&wo.task, &task);
    }

    // --- Receipt ---

    #[test]
    fn receipt_serde_roundtrip(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        let back: Receipt = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(r.meta.run_id, back.meta.run_id);
        prop_assert_eq!(&r.backend.id, &back.backend.id);
        prop_assert_eq!(&r.outcome, &back.outcome);
    }

    #[test]
    fn receipt_json_is_utf8(r in arb_receipt()) {
        let json = serde_json::to_string(&r).unwrap();
        prop_assert!(std::str::from_utf8(json.as_bytes()).is_ok());
    }

    // --- AgentEvent ---

    #[test]
    fn agent_event_serde_roundtrip(ev in arb_agent_event()) {
        let json = serde_json::to_string(&ev).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(ev.ts.timestamp(), back.ts.timestamp());
    }

    #[test]
    fn agent_event_json_is_utf8(ev in arb_agent_event()) {
        let json = serde_json::to_string(&ev).unwrap();
        prop_assert!(std::str::from_utf8(json.as_bytes()).is_ok());
    }

    // --- Envelope ---

    #[test]
    fn envelope_serde_roundtrip(env in arb_envelope()) {
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        // Verify the tag field survived.
        let v_orig: serde_json::Value = serde_json::to_value(&env).unwrap();
        let v_back: serde_json::Value = serde_json::to_value(&back).unwrap();
        prop_assert_eq!(&v_orig["t"], &v_back["t"]);
    }

    #[test]
    fn envelope_jsonl_roundtrip(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        prop_assert!(line.ends_with('\n'));
        let back = JsonlCodec::decode(line.trim()).unwrap();
        let v_orig: serde_json::Value = serde_json::to_value(&env).unwrap();
        let v_back: serde_json::Value = serde_json::to_value(&back).unwrap();
        prop_assert_eq!(&v_orig["t"], &v_back["t"]);
    }

    #[test]
    fn envelope_json_is_utf8(env in arb_envelope()) {
        let json = serde_json::to_string(&env).unwrap();
        prop_assert!(std::str::from_utf8(json.as_bytes()).is_ok());
    }

    // --- Capability manifest ---

    #[test]
    fn capability_manifest_serde_roundtrip(manifest in arb_capability_manifest()) {
        let json = serde_json::to_string(&manifest).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(manifest.len(), back.len());
        for cap in manifest.keys() {
            prop_assert!(back.contains_key(cap));
        }
    }

    // --- PolicyProfile ---

    #[test]
    fn policy_profile_serde_roundtrip(pp in arb_policy_profile()) {
        let json = serde_json::to_string(&pp).unwrap();
        let back: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&pp.allowed_tools, &back.allowed_tools);
        prop_assert_eq!(&pp.disallowed_tools, &back.disallowed_tools);
        prop_assert_eq!(&pp.deny_read, &back.deny_read);
        prop_assert_eq!(&pp.deny_write, &back.deny_write);
    }

    // --- BackendIdentity ---

    #[test]
    fn backend_identity_serde_roundtrip(bi in arb_backend_identity()) {
        let json = serde_json::to_string(&bi).unwrap();
        let back: BackendIdentity = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&bi.id, &back.id);
        prop_assert_eq!(&bi.backend_version, &back.backend_version);
    }

    // --- Canonical JSON determinism ---

    #[test]
    fn canonical_json_deterministic(wo in arb_work_order()) {
        let a = canonical_json(&wo).unwrap();
        let b = canonical_json(&wo).unwrap();
        prop_assert_eq!(&a, &b);
    }

    #[test]
    fn canonical_json_btreemap_deterministic(
        entries in prop::collection::btree_map(nonempty_string(), nonempty_string(), 0..8)
    ) {
        let a = canonical_json(&entries).unwrap();
        let b = canonical_json(&entries).unwrap();
        prop_assert_eq!(&a, &b);
    }

    // --- ArtifactRef ---

    #[test]
    fn artifact_ref_serde_roundtrip(ar in arb_artifact_ref()) {
        let json = serde_json::to_string(&ar).unwrap();
        let back: ArtifactRef = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&ar.kind, &back.kind);
        prop_assert_eq!(&ar.path, &back.path);
    }
}

// ===========================================================================
// 2. Receipt hashing properties (12 tests)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn receipt_hash_deterministic(r in arb_receipt()) {
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        prop_assert_eq!(&h1, &h2);
    }

    #[test]
    fn receipt_hash_is_valid_hex(r in arb_receipt()) {
        let h = receipt_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_sha256_none_before_hash(r in arb_receipt()) {
        prop_assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn receipt_sha256_some_after_with_hash(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert!(hashed.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_with_hash_length_64(r in arb_receipt()) {
        let hashed = r.with_hash().unwrap();
        prop_assert_eq!(hashed.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn receipt_hash_ignores_receipt_sha256_field(r in arb_receipt()) {
        let h1 = receipt_hash(&r).unwrap();
        let mut r2 = r.clone();
        r2.receipt_sha256 = Some("deadbeef".to_string());
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_eq!(&h1, &h2);
    }

    #[test]
    fn receipt_hash_changes_on_outcome_change(outcome_a in arb_outcome(), outcome_b in arb_outcome()) {
        let r1 = ReceiptBuilder::new("test").outcome(outcome_a.clone()).build();
        let r2 = ReceiptBuilder::new("test").outcome(outcome_b.clone()).build();
        if std::mem::discriminant(&outcome_a) != std::mem::discriminant(&outcome_b) {
            // Different outcomes should (very likely) produce different hashes,
            // but run_id differs too, so hashes will always differ.
            let h1 = receipt_hash(&r1).unwrap();
            let h2 = receipt_hash(&r2).unwrap();
            prop_assert_ne!(&h1, &h2);
        }
    }

    #[test]
    fn receipt_hash_changes_on_backend_change(a in nonempty_string(), b in nonempty_string()) {
        let r1 = ReceiptBuilder::new(&a).build();
        let r2 = ReceiptBuilder::new(&b).build();
        // Different run_ids guarantee different hashes.
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_ne!(&h1, &h2);
    }

    #[test]
    fn receipt_with_hash_is_idempotent_value(r in arb_receipt()) {
        let h1 = r.clone().with_hash().unwrap();
        let h2 = h1.clone().with_hash().unwrap();
        prop_assert_eq!(&h1.receipt_sha256, &h2.receipt_sha256);
    }

    #[test]
    fn receipt_hash_hex_lowercase(r in arb_receipt()) {
        let h = receipt_hash(&r).unwrap();
        prop_assert_eq!(&h, &h.to_lowercase());
    }

    #[test]
    fn receipt_hash_consistent_with_with_hash(r in arb_receipt()) {
        let manual = receipt_hash(&r).unwrap();
        let via_method = r.with_hash().unwrap();
        prop_assert_eq!(&manual, via_method.receipt_sha256.as_ref().unwrap());
    }

    #[test]
    fn receipt_hash_unique_per_run_id(backend in nonempty_string()) {
        // Two receipts with same backend but different auto-generated run_ids.
        let r1 = ReceiptBuilder::new(&backend).build();
        let r2 = ReceiptBuilder::new(&backend).build();
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_ne!(&h1, &h2);
    }
}

// ===========================================================================
// 3. Policy engine properties (12 tests)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn empty_policy_allows_any_tool(tool in nonempty_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    #[test]
    fn empty_policy_allows_read(path in nonempty_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_read_path(Path::new(&path)).allowed);
    }

    #[test]
    fn empty_policy_allows_write(path in nonempty_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }

    #[test]
    fn deny_overrides_allow_for_tools(tool in nonempty_string()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }

    #[test]
    fn allowlist_blocks_unlisted(tool in nonempty_string()) {
        // Use a different string for the allowlist so the tool is never in it.
        let policy = PolicyProfile {
            allowed_tools: vec!["ZZNOTEXIST_1234".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        if tool != "ZZNOTEXIST_1234" {
            prop_assert!(!engine.can_use_tool(&tool).allowed);
        }
    }

    #[test]
    fn deny_read_blocks_path(name in nonempty_string()) {
        let policy = PolicyProfile {
            deny_read: vec![format!("**/{name}")],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_read_path(Path::new(&name)).allowed);
    }

    #[test]
    fn deny_write_blocks_path(name in nonempty_string()) {
        let policy = PolicyProfile {
            deny_write: vec![format!("**/{name}")],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_write_path(Path::new(&name)).allowed);
    }

    #[test]
    fn policy_default_serde_roundtrip(_x in 0..1u8) {
        let pp = PolicyProfile::default();
        let json = serde_json::to_string(&pp).unwrap();
        let back: PolicyProfile = serde_json::from_str(&json).unwrap();
        prop_assert!(back.allowed_tools.is_empty());
        prop_assert!(back.disallowed_tools.is_empty());
    }

    #[test]
    fn wildcard_allow_permits_any(tool in nonempty_string()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    #[test]
    fn deny_decision_has_reason(tool in nonempty_string()) {
        let policy = PolicyProfile {
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool(&tool);
        if !decision.allowed {
            prop_assert!(decision.reason.is_some());
        }
    }

    #[test]
    fn allow_decision_has_no_reason(tool in nonempty_string()) {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        let decision = engine.can_use_tool(&tool);
        prop_assert!(decision.allowed);
        prop_assert!(decision.reason.is_none());
    }

    #[test]
    fn policy_deny_both_read_write(name in nonempty_string()) {
        let policy = PolicyProfile {
            deny_read: vec![format!("**/{name}")],
            deny_write: vec![format!("**/{name}")],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_read_path(Path::new(&name)).allowed);
        prop_assert!(!engine.can_write_path(Path::new(&name)).allowed);
    }
}

// ===========================================================================
// 4. Capability / SupportLevel properties (10 tests)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn native_satisfies_native(_x in 0..1u8) {
        prop_assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated(_x in 0..1u8) {
        prop_assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_does_not_satisfy_native(_x in 0..1u8) {
        prop_assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn emulated_satisfies_emulated(_x in 0..1u8) {
        prop_assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn unsupported_satisfies_nothing(min in arb_min_support()) {
        prop_assert!(!SupportLevel::Unsupported.satisfies(&min));
    }

    #[test]
    fn restricted_satisfies_emulated_min(reason in nonempty_string()) {
        let level = SupportLevel::Restricted { reason };
        prop_assert!(level.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_does_not_satisfy_native(reason in nonempty_string()) {
        let level = SupportLevel::Restricted { reason };
        prop_assert!(!level.satisfies(&MinSupport::Native));
    }

    #[test]
    fn capability_serde_roundtrip(cap in arb_capability()) {
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&cap, &back);
    }

    #[test]
    fn negotiation_satisfied_plus_unsatisfied_covers_required(
        caps in prop::collection::vec(arb_capability(), 1..6),
        manifest in arb_capability_manifest(),
    ) {
        let req = NegotiationRequest {
            required: caps.clone(),
            preferred: vec![],
            minimum_support: SupportLevel::Emulated,
        };
        let result = CapabilityNegotiator::negotiate(&req, &manifest);
        // Every required capability must appear in satisfied OR unsatisfied.
        for cap in &caps {
            let in_sat = result.satisfied.contains(cap);
            let in_unsat = result.unsatisfied.contains(cap);
            prop_assert!(in_sat || in_unsat);
        }
    }

    #[test]
    fn negotiation_compatible_iff_no_unsatisfied(
        caps in prop::collection::vec(arb_capability(), 0..4),
        manifest in arb_capability_manifest(),
    ) {
        let req = NegotiationRequest {
            required: caps,
            preferred: vec![],
            minimum_support: SupportLevel::Emulated,
        };
        let result = CapabilityNegotiator::negotiate(&req, &manifest);
        prop_assert_eq!(result.is_compatible, result.unsatisfied.is_empty());
    }
}

// ===========================================================================
// 5. WorkOrder invariants (8 tests)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn work_order_id_is_nonzero(wo in arb_work_order()) {
        prop_assert!(!wo.id.is_nil());
    }

    #[test]
    fn work_order_task_is_nonempty(task in nonempty_string()) {
        let wo = WorkOrderBuilder::new(&task).build();
        prop_assert!(!wo.task.is_empty());
    }

    #[test]
    fn work_order_builder_contract_version(_x in 0..1u8) {
        let wo = WorkOrderBuilder::new("test").build();
        let receipt = ReceiptBuilder::new("mock").work_order_id(wo.id).build();
        prop_assert_eq!(&receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn work_order_config_vendor_keys_nonempty(
        entries in prop::collection::btree_map(nonempty_string(), Just(serde_json::json!(1)), 0..5)
    ) {
        let config = RuntimeConfig {
            vendor: entries.clone(),
            ..RuntimeConfig::default()
        };
        let wo = WorkOrderBuilder::new("t").config(config).build();
        for key in wo.config.vendor.keys() {
            prop_assert!(!key.is_empty());
        }
    }

    #[test]
    fn work_order_unique_ids(_x in 0..16u8) {
        let a = WorkOrderBuilder::new("a").build();
        let b = WorkOrderBuilder::new("b").build();
        prop_assert_ne!(a.id, b.id);
    }

    #[test]
    fn work_order_model_preserved(model in nonempty_string()) {
        let wo = WorkOrderBuilder::new("t").model(&model).build();
        prop_assert_eq!(wo.config.model.as_deref(), Some(model.as_str()));
    }

    #[test]
    fn work_order_max_turns_preserved(turns in 1u32..1000) {
        let wo = WorkOrderBuilder::new("t").max_turns(turns).build();
        prop_assert_eq!(wo.config.max_turns, Some(turns));
    }

    #[test]
    fn work_order_workspace_root_preserved(root in nonempty_string()) {
        let wo = WorkOrderBuilder::new("t").root(&root).build();
        prop_assert_eq!(&wo.workspace.root, &root);
    }
}

// ===========================================================================
// 6. ErrorCode properties (bonus, 4 tests)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn error_code_serde_roundtrip(code in arb_error_code()) {
        let json = serde_json::to_string(&code).unwrap();
        let back: abp_error::ErrorCode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(code, back);
    }

    #[test]
    fn error_code_as_str_nonempty(code in arb_error_code()) {
        prop_assert!(!code.as_str().is_empty());
    }

    #[test]
    fn error_code_category_is_consistent(code in arb_error_code()) {
        let cat = code.category();
        let display = format!("{cat}");
        prop_assert!(!display.is_empty());
    }

    #[test]
    fn error_code_retryable_subset(code in arb_error_code()) {
        if code.is_retryable() {
            let cat = code.category();
            // Only backend errors should be retryable.
            prop_assert_eq!(cat, abp_error::ErrorCategory::Backend);
        }
    }
}
