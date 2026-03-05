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
#![allow(clippy::collapsible_if)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-crate property-based tests covering receipt hashing, policy evaluation,
//! glob matching, config serialization, envelope codec, work order builder,
//! error codes, and capability matching.

use proptest::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use abp_core::*;
use abp_error::{ErrorCategory, ErrorCode};
use abp_protocol::{Envelope, JsonlCodec};

// ═══════════════════════════════════════════════════════════════════════
// Shared strategies
// ═══════════════════════════════════════════════════════════════════════

fn arb_uuid() -> impl Strategy<Value = uuid::Uuid> {
    any::<u128>().prop_map(uuid::Uuid::from_u128)
}

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<chrono::Utc>> {
    use chrono::{TimeZone, Utc};
    (0i64..2_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

fn arb_safe_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_. ]{1,30}"
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
        Just(Capability::StructuredOutputJsonSchema),
        Just(Capability::McpClient),
        Just(Capability::McpServer),
        Just(Capability::ToolUse),
        Just(Capability::ExtendedThinking),
        Just(Capability::ImageInput),
        Just(Capability::PdfInput),
        Just(Capability::CodeExecution),
        Just(Capability::Logprobs),
        Just(Capability::SeedDeterminism),
        Just(Capability::StopSequences),
        Just(Capability::FunctionCalling),
        Just(Capability::Vision),
        Just(Capability::Audio),
        Just(Capability::JsonMode),
        Just(Capability::SystemMessage),
        Just(Capability::Temperature),
        Just(Capability::TopP),
        Just(Capability::TopK),
        Just(Capability::MaxTokens),
        Just(Capability::FrequencyPenalty),
        Just(Capability::PresencePenalty),
        Just(Capability::CacheControl),
        Just(Capability::BatchMode),
        Just(Capability::Embeddings),
        Just(Capability::ImageGeneration),
    ]
}

fn arb_support_level() -> impl Strategy<Value = SupportLevel> {
    prop_oneof![
        Just(SupportLevel::Native),
        Just(SupportLevel::Emulated),
        Just(SupportLevel::Unsupported),
        arb_safe_string().prop_map(|reason| SupportLevel::Restricted { reason }),
    ]
}

fn arb_min_support() -> impl Strategy<Value = MinSupport> {
    prop_oneof![
        Just(MinSupport::Native),
        Just(MinSupport::Emulated),
        Just(MinSupport::Any),
    ]
}

fn arb_capability_manifest() -> impl Strategy<Value = CapabilityManifest> {
    prop::collection::vec((arb_capability(), arb_support_level()), 0..5)
        .prop_map(|pairs| pairs.into_iter().collect())
}

fn arb_backend_identity() -> impl Strategy<Value = BackendIdentity> {
    (
        arb_safe_string(),
        prop::option::of(arb_safe_string()),
        prop::option::of(arb_safe_string()),
    )
        .prop_map(|(id, backend_version, adapter_version)| BackendIdentity {
            id,
            backend_version,
            adapter_version,
        })
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
        prop::option::of(0u64..100_000),
        prop::option::of(0u64..100_000),
    )
        .prop_map(|(input_tokens, output_tokens)| UsageNormalized {
            input_tokens,
            output_tokens,
            ..Default::default()
        })
}

fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        arb_safe_string().prop_map(|message| AgentEventKind::RunStarted { message }),
        arb_safe_string().prop_map(|message| AgentEventKind::RunCompleted { message }),
        arb_safe_string().prop_map(|text| AgentEventKind::AssistantDelta { text }),
        arb_safe_string().prop_map(|text| AgentEventKind::AssistantMessage { text }),
        (arb_safe_string(), arb_safe_string())
            .prop_map(|(path, summary)| AgentEventKind::FileChanged { path, summary }),
        arb_safe_string().prop_map(|message| AgentEventKind::Warning { message }),
        arb_safe_string().prop_map(|message| AgentEventKind::Error {
            message,
            error_code: None
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

fn arb_work_order() -> impl Strategy<Value = WorkOrder> {
    (
        arb_uuid(),
        arb_safe_string(),
        arb_execution_lane(),
        arb_workspace_mode(),
        prop::option::of(arb_safe_string()),
        arb_outcome(),
    )
        .prop_map(|(id, task, lane, ws_mode, model, _)| WorkOrder {
            id,
            task,
            lane,
            workspace: WorkspaceSpec {
                root: ".".into(),
                mode: ws_mode,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig {
                model,
                ..Default::default()
            },
        })
}

fn arb_envelope() -> impl Strategy<Value = Envelope> {
    prop_oneof![
        // Hello
        (
            arb_backend_identity(),
            arb_capability_manifest(),
            arb_execution_mode()
        )
            .prop_map(|(backend, capabilities, mode)| {
                Envelope::hello_with_mode(backend, capabilities, mode)
            }),
        // Run
        arb_work_order().prop_map(|wo| Envelope::Run {
            id: wo.id.to_string(),
            work_order: wo,
        }),
        // Event
        (arb_safe_string(), arb_agent_event())
            .prop_map(|(ref_id, event)| Envelope::Event { ref_id, event }),
        // Final
        (arb_safe_string(), arb_receipt())
            .prop_map(|(ref_id, receipt)| Envelope::Final { ref_id, receipt }),
        // Fatal
        (prop::option::of(arb_safe_string()), arb_safe_string()).prop_map(|(ref_id, error)| {
            Envelope::Fatal {
                ref_id,
                error,
                error_code: None,
            }
        }),
    ]
}

fn arb_error_code() -> impl Strategy<Value = ErrorCode> {
    prop_oneof![
        Just(ErrorCode::ProtocolInvalidEnvelope),
        Just(ErrorCode::ProtocolHandshakeFailed),
        Just(ErrorCode::ProtocolMissingRefId),
        Just(ErrorCode::ProtocolUnexpectedMessage),
        Just(ErrorCode::ProtocolVersionMismatch),
        Just(ErrorCode::MappingUnsupportedCapability),
        Just(ErrorCode::MappingDialectMismatch),
        Just(ErrorCode::MappingLossyConversion),
        Just(ErrorCode::MappingUnmappableTool),
        Just(ErrorCode::BackendNotFound),
        Just(ErrorCode::BackendUnavailable),
        Just(ErrorCode::BackendTimeout),
        Just(ErrorCode::BackendRateLimited),
        Just(ErrorCode::BackendAuthFailed),
        Just(ErrorCode::BackendModelNotFound),
        Just(ErrorCode::BackendCrashed),
        Just(ErrorCode::ExecutionToolFailed),
        Just(ErrorCode::ExecutionWorkspaceError),
        Just(ErrorCode::ExecutionPermissionDenied),
        Just(ErrorCode::ContractVersionMismatch),
        Just(ErrorCode::ContractSchemaViolation),
        Just(ErrorCode::ContractInvalidReceipt),
        Just(ErrorCode::CapabilityUnsupported),
        Just(ErrorCode::CapabilityEmulationFailed),
        Just(ErrorCode::PolicyDenied),
        Just(ErrorCode::PolicyInvalid),
        Just(ErrorCode::WorkspaceInitFailed),
        Just(ErrorCode::WorkspaceStagingFailed),
        Just(ErrorCode::IrLoweringFailed),
        Just(ErrorCode::IrInvalid),
        Just(ErrorCode::ReceiptHashMismatch),
        Just(ErrorCode::ReceiptChainBroken),
        Just(ErrorCode::DialectUnknown),
        Just(ErrorCode::DialectMappingFailed),
        Just(ErrorCode::ConfigInvalid),
        Just(ErrorCode::RateLimitExceeded),
        Just(ErrorCode::CircuitBreakerOpen),
        Just(ErrorCode::StreamClosed),
        Just(ErrorCode::ReceiptStoreFailed),
        Just(ErrorCode::ValidationFailed),
        Just(ErrorCode::SidecarSpawnFailed),
        Just(ErrorCode::BackendContentFiltered),
        Just(ErrorCode::BackendContextLength),
        Just(ErrorCode::Internal),
    ]
}

fn arb_tool_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Read".to_string()),
        Just("Write".to_string()),
        Just("Bash".to_string()),
        Just("Grep".to_string()),
        Just("Glob".to_string()),
        Just("Edit".to_string()),
        Just("DeleteFile".to_string()),
        Just("WebSearch".to_string()),
    ]
}

fn arb_path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,7}"
}

fn arb_relative_path() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_path_segment(), 1..4)
        .prop_map(|segs| segs.join("/"))
        .prop_map(|p| format!("{p}.rs"))
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Receipt hashing: deterministic + self-referential prevention
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// hash(receipt) is deterministic for same input.
    #[test]
    fn receipt_hash_is_deterministic(r in arb_receipt()) {
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        prop_assert_eq!(&h1, &h2, "same receipt must produce same hash");
    }

    /// with_hash is idempotent: hashing an already-hashed receipt
    /// produces the same hash value.
    #[test]
    fn receipt_with_hash_idempotent(r in arb_receipt()) {
        let r1 = r.clone().with_hash().unwrap();
        let r2 = r1.clone().with_hash().unwrap();
        prop_assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    }

    /// The hash output is always a 64-char hex string.
    #[test]
    fn receipt_hash_format(r in arb_receipt()) {
        let h = receipt_hash(&r).unwrap();
        prop_assert_eq!(h.len(), 64);
        prop_assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Changing the outcome field changes the hash.
    #[test]
    fn receipt_hash_varies_with_outcome(r in arb_receipt()) {
        let mut r2 = r.clone();
        r2.outcome = match r.outcome {
            Outcome::Complete => Outcome::Failed,
            _ => Outcome::Complete,
        };
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        prop_assert_ne!(h1, h2, "different outcomes must produce different hashes");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Policy evaluation: allow ≠ deny, deny overrides
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// A tool in the denylist is always denied regardless of allowlist.
    #[test]
    fn policy_deny_overrides_allow(tool in arb_tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec![tool.clone()],
            ..PolicyProfile::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool(&tool);
        prop_assert!(!decision.allowed, "denied tool must not be allowed: {tool}");
    }

    /// Empty policy allows every tool.
    #[test]
    fn policy_empty_allows_all(tool in arb_tool_name()) {
        let engine = abp_policy::PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_use_tool(&tool).allowed);
    }

    /// A tool NOT in the allowlist is denied when the allowlist is non-empty.
    #[test]
    fn policy_allowlist_excludes_unlisted(tool in arb_tool_name()) {
        let policy = PolicyProfile {
            allowed_tools: vec!["__never_matches__".to_string()],
            ..PolicyProfile::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_use_tool(&tool).allowed);
    }

    /// deny_write blocks paths matching the pattern.
    #[test]
    fn policy_deny_write_blocks_pattern(path in arb_relative_path()) {
        let policy = PolicyProfile {
            deny_write: vec!["**/*.rs".to_string()],
            ..PolicyProfile::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_write_path(Path::new(&path)).allowed);
    }

    /// deny_read blocks paths matching the pattern.
    #[test]
    fn policy_deny_read_blocks_pattern(path in arb_relative_path()) {
        let policy = PolicyProfile {
            deny_read: vec!["**/*.rs".to_string()],
            ..PolicyProfile::default()
        };
        let engine = abp_policy::PolicyEngine::new(&policy).unwrap();
        prop_assert!(!engine.can_read_path(Path::new(&path)).allowed);
    }

    /// Empty deny_write does not block any path.
    #[test]
    fn policy_empty_deny_write_allows_all(path in arb_relative_path()) {
        let engine = abp_policy::PolicyEngine::new(&PolicyProfile::default()).unwrap();
        prop_assert!(engine.can_write_path(Path::new(&path)).allowed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Glob matching: include ∩ exclude = ∅ for non-overlapping patterns
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// With disjoint include and exclude prefixes, a path under the
    /// exclude prefix is denied even though it matches the include.
    #[test]
    fn glob_exclude_takes_precedence(seg in arb_path_segment()) {
        let globs = abp_glob::IncludeExcludeGlobs::new(
            &["src/**".to_string()],
            &["src/secret/**".to_string()],
        ).unwrap();

        let secret_path = format!("src/secret/{seg}.rs");
        let normal_path = format!("src/{seg}.rs");

        prop_assert_eq!(
            globs.decide_str(&secret_path),
            abp_glob::MatchDecision::DeniedByExclude,
        );
        prop_assert_eq!(
            globs.decide_str(&normal_path),
            abp_glob::MatchDecision::Allowed,
        );
    }

    /// Empty globs allow everything.
    #[test]
    fn glob_empty_allows_all(path in arb_relative_path()) {
        let globs = abp_glob::IncludeExcludeGlobs::new(&[], &[]).unwrap();
        prop_assert_eq!(globs.decide_str(&path), abp_glob::MatchDecision::Allowed);
    }

    /// decide_str and decide_path agree for all paths.
    #[test]
    fn glob_decide_str_vs_path_consistent(path in arb_relative_path()) {
        let globs = abp_glob::IncludeExcludeGlobs::new(
            &["src/**".to_string()],
            &["*.log".to_string()],
        ).unwrap();
        let str_decision = globs.decide_str(&path);
        let path_decision = globs.decide_path(Path::new(&path));
        prop_assert_eq!(str_decision, path_decision);
    }

    /// A path not matching any include pattern is denied when includes exist.
    #[test]
    fn glob_missing_include_denied(seg in arb_path_segment()) {
        let globs = abp_glob::IncludeExcludeGlobs::new(
            &["tests/**".to_string()],
            &[],
        ).unwrap();
        let outside_path = format!("docs/{seg}.md");
        prop_assert_eq!(
            globs.decide_str(&outside_path),
            abp_glob::MatchDecision::DeniedByMissingInclude,
        );
    }

    /// Decision is deterministic: calling twice gives the same result.
    #[test]
    fn glob_decision_deterministic(path in arb_relative_path()) {
        let globs = abp_glob::IncludeExcludeGlobs::new(
            &["src/**".to_string()],
            &["src/gen/**".to_string()],
        ).unwrap();
        let d1 = globs.decide_str(&path);
        let d2 = globs.decide_str(&path);
        prop_assert_eq!(d1, d2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Config serialization: roundtrip(config) == config
// ═══════════════════════════════════════════════════════════════════════

fn arb_backplane_config() -> impl Strategy<Value = abp_config::BackplaneConfig> {
    (
        prop::option::of(arb_safe_string()),
        prop::option::of(arb_safe_string()),
        prop::option::of(prop_oneof![
            Just("error".to_string()),
            Just("warn".to_string()),
            Just("info".to_string()),
            Just("debug".to_string()),
            Just("trace".to_string()),
        ]),
        prop::option::of(arb_safe_string()),
        prop::option::of(1u16..65535),
    )
        .prop_map(
            |(default_backend, workspace_dir, log_level, receipts_dir, port)| {
                abp_config::BackplaneConfig {
                    default_backend,
                    workspace_dir,
                    log_level,
                    receipts_dir,
                    bind_address: None,
                    port,
                    policy_profiles: vec![],
                    backends: BTreeMap::new(),
                }
            },
        )
}

fn arb_backplane_config_with_backend() -> impl Strategy<Value = abp_config::BackplaneConfig> {
    (arb_backplane_config(), arb_safe_string()).prop_map(|(mut config, name)| {
        config
            .backends
            .insert(name, abp_config::BackendEntry::Mock {});
        config
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// JSON roundtrip: serialize then deserialize produces the same config.
    #[test]
    fn config_json_roundtrip(cfg in arb_backplane_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let deser: abp_config::BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cfg, deser);
    }

    /// TOML roundtrip: to_toml then parse_toml recovers the config.
    #[test]
    fn config_toml_roundtrip(cfg in arb_backplane_config_with_backend()) {
        let toml_str = abp_config::to_toml(&cfg).unwrap();
        let deser = abp_config::parse_toml(&toml_str).unwrap();
        prop_assert_eq!(cfg, deser);
    }

    /// Merge with default is identity: merge(cfg, default) preserves cfg fields.
    #[test]
    fn config_merge_with_default_is_identity(cfg in arb_backplane_config()) {
        let merged = abp_config::merge_configs(
            cfg.clone(),
            abp_config::BackplaneConfig {
                log_level: None,
                ..Default::default()
            },
        );
        prop_assert_eq!(cfg.default_backend, merged.default_backend);
        prop_assert_eq!(cfg.workspace_dir, merged.workspace_dir);
        prop_assert_eq!(cfg.receipts_dir, merged.receipts_dir);
        prop_assert_eq!(cfg.port, merged.port);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Envelope codec: decode(encode(envelope)) == envelope
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Encoding then decoding an envelope recovers the tag discriminator.
    #[test]
    fn envelope_encode_decode_tag_preserved(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        let tag_orig = std::mem::discriminant(&env);
        let tag_decoded = std::mem::discriminant(&decoded);
        prop_assert_eq!(tag_orig, tag_decoded);
    }

    /// Encoded envelope always ends with a newline.
    #[test]
    fn envelope_encode_ends_with_newline(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        prop_assert!(line.ends_with('\n'));
    }

    /// Encoded envelope is valid JSON.
    #[test]
    fn envelope_encode_is_valid_json(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        prop_assert!(parsed.is_object());
    }

    /// The "t" field is always present in the encoded JSON.
    #[test]
    fn envelope_has_tag_field(env in arb_envelope()) {
        let line = JsonlCodec::encode(&env).unwrap();
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        prop_assert!(v.get("t").is_some(), "envelope JSON must contain 't' field");
    }

    /// Fatal envelopes preserve ref_id and error message through codec.
    #[test]
    fn envelope_fatal_roundtrip(
        ref_id in prop::option::of(arb_safe_string()),
        error in arb_safe_string(),
    ) {
        let env = Envelope::Fatal { ref_id: ref_id.clone(), error: error.clone(), error_code: None };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Fatal { ref_id: rid, error: err, .. } = decoded {
            prop_assert_eq!(ref_id, rid);
            prop_assert_eq!(error, err);
        } else {
            prop_assert!(false, "expected Fatal variant");
        }
    }

    /// Encoding multiple envelopes concatenated decodes to same count.
    #[test]
    fn envelope_multi_encode_decode(envs in prop::collection::vec(arb_envelope(), 1..5)) {
        let mut buf = Vec::new();
        for e in &envs {
            let line = JsonlCodec::encode(e).unwrap();
            buf.extend(line.as_bytes());
        }
        let reader = std::io::BufReader::new(buf.as_slice());
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        prop_assert_eq!(envs.len(), decoded.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. WorkOrder builder: build(fields) produces valid work order
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Builder always produces a valid work order with the given task.
    #[test]
    fn work_order_builder_preserves_task(task in arb_safe_string()) {
        let wo = WorkOrderBuilder::new(task.clone()).build();
        prop_assert_eq!(wo.task, task);
    }

    /// Builder respects lane setting.
    #[test]
    fn work_order_builder_preserves_lane(
        task in arb_safe_string(),
        lane in arb_execution_lane(),
    ) {
        let wo = WorkOrderBuilder::new(task).lane(lane.clone()).build();
        prop_assert_eq!(wo.lane, lane);
    }

    /// Builder respects model setting.
    #[test]
    fn work_order_builder_preserves_model(
        task in arb_safe_string(),
        model in arb_safe_string(),
    ) {
        let wo = WorkOrderBuilder::new(task).model(model.clone()).build();
        prop_assert_eq!(wo.config.model.as_deref(), Some(model.as_str()));
    }

    /// Builder respects max_turns setting.
    #[test]
    fn work_order_builder_preserves_max_turns(
        task in arb_safe_string(),
        turns in 1u32..1000,
    ) {
        let wo = WorkOrderBuilder::new(task).max_turns(turns).build();
        prop_assert_eq!(wo.config.max_turns, Some(turns));
    }

    /// Built work order round-trips through JSON.
    #[test]
    fn work_order_builder_json_roundtrip(task in arb_safe_string()) {
        let wo = WorkOrderBuilder::new(task).build();
        let json = serde_json::to_string(&wo).unwrap();
        let deser: WorkOrder = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(wo.task, deser.task);
        prop_assert_eq!(wo.id, deser.id);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Error code: unique string representations + category consistency
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// as_str() is non-empty for every error code.
    #[test]
    fn error_code_as_str_non_empty(code in arb_error_code()) {
        prop_assert!(!code.as_str().is_empty());
    }

    /// as_str() is a valid snake_case identifier.
    #[test]
    fn error_code_as_str_is_snake_case(code in arb_error_code()) {
        let s = code.as_str();
        prop_assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "as_str() must be snake_case: {s}",
        );
    }

    /// message() is non-empty for every error code.
    #[test]
    fn error_code_message_non_empty(code in arb_error_code()) {
        prop_assert!(!code.message().is_empty());
    }

    /// category() returns a valid ErrorCategory for every code.
    #[test]
    fn error_code_has_valid_category(code in arb_error_code()) {
        let cat = code.category();
        // Verify Display doesn't panic and is non-empty.
        let cat_str = format!("{cat}");
        prop_assert!(!cat_str.is_empty());
    }

    /// is_retryable() is consistent: retryable codes belong to known categories.
    #[test]
    fn error_code_retryable_category_consistency(code in arb_error_code()) {
        if code.is_retryable() {
            let cat = code.category();
            prop_assert!(
                matches!(cat, ErrorCategory::Backend | ErrorCategory::RateLimit | ErrorCategory::Stream),
                "retryable code {code:?} has unexpected category {cat:?}",
            );
        }
    }

    /// Serde roundtrip for ErrorCode.
    #[test]
    fn error_code_serde_roundtrip(code in arb_error_code()) {
        let json = serde_json::to_string(&code).unwrap();
        let deser: ErrorCode = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(code, deser);
    }
}

/// All error codes have unique as_str() values (exhaustive, non-proptest).
#[test]
fn error_code_all_unique_strings() {
    let all_codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolHandshakeFailed,
        ErrorCode::ProtocolMissingRefId,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::MappingUnsupportedCapability,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::MappingLossyConversion,
        ErrorCode::MappingUnmappableTool,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendUnavailable,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendRateLimited,
        ErrorCode::BackendAuthFailed,
        ErrorCode::BackendModelNotFound,
        ErrorCode::BackendCrashed,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::ExecutionWorkspaceError,
        ErrorCode::ExecutionPermissionDenied,
        ErrorCode::ContractVersionMismatch,
        ErrorCode::ContractSchemaViolation,
        ErrorCode::ContractInvalidReceipt,
        ErrorCode::CapabilityUnsupported,
        ErrorCode::CapabilityEmulationFailed,
        ErrorCode::PolicyDenied,
        ErrorCode::PolicyInvalid,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::WorkspaceStagingFailed,
        ErrorCode::IrLoweringFailed,
        ErrorCode::IrInvalid,
        ErrorCode::ReceiptHashMismatch,
        ErrorCode::ReceiptChainBroken,
        ErrorCode::DialectUnknown,
        ErrorCode::DialectMappingFailed,
        ErrorCode::ConfigInvalid,
        ErrorCode::RateLimitExceeded,
        ErrorCode::CircuitBreakerOpen,
        ErrorCode::StreamClosed,
        ErrorCode::ReceiptStoreFailed,
        ErrorCode::ValidationFailed,
        ErrorCode::SidecarSpawnFailed,
        ErrorCode::BackendContentFiltered,
        ErrorCode::BackendContextLength,
        ErrorCode::Internal,
    ];

    let mut seen = HashSet::new();
    for code in &all_codes {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate as_str for {code:?}: {s}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Capability matching: native ⊃ emulated ⊃ unsupported ordering
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Native satisfies every MinSupport threshold.
    #[test]
    fn capability_native_satisfies_all(min in arb_min_support()) {
        prop_assert!(SupportLevel::Native.satisfies(&min));
    }

    /// Unsupported never satisfies Native or Emulated.
    #[test]
    fn capability_unsupported_never_satisfies_strict(_dummy in Just(())) {
        prop_assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
        prop_assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    /// Emulated does not satisfy Native.
    #[test]
    fn capability_emulated_does_not_satisfy_native(_dummy in Just(())) {
        prop_assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    /// Emulated satisfies Emulated.
    #[test]
    fn capability_emulated_satisfies_emulated(_dummy in Just(())) {
        prop_assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    /// Any MinSupport::Any is satisfied by all support levels.
    #[test]
    fn capability_any_satisfied_by_all(level in arb_support_level()) {
        prop_assert!(level.satisfies(&MinSupport::Any));
    }

    /// Ordering: if Native satisfies threshold T, then Emulated satisfies
    /// every threshold that Emulated satisfies, and if Emulated doesn't satisfy T,
    /// Unsupported also doesn't.
    #[test]
    fn capability_ordering_consistency(min in arb_min_support()) {
        let native_ok = SupportLevel::Native.satisfies(&min);
        let emulated_ok = SupportLevel::Emulated.satisfies(&min);
        let unsupported_ok = SupportLevel::Unsupported.satisfies(&min);

        // Native always satisfies, verified above
        prop_assert!(native_ok);

        // If Emulated doesn't satisfy, Unsupported also must not.
        if !emulated_ok {
            prop_assert!(!unsupported_ok);
        }
    }

    /// negotiate_capabilities: native caps appear in result.native.
    #[test]
    fn capability_negotiate_native_recognized(cap in arb_capability()) {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(cap.clone(), SupportLevel::Native);

        let result = abp_capability::negotiate_capabilities(&[cap.clone()], &manifest);
        prop_assert!(
            result.native.contains(&cap),
            "native capability should appear in result.native",
        );
        prop_assert!(result.is_viable());
    }

    /// negotiate_capabilities: missing caps are unsupported.
    #[test]
    fn capability_negotiate_missing_is_unsupported(cap in arb_capability()) {
        let manifest = CapabilityManifest::new(); // empty
        let result = abp_capability::negotiate_capabilities(&[cap.clone()], &manifest);
        prop_assert!(
            result.unsupported_caps().contains(&cap),
            "missing capability should be unsupported",
        );
        prop_assert!(!result.is_viable());
    }

    /// Restricted satisfies Emulated threshold.
    #[test]
    fn capability_restricted_satisfies_emulated(reason in arb_safe_string()) {
        let restricted = SupportLevel::Restricted { reason };
        prop_assert!(restricted.satisfies(&MinSupport::Emulated));
    }
}
