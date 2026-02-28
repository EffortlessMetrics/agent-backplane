// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-integrations`.

use abp_core::*;
use abp_integrations::projection::{Dialect, ProjectionMatrix};
use abp_integrations::{MockBackend, Backend, ensure_capability_requirements, extract_execution_mode};
use proptest::prelude::*;
use uuid::Uuid;

// ── Strategies ──────────────────────────────────────────────────────

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<u128>().prop_map(Uuid::from_u128)
}

fn arb_dialect() -> impl Strategy<Value = Dialect> {
    prop_oneof![
        Just(Dialect::Abp),
        Just(Dialect::Claude),
        Just(Dialect::Codex),
        Just(Dialect::Gemini),
        Just(Dialect::Kimi),
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
        "[a-z]{1,8}".prop_map(|reason| SupportLevel::Restricted { reason }),
    ]
}

fn arb_min_support() -> impl Strategy<Value = MinSupport> {
    prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)]
}

fn arb_capability_manifest() -> impl Strategy<Value = CapabilityManifest> {
    prop::collection::vec((arb_capability(), arb_support_level()), 0..6)
        .prop_map(|pairs| pairs.into_iter().collect())
}

fn arb_capability_requirements() -> impl Strategy<Value = CapabilityRequirements> {
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
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (arb_uuid(), "\\PC{1,64}").prop_map(|(id, task)| WorkOrder {
        id,
        task,
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket {
            files: vec![],
            snippets: vec![],
        },
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements { required: vec![] },
        config: RuntimeConfig::default(),
    })
}

// ── 1. Arbitrary dialect pairs → projection never panics ────────────

proptest! {
    #[test]
    fn projection_never_panics(
        from in arb_dialect(),
        to in arb_dialect(),
        wo in arb_work_order(),
    ) {
        let matrix = ProjectionMatrix::new();
        // translate may return Err for unsupported pairs, but must never panic.
        let _result = matrix.translate(from, to, &wo);
    }
}

// ── 2. Identity translations always succeed ─────────────────────────

proptest! {
    #[test]
    fn identity_translation_always_succeeds(
        dialect in arb_dialect(),
        wo in arb_work_order(),
    ) {
        let matrix = ProjectionMatrix::new();
        let result = matrix.translate(dialect, dialect, &wo);
        prop_assert!(result.is_ok(), "identity translation failed: {result:?}");
    }
}

// ── 3. Random capability sets → satisfaction check is deterministic ──

proptest! {
    #[test]
    fn capability_satisfaction_is_deterministic(
        reqs in arb_capability_requirements(),
        manifest in arb_capability_manifest(),
    ) {
        let r1 = ensure_capability_requirements(&reqs, &manifest);
        let r2 = ensure_capability_requirements(&reqs, &manifest);
        prop_assert_eq!(r1.is_ok(), r2.is_ok(), "satisfaction must be deterministic");
    }
}

// ── 4. MockBackend with arbitrary tasks → always produces receipt ────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn mock_backend_always_produces_receipt(
        run_id in arb_uuid(),
        wo in arb_work_order(),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let mock = MockBackend;
            let (tx, _rx) = tokio::sync::mpsc::channel(256);
            let receipt = mock.run(run_id, wo, tx).await;
            prop_assert!(receipt.is_ok(), "mock backend should always succeed");

            let r = receipt.unwrap();
            prop_assert_eq!(r.meta.run_id, run_id);
            prop_assert!(r.receipt_sha256.is_some(), "receipt should be hashed");
            Ok(())
        })?;
    }
}

// ── 5. Execution mode extraction → consistent with vendor config ────

proptest! {
    #[test]
    fn execution_mode_extraction_consistency(
        wo in arb_work_order(),
        use_passthrough in any::<bool>(),
    ) {
        // Without any vendor config, default is Mapped.
        let mode = extract_execution_mode(&wo);
        prop_assert_eq!(mode, ExecutionMode::Mapped);

        // With explicit vendor config, mode should match.
        let mut wo2 = wo.clone();
        let mode_str = if use_passthrough {
            "passthrough"
        } else {
            "mapped"
        };
        wo2.config.vendor.insert(
            "abp".into(),
            serde_json::json!({ "mode": mode_str }),
        );
        let extracted = extract_execution_mode(&wo2);
        let expected = if use_passthrough {
            ExecutionMode::Passthrough
        } else {
            ExecutionMode::Mapped
        };
        prop_assert_eq!(extracted, expected);
    }
}

// ── 6. ABP-to-vendor translations always succeed ────────────────────

proptest! {
    #[test]
    fn abp_to_vendor_translations_succeed(
        to in arb_dialect(),
        wo in arb_work_order(),
    ) {
        let matrix = ProjectionMatrix::new();
        let result = matrix.translate(Dialect::Abp, to, &wo);
        prop_assert!(result.is_ok(), "ABP→{to:?} translation failed: {result:?}");
    }
}

// ── 7. MockBackend identity/capabilities are consistent ─────────────

proptest! {
    #[test]
    fn mock_backend_identity_is_consistent(_unused in Just(())) {
        let mock = MockBackend;

        let id1 = mock.identity();
        let id2 = mock.identity();
        prop_assert_eq!(id1.id, id2.id);
        prop_assert_eq!(id1.backend_version, id2.backend_version);

        let caps1 = mock.capabilities();
        let caps2 = mock.capabilities();
        prop_assert_eq!(caps1.len(), caps2.len());
    }
}
