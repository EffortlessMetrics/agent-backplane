// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::*;
use chrono::{TimeZone, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────

fn sample_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "Refactor auth module".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/ws".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["src/**".into()],
            exclude: vec!["target/**".into()],
        },
        context: ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "Use JWT".into(),
            }],
        },
        policy: PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![".env".into()],
            deny_write: vec!["Cargo.lock".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["write".into()],
        },
        requirements: CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        },
        config: RuntimeConfig {
            model: Some("gpt-4".into()),
            vendor: BTreeMap::new(),
            env: BTreeMap::new(),
            max_budget_usd: Some(1.0),
            max_turns: Some(10),
        },
    }
}

fn sample_receipt() -> Receipt {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: BTreeMap::from([
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(50),
            ..Default::default()
        },
        trace: vec![],
        artifacts: vec![ArtifactRef {
            kind: "diff".into(),
            path: "out.patch".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("+line".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ── 1. WorkOrder round-trip ─────────────────────────────────────────

mod work_order_serde {
    use super::*;

    #[test]
    fn round_trip_json() {
        let wo = sample_work_order();
        let json = serde_json::to_string_pretty(&wo).unwrap();
        let wo2: WorkOrder = serde_json::from_str(&json).unwrap();

        assert_eq!(wo.id, wo2.id);
        assert_eq!(wo.task, wo2.task);
        assert_eq!(wo.context.files, wo2.context.files);
        assert_eq!(wo.policy.allowed_tools, wo2.policy.allowed_tools);
        assert_eq!(wo.policy.deny_read, wo2.policy.deny_read);
        assert_eq!(wo.config.model, wo2.config.model);
        assert_eq!(wo.config.max_budget_usd, wo2.config.max_budget_usd);
        assert_eq!(wo.config.max_turns, wo2.config.max_turns);
    }

    #[test]
    fn execution_lane_serde_values() {
        let j = serde_json::to_value(ExecutionLane::PatchFirst).unwrap();
        assert_eq!(j, json!("patch_first"));

        let j = serde_json::to_value(ExecutionLane::WorkspaceFirst).unwrap();
        assert_eq!(j, json!("workspace_first"));
    }

    #[test]
    fn workspace_mode_serde_values() {
        let j = serde_json::to_value(WorkspaceMode::PassThrough).unwrap();
        assert_eq!(j, json!("pass_through"));

        let j = serde_json::to_value(WorkspaceMode::Staged).unwrap();
        assert_eq!(j, json!("staged"));
    }
}

// ── 2. Receipt hashing ──────────────────────────────────────────────

mod receipt_hashing {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let r = sample_receipt();
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn with_hash_produces_valid_hash() {
        let r = sample_receipt().with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
        let recomputed = receipt_hash(&r).unwrap();
        assert_eq!(r.receipt_sha256.as_deref().unwrap(), recomputed);
    }

    #[test]
    fn changing_field_changes_hash() {
        let r1 = sample_receipt();
        let mut r2 = sample_receipt();
        r2.meta.duration_ms = 999;

        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_ne!(
            h1, h2,
            "different duration_ms must produce different hashes"
        );
    }

    #[test]
    fn changing_outcome_changes_hash() {
        let r1 = sample_receipt();
        let mut r2 = sample_receipt();
        r2.outcome = Outcome::Failed;

        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn existing_hash_ignored_during_computation() {
        let mut r = sample_receipt();
        r.receipt_sha256 = Some("bogus".into());
        let h1 = receipt_hash(&r).unwrap();

        r.receipt_sha256 = None;
        let h2 = receipt_hash(&r).unwrap();

        assert_eq!(h1, h2, "receipt_sha256 value must not affect hash");
    }
}

// ── 3. AgentEvent all variants ──────────────────────────────────────

mod agent_event_variants {
    use super::*;

    fn ts() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
    }

    fn round_trip(kind: AgentEventKind) {
        let event = AgentEvent {
            ts: ts(),
            kind,
            ext: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event.ts, back.ts);
    }

    #[test]
    fn run_started() {
        round_trip(AgentEventKind::RunStarted {
            message: "go".into(),
        });
    }

    #[test]
    fn run_completed() {
        round_trip(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
    }

    #[test]
    fn assistant_delta() {
        round_trip(AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        });
    }

    #[test]
    fn assistant_message() {
        round_trip(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        });
    }

    #[test]
    fn tool_call() {
        round_trip(AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "file.rs"}),
        });
    }

    #[test]
    fn tool_result() {
        round_trip(AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: json!("contents"),
            is_error: false,
        });
    }

    #[test]
    fn file_changed() {
        round_trip(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        });
    }

    #[test]
    fn command_executed() {
        round_trip(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        });
    }

    #[test]
    fn warning() {
        round_trip(AgentEventKind::Warning {
            message: "watch out".into(),
        });
    }

    #[test]
    fn error() {
        round_trip(AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        });
    }

    #[test]
    fn event_type_tag_in_json() {
        let event = AgentEvent {
            ts: ts(),
            kind: AgentEventKind::Warning {
                message: "x".into(),
            },
            ext: None,
        };
        let v: serde_json::Value = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "warning");
    }

    #[test]
    fn ext_field_round_trips() {
        let mut ext = BTreeMap::new();
        ext.insert("raw_message".into(), json!({"vendor": true}));

        let event = AgentEvent {
            ts: ts(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some(ext),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        assert!(back.ext.is_some());
        assert_eq!(back.ext.unwrap()["raw_message"], json!({"vendor": true}));
    }
}

// ── 4. ExecutionMode ────────────────────────────────────────────────

mod execution_mode {
    use super::*;

    #[test]
    fn default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn serde_values() {
        assert_eq!(
            serde_json::to_value(ExecutionMode::Passthrough).unwrap(),
            json!("passthrough")
        );
        assert_eq!(
            serde_json::to_value(ExecutionMode::Mapped).unwrap(),
            json!("mapped")
        );
    }

    #[test]
    fn round_trip() {
        for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: ExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }
}

// ── 5. SupportLevel::satisfies ──────────────────────────────────────

mod support_level_satisfies {
    use super::*;

    #[test]
    fn native_min_native_level() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_min_emulated_level() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_min_unsupported_level() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_min_restricted_level() {
        let r = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(!r.satisfies(&MinSupport::Native));
    }

    #[test]
    fn emulated_min_native_level() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_min_emulated_level() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_min_unsupported_level() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_min_restricted_level() {
        let r = SupportLevel::Restricted {
            reason: "env".into(),
        };
        assert!(r.satisfies(&MinSupport::Emulated));
    }
}

// ── 6. Capability ordering ──────────────────────────────────────────

mod capability_ordering {
    use super::*;

    #[test]
    fn btreemap_is_deterministic() {
        let mut m1 = BTreeMap::new();
        m1.insert(Capability::ToolWrite, SupportLevel::Native);
        m1.insert(Capability::Streaming, SupportLevel::Native);
        m1.insert(Capability::ToolRead, SupportLevel::Emulated);

        let mut m2 = BTreeMap::new();
        m2.insert(Capability::ToolRead, SupportLevel::Emulated);
        m2.insert(Capability::Streaming, SupportLevel::Native);
        m2.insert(Capability::ToolWrite, SupportLevel::Native);

        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        assert_eq!(
            j1, j2,
            "BTreeMap must produce identical JSON regardless of insertion order"
        );
    }

    #[test]
    fn capability_serde_snake_case() {
        assert_eq!(
            serde_json::to_value(Capability::ToolRead).unwrap(),
            json!("tool_read")
        );
        assert_eq!(
            serde_json::to_value(Capability::HooksPreToolUse).unwrap(),
            json!("hooks_pre_tool_use")
        );
        assert_eq!(
            serde_json::to_value(Capability::McpClient).unwrap(),
            json!("mcp_client")
        );
    }
}

// ── 7. ContextPacket default ────────────────────────────────────────

mod context_packet_default {
    use super::*;

    #[test]
    fn default_has_empty_fields() {
        let cp = ContextPacket::default();
        assert!(cp.files.is_empty());
        assert!(cp.snippets.is_empty());
    }

    #[test]
    fn default_round_trips() {
        let cp = ContextPacket::default();
        let json = serde_json::to_string(&cp).unwrap();
        let back: ContextPacket = serde_json::from_str(&json).unwrap();
        assert!(back.files.is_empty());
        assert!(back.snippets.is_empty());
    }
}

// ── 8. PolicyProfile default ────────────────────────────────────────

mod policy_profile_default {
    use super::*;

    #[test]
    fn all_fields_default_to_empty() {
        let p = PolicyProfile::default();
        assert!(p.allowed_tools.is_empty());
        assert!(p.disallowed_tools.is_empty());
        assert!(p.deny_read.is_empty());
        assert!(p.deny_write.is_empty());
        assert!(p.allow_network.is_empty());
        assert!(p.deny_network.is_empty());
        assert!(p.require_approval_for.is_empty());
    }
}

// ── 9. canonical_json determinism ───────────────────────────────────

mod canonical_json_tests {
    use super::*;

    #[test]
    fn deterministic_for_btreemap() {
        let mut m = BTreeMap::new();
        m.insert("z_key".to_string(), json!(1));
        m.insert("a_key".to_string(), json!(2));
        m.insert("m_key".to_string(), json!(3));

        let j1 = canonical_json(&m).unwrap();
        let j2 = canonical_json(&m).unwrap();
        assert_eq!(j1, j2);
        // Keys should be alphabetically sorted
        let parsed: serde_json::Value = serde_json::from_str(&j1).unwrap();
        let keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["a_key", "m_key", "z_key"]);
    }

    #[test]
    fn nested_objects_sorted() {
        let mut inner = BTreeMap::new();
        inner.insert("beta".to_string(), json!(2));
        inner.insert("alpha".to_string(), json!(1));

        let mut outer = BTreeMap::new();
        outer.insert("inner".to_string(), json!(inner));

        let j = canonical_json(&outer).unwrap();
        // alpha before beta in the inner object
        assert!(j.find("alpha").unwrap() < j.find("beta").unwrap());
    }
}

// ── 10. sha256_hex correctness ──────────────────────────────────────

mod sha256_hex_tests {
    use super::*;

    #[test]
    fn known_empty_hash() {
        // SHA-256 of empty string
        let h = sha256_hex(b"");
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn known_hello_hash() {
        // SHA-256 of "hello"
        let h = sha256_hex(b"hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn hash_is_lowercase_hex() {
        let h = sha256_hex(b"test");
        assert!(
            h.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_eq!(h.len(), 64);
    }
}

// ── 11. CONTRACT_VERSION ────────────────────────────────────────────

mod contract_version {
    use super::*;

    #[test]
    fn equals_expected() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }
}

// ── 12. ExecutionLane serde round-trip ──────────────────────────────

mod execution_lane {
    use super::*;

    #[test]
    fn patch_first_round_trip() {
        let lane = ExecutionLane::PatchFirst;
        let json = serde_json::to_string(&lane).unwrap();
        let back: ExecutionLane = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), json!("patch_first"));
    }

    #[test]
    fn workspace_first_round_trip() {
        let lane = ExecutionLane::WorkspaceFirst;
        let json = serde_json::to_string(&lane).unwrap();
        let back: ExecutionLane = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            json!("workspace_first")
        );
    }
}

// ── Bonus: Outcome + SupportLevel serde ─────────────────────────────

mod outcome_serde {
    use super::*;

    #[test]
    fn outcome_values() {
        assert_eq!(
            serde_json::to_value(Outcome::Complete).unwrap(),
            json!("complete")
        );
        assert_eq!(
            serde_json::to_value(Outcome::Partial).unwrap(),
            json!("partial")
        );
        assert_eq!(
            serde_json::to_value(Outcome::Failed).unwrap(),
            json!("failed")
        );
    }
}

// ── WorkOrderBuilder ────────────────────────────────────────────────

mod builder {
    use super::*;

    #[test]
    fn builder_with_defaults() {
        let wo = WorkOrderBuilder::new("do something").build();
        assert_eq!(wo.task, "do something");
        assert_eq!(wo.workspace.root, ".");
        assert!(wo.config.model.is_none());
        assert!(wo.config.max_budget_usd.is_none());
        assert!(wo.config.max_turns.is_none());
        assert!(wo.context.files.is_empty());
        assert!(wo.policy.allowed_tools.is_empty());
        assert!(wo.requirements.required.is_empty());
    }

    #[test]
    fn builder_with_all_fields() {
        let wo = WorkOrderBuilder::new("refactor auth")
            .lane(ExecutionLane::WorkspaceFirst)
            .root("/tmp/ws")
            .workspace_mode(WorkspaceMode::PassThrough)
            .include(vec!["src/**".into()])
            .exclude(vec!["target/**".into()])
            .context(ContextPacket {
                files: vec!["README.md".into()],
                snippets: vec![],
            })
            .policy(PolicyProfile {
                allowed_tools: vec!["read".into()],
                ..Default::default()
            })
            .requirements(CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                }],
            })
            .model("gpt-4")
            .max_budget_usd(1.0)
            .max_turns(10)
            .build();

        assert_eq!(wo.task, "refactor auth");
        assert_eq!(wo.workspace.root, "/tmp/ws");
        assert_eq!(wo.workspace.include, vec!["src/**"]);
        assert_eq!(wo.workspace.exclude, vec!["target/**"]);
        assert_eq!(wo.context.files, vec!["README.md"]);
        assert_eq!(wo.policy.allowed_tools, vec!["read"]);
        assert_eq!(wo.requirements.required.len(), 1);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(wo.config.max_budget_usd, Some(1.0));
        assert_eq!(wo.config.max_turns, Some(10));
    }

    #[test]
    fn builder_produces_valid_work_order() {
        let wo = WorkOrderBuilder::new("test task")
            .model("claude-3")
            .max_turns(5)
            .build();

        let json = serde_json::to_string_pretty(&wo).unwrap();
        let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(wo.id, wo2.id);
        assert_eq!(wo.task, wo2.task);
        assert_eq!(wo.config.model, wo2.config.model);
        assert_eq!(wo.config.max_turns, wo2.config.max_turns);
    }
}

mod support_level_serde {
    use super::*;

    #[test]
    fn native_round_trip() {
        let j = serde_json::to_string(&SupportLevel::Native).unwrap();
        let back: SupportLevel = serde_json::from_str(&j).unwrap();
        let jv = serde_json::to_value(&back).unwrap();
        assert_eq!(jv, json!("native"));
    }

    #[test]
    fn restricted_round_trip() {
        let sl = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        let json = serde_json::to_string(&sl).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        if let SupportLevel::Restricted { reason } = back {
            assert_eq!(reason, "policy");
        } else {
            panic!("expected Restricted variant");
        }
    }
}
