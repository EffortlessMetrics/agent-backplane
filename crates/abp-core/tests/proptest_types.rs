use abp_core::*;
use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use uuid::Uuid;

// ── Arbitrary strategies ────────────────────────────────────────────

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<u128>().prop_map(Uuid::from_u128)
}

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    // Stay within a reasonable range that chrono handles well.
    (0i64..2_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

fn arb_execution_mode() -> impl Strategy<Value = ExecutionMode> {
    prop_oneof![Just(ExecutionMode::Passthrough), Just(ExecutionMode::Mapped)]
}

fn arb_execution_lane() -> impl Strategy<Value = ExecutionLane> {
    prop_oneof![Just(ExecutionLane::PatchFirst), Just(ExecutionLane::WorkspaceFirst)]
}

fn arb_workspace_mode() -> impl Strategy<Value = WorkspaceMode> {
    prop_oneof![Just(WorkspaceMode::PassThrough), Just(WorkspaceMode::Staged)]
}

fn arb_workspace_spec() -> impl Strategy<Value = WorkspaceSpec> {
    (".*", arb_workspace_mode()).prop_map(|(root, mode)| WorkspaceSpec {
        root,
        mode,
        include: vec![],
        exclude: vec![],
    })
}

fn arb_context_snippet() -> impl Strategy<Value = ContextSnippet> {
    (".*", ".*").prop_map(|(name, content)| ContextSnippet { name, content })
}

fn arb_context_packet() -> impl Strategy<Value = ContextPacket> {
    prop::collection::vec(arb_context_snippet(), 0..3).prop_map(|snippets| ContextPacket {
        files: vec![],
        snippets,
    })
}

fn arb_policy_profile() -> impl Strategy<Value = PolicyProfile> {
    Just(PolicyProfile::default())
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

fn arb_min_support() -> impl Strategy<Value = MinSupport> {
    prop_oneof![Just(MinSupport::Native), Just(MinSupport::Emulated)]
}

fn arb_capability_requirement() -> impl Strategy<Value = CapabilityRequirement> {
    (arb_capability(), arb_min_support())
        .prop_map(|(capability, min_support)| CapabilityRequirement {
            capability,
            min_support,
        })
}

fn arb_capability_requirements() -> impl Strategy<Value = CapabilityRequirements> {
    prop::collection::vec(arb_capability_requirement(), 0..3)
        .prop_map(|required| CapabilityRequirements { required })
}

fn arb_runtime_config() -> impl Strategy<Value = RuntimeConfig> {
    prop::option::of(".*").prop_map(|model| RuntimeConfig {
        model,
        ..Default::default()
    })
}

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        arb_uuid(),
        ".*",
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

fn arb_support_level() -> impl Strategy<Value = SupportLevel> {
    prop_oneof![
        Just(SupportLevel::Native),
        Just(SupportLevel::Emulated),
        Just(SupportLevel::Unsupported),
        ".*".prop_map(|reason| SupportLevel::Restricted { reason }),
    ]
}

fn arb_capability_manifest() -> impl Strategy<Value = CapabilityManifest> {
    prop::collection::vec((arb_capability(), arb_support_level()), 0..4)
        .prop_map(|pairs| pairs.into_iter().collect())
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
}

fn arb_backend_identity() -> impl Strategy<Value = BackendIdentity> {
    (".*", prop::option::of(".*"), prop::option::of(".*")).prop_map(
        |(id, backend_version, adapter_version)| BackendIdentity {
            id,
            backend_version,
            adapter_version,
        },
    )
}

fn arb_run_metadata() -> impl Strategy<Value = RunMetadata> {
    (arb_uuid(), arb_uuid(), arb_datetime(), arb_datetime()).prop_map(
        |(run_id, work_order_id, started_at, finished_at)| RunMetadata {
            run_id,
            work_order_id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at,
            finished_at,
            duration_ms: 0,
        },
    )
}

fn arb_usage_normalized() -> impl Strategy<Value = UsageNormalized> {
    (
        prop::option::of(any::<u64>()),
        prop::option::of(any::<u64>()),
    )
        .prop_map(|(input_tokens, output_tokens)| UsageNormalized {
            input_tokens,
            output_tokens,
            ..Default::default()
        })
}

fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        ".*".prop_map(|message| AgentEventKind::RunStarted { message }),
        ".*".prop_map(|message| AgentEventKind::RunCompleted { message }),
        ".*".prop_map(|text| AgentEventKind::AssistantDelta { text }),
        ".*".prop_map(|text| AgentEventKind::AssistantMessage { text }),
        (".*", ".*").prop_map(|(path, summary)| AgentEventKind::FileChanged { path, summary }),
        ".*".prop_map(|message| AgentEventKind::Warning { message }),
        ".*".prop_map(|message| AgentEventKind::Error { message }),
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
        arb_run_metadata(),
        arb_backend_identity(),
        arb_capability_manifest(),
        arb_execution_mode(),
        arb_usage_normalized(),
        prop::collection::vec(arb_agent_event(), 0..3),
        arb_outcome(),
    )
        .prop_map(
            |(meta, backend, capabilities, mode, usage, trace, outcome)| Receipt {
                meta,
                backend,
                capabilities,
                mode,
                usage_raw: serde_json::Value::Null,
                usage,
                trace,
                artifacts: vec![],
                verification: VerificationReport::default(),
                outcome,
                receipt_sha256: None,
            },
        )
}

// ── Property tests ──────────────────────────────────────────────────

proptest! {
    #[test]
    fn work_order_json_round_trip(wo in arb_work_order()) {
        let json = serde_json::to_string(&wo).unwrap();
        let deser: WorkOrder = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }

    #[test]
    fn receipt_hash_determinism(r in arb_receipt()) {
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        prop_assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_with_hash_idempotence(r in arb_receipt()) {
        let r1 = r.clone().with_hash().unwrap();
        let r2 = r1.clone().with_hash().unwrap();
        prop_assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn agent_event_serde_round_trip(ev in arb_agent_event()) {
        let json = serde_json::to_string(&ev).unwrap();
        let deser: AgentEvent = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }

    #[test]
    fn execution_mode_round_trip(mode in arb_execution_mode()) {
        let json = serde_json::to_string(&mode).unwrap();
        let deser: ExecutionMode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(mode, deser);
    }

    #[test]
    fn canonical_json_determinism(r in arb_receipt()) {
        let s1 = canonical_json(&r).unwrap();
        let s2 = canonical_json(&r).unwrap();
        prop_assert_eq!(s1, s2);
    }

    #[test]
    fn sha256_hex_length(input in prop::collection::vec(any::<u8>(), 0..256)) {
        let hex = sha256_hex(&input);
        prop_assert_eq!(hex.len(), 64);
        prop_assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn support_level_satisfies_consistency(
        min in arb_min_support(),
        _unused in Just(())
    ) {
        // Native always satisfies any requirement.
        prop_assert!(SupportLevel::Native.satisfies(&min));

        // Native requirement rejects everything except Native.
        prop_assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
        prop_assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));

        // Unsupported never satisfies Emulated.
        prop_assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }
}
