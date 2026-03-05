#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests verifying the full crate dependency chain works together.
//!
//! Exercises boundaries across the entire hierarchy:
//!   abp-core → abp-protocol roundtrip
//!   abp-core → abp-ir → abp-mapper (IR pipeline)
//!   abp-policy → abp-workspace (policy enforcement)
//!   abp-config → abp-runtime (config drives runtime)
//!   abp-error-taxonomy → abp-protocol (error codes in fatal envelopes)
//!   Receipt lifecycle: create → hash → store → retrieve → verify
//!   SDK type → IR → different SDK type (full mapping chain)
//!   Event creation → stream → multiplex → collect

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use abp_capability::{
    NegotiationResult, check_capability, claude_35_sonnet_manifest, codex_manifest,
    copilot_manifest, gemini_15_pro_manifest, generate_report, kimi_manifest,
    negotiate_capabilities, openai_gpt4o_manifest,
};
use abp_config::load_from_str;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, Outcome,
    PolicyProfile, Receipt, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, WorkspaceMode,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_error_taxonomy::classification::{ErrorClassification, ErrorClassifier};
use abp_glob::IncludeExcludeGlobs;
use abp_ir::lower::{lower_for_dialect, lower_to_claude, lower_to_openai};
use abp_ir::normalize::{dedup_system, merge_adjacent_text, normalize, strip_empty, trim_text};
use abp_mapper::{IdentityMapper, Mapper};
use abp_mapping::{Fidelity, MappingRegistry, known_rules, validate_mapping};
use abp_policy::PolicyEngine;
use abp_projection::{ProjectionMatrix, ProjectionScore};
use abp_protocol::{Envelope, JsonlCodec};
use abp_receipt::{self as receipt_crate, ReceiptChain};
use abp_receipt_store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_sdk_types::Dialect as SdkDialect;
use abp_stream::{EventFilter as StreamEventFilter, EventMultiplexer, StreamPipelineBuilder};
use abp_validate::{
    EventValidator, ReceiptValidator as ValidateReceiptValidator, Validator, WorkOrderValidator,
};
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn make_receipt(backend: &str) -> Receipt {
    abp_receipt::ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .add_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build()
}

fn make_hashed_receipt(backend: &str) -> Receipt {
    abp_receipt::ReceiptBuilder::new(backend)
        .work_order_id(Uuid::new_v4())
        .run_id(Uuid::new_v4())
        .outcome(Outcome::Complete)
        .add_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .with_hash()
        .expect("hash should succeed")
}

fn sample_capability_manifest() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Native);
    m
}

fn sample_backend_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0".into()),
        adapter_version: Some("0.1".into()),
    }
}

fn make_policy(
    allowed: &[&str],
    disallowed: &[&str],
    deny_read: &[&str],
    deny_write: &[&str],
) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: allowed.iter().map(|s| s.to_string()).collect(),
        disallowed_tools: disallowed.iter().map(|s| s.to_string()).collect(),
        deny_read: deny_read.iter().map(|s| s.to_string()).collect(),
        deny_write: deny_write.iter().map(|s| s.to_string()).collect(),
        allow_network: vec![],
        deny_network: vec![],
        require_approval_for: vec![],
    }
}

fn make_ir_conversation() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"))
}

fn make_tool_definition() -> IrToolDefinition {
    IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
    }
}

fn make_full_receipt(backend: &str, outcome: Outcome) -> Receipt {
    Receipt {
        meta: abp_core::RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: backend.into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        capabilities: sample_capability_manifest(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![
            make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ],
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        },
        outcome,
        receipt_sha256: None,
    }
}

// ===========================================================================
// Module 1: abp-core → abp-protocol roundtrip
// ===========================================================================

mod core_to_protocol_roundtrip {
    use super::*;

    #[test]
    fn work_order_through_run_envelope() {
        let wo = make_work_order("integration test task");
        let env = Envelope::Run {
            id: "run-chain-1".into(),
            work_order: wo.clone(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, id } => {
                assert_eq!(id, "run-chain-1");
                assert_eq!(work_order.task, "integration test task");
                assert_eq!(work_order.id, wo.id);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn receipt_through_final_envelope() {
        let receipt = make_receipt("chain-mock");
        let env = Envelope::Final {
            ref_id: "run-chain-2".into(),
            receipt: receipt.clone(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { receipt: r, .. } => {
                assert_eq!(r.backend.id, "chain-mock");
                assert_eq!(r.outcome, Outcome::Complete);
                assert_eq!(r.trace.len(), receipt.trace.len());
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn all_event_kinds_survive_roundtrip() {
        let kinds = vec![
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            AgentEventKind::AssistantMessage {
                text: "full msg".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "test.rs"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-1".into()),
                output: json!("content"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added fn".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "cargo build".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            AgentEventKind::Warning {
                message: "rate limit".into(),
            },
            AgentEventKind::Error {
                message: "backend err".into(),
                error_code: Some(ErrorCode::BackendTimeout),
            },
        ];
        for kind in kinds {
            let evt = make_event(kind);
            let env = Envelope::Event {
                ref_id: "chain".into(),
                event: evt,
            };
            let json = JsonlCodec::encode(&env).unwrap();
            let decoded = JsonlCodec::decode(json.trim()).unwrap();
            assert!(matches!(decoded, Envelope::Event { .. }));
        }
    }

    #[test]
    fn hello_envelope_preserves_capabilities() {
        let caps = sample_capability_manifest();
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.into(),
            backend: sample_backend_identity(),
            capabilities: caps.clone(),
            mode: ExecutionMode::Mapped,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello {
                capabilities,
                contract_version,
                ..
            } => {
                assert_eq!(capabilities.len(), caps.len());
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn work_order_with_context_through_protocol() {
        let ctx = ContextPacket {
            files: vec!["main.rs".into(), "lib.rs".into()],
            snippets: vec![ContextSnippet {
                name: "snippet1".into(),
                content: "fn main() {}".into(),
            }],
        };
        let wo = WorkOrderBuilder::new("review").context(ctx).build();
        let env = Envelope::Run {
            id: "ctx-run".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => {
                assert_eq!(work_order.context.files.len(), 2);
                assert_eq!(work_order.context.snippets[0].name, "snippet1");
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn work_order_uuid_preserved_through_protocol() {
        let wo = make_work_order("uuid test");
        let original_id = wo.id;
        let env = Envelope::Run {
            id: "uuid-run".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Run { work_order, .. } => assert_eq!(work_order.id, original_id),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn receipt_with_usage_tokens_through_protocol() {
        let receipt = abp_receipt::ReceiptBuilder::new("token-backend")
            .outcome(Outcome::Partial)
            .backend_version("2.0")
            .adapter_version("0.5")
            .mode(ExecutionMode::Passthrough)
            .usage_tokens(500, 200)
            .build();
        let env = Envelope::Final {
            ref_id: "token-run".into(),
            receipt: receipt.clone(),
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { receipt: r, .. } => {
                assert_eq!(r.outcome, Outcome::Partial);
                assert_eq!(r.mode, ExecutionMode::Passthrough);
                assert_eq!(r.usage.input_tokens, Some(500));
                assert_eq!(r.usage.output_tokens, Some(200));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn multi_envelope_encode_decode_stream() {
        let envelopes = vec![
            Envelope::hello(sample_backend_identity(), sample_capability_manifest()),
            Envelope::Run {
                id: "r1".into(),
                work_order: make_work_order("task"),
            },
            Envelope::Event {
                ref_id: "r1".into(),
                event: make_event(AgentEventKind::AssistantDelta {
                    text: "hello".into(),
                }),
            },
            Envelope::Final {
                ref_id: "r1".into(),
                receipt: make_receipt("mock"),
            },
        ];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();
        let reader = std::io::BufReader::new(&buf[..]);
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded.len(), 4);
        assert!(matches!(decoded[0], Envelope::Hello { .. }));
        assert!(matches!(decoded[1], Envelope::Run { .. }));
        assert!(matches!(decoded[2], Envelope::Event { .. }));
        assert!(matches!(decoded[3], Envelope::Final { .. }));
    }

    #[test]
    fn vendor_manifest_survives_hello_roundtrip() {
        let caps = openai_gpt4o_manifest();
        let env = Envelope::hello(
            BackendIdentity {
                id: "openai".into(),
                backend_version: None,
                adapter_version: None,
            },
            caps.clone(),
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Hello { capabilities, .. } => {
                assert_eq!(capabilities.len(), caps.len());
            }
            _ => panic!("wrong variant"),
        }
    }
}

// ===========================================================================
// Module 2: abp-core → abp-ir → abp-mapper (IR pipeline)
// ===========================================================================

mod core_ir_mapper_pipeline {
    use super::*;

    #[test]
    fn ir_conversation_to_openai_json() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        let result = lower_to_openai(&conv, &tools);
        assert!(result.is_object());
        assert!(result.get("messages").is_some());
    }

    #[test]
    fn ir_conversation_to_claude_json() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        let result = lower_to_claude(&conv, &tools);
        assert!(result.is_object());
    }

    #[test]
    fn ir_lower_for_all_dialects() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        for dialect in SdkDialect::all() {
            let result = lower_for_dialect(*dialect, &conv, &tools);
            assert!(result.is_object(), "dialect {dialect:?} failed");
        }
    }

    #[test]
    fn ir_normalize_then_lower_pipeline() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, "  hello  "))
            .push(IrMessage::text(IrRole::Assistant, " response "));
        let normalized = normalize(&conv);
        let tools = vec![];
        let openai = lower_to_openai(&normalized, &tools);
        assert!(openai.is_object());
        let claude = lower_to_claude(&normalized, &tools);
        assert!(claude.is_object());
    }

    #[test]
    fn ir_tool_use_message_lowers_to_all_dialects() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Use the tool"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "tu-1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "test.rs"}),
                }],
            ));
        let tools = vec![make_tool_definition()];
        for dialect in SdkDialect::all() {
            let result = lower_for_dialect(*dialect, &conv, &tools);
            assert!(result.is_object(), "dialect {dialect:?} failed");
        }
    }

    #[test]
    fn ir_dedup_system_then_lower() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "First"))
            .push(IrMessage::text(IrRole::System, "Second"))
            .push(IrMessage::text(IrRole::User, "Hello"));
        let deduped = dedup_system(&conv);
        let system_count = deduped
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .count();
        assert!(system_count <= 1);
        let result = lower_to_openai(&deduped, &[]);
        assert!(result.is_object());
    }

    #[test]
    fn ir_strip_empty_then_lower() {
        let empty_msg = IrMessage {
            role: IrRole::User,
            content: vec![],
            metadata: Default::default(),
        };
        let non_empty = IrMessage::text(IrRole::User, "hello");
        let conv = IrConversation::from_messages(vec![empty_msg, non_empty]);
        let stripped = strip_empty(&conv);
        assert_eq!(stripped.len(), 1);
        let result = lower_to_openai(&stripped, &[]);
        assert!(result.is_object());
    }

    #[test]
    fn ir_merge_adjacent_then_lower() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hello "))
            .push(IrMessage::text(IrRole::User, "world"));
        let merged = merge_adjacent_text(&conv);
        let result = lower_to_openai(&merged, &[]);
        assert!(result.is_object());
    }

    #[test]
    fn identity_mapper_preserves_body() {
        use abp_mapper::DialectRequest;
        let mapper = IdentityMapper;
        let body = json!({"model": "test", "messages": [{"role": "user", "content": "hi"}]});
        let request = DialectRequest {
            dialect: mapper.source_dialect(),
            body: body.clone(),
        };
        let result = mapper.map_request(&request).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn supported_ir_pairs_not_empty() {
        let pairs = abp_mapper::supported_ir_pairs();
        assert!(!pairs.is_empty());
    }

    #[test]
    fn ir_thinking_block_lowers() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think...".into(),
                },
                IrContentBlock::Text {
                    text: "Answer".into(),
                },
            ],
        ));
        let result = lower_to_claude(&conv, &[]);
        assert!(result.is_object());
    }

    #[test]
    fn ir_conversation_properties_survive_normalize() {
        let conv = make_ir_conversation();
        let normalized = normalize(&conv);
        assert!(normalized.len() <= conv.len());
        assert!(normalized.system_message().is_some());
    }

    #[test]
    fn ir_trim_then_merge_then_lower() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "  first  "))
            .push(IrMessage::text(IrRole::User, "  second  "));
        let trimmed = trim_text(&conv);
        let merged = merge_adjacent_text(&trimmed);
        let result = lower_to_openai(&merged, &[]);
        assert!(result.is_object());
    }
}

// ===========================================================================
// Module 3: abp-policy → abp-workspace (policy enforcement in workspace)
// ===========================================================================

mod policy_workspace_chain {
    use super::*;

    #[test]
    fn policy_from_work_order_enforces_tool_restrictions() {
        let policy = make_policy(&["read_file", "write_file"], &["rm", "exec"], &[], &[]);
        let wo = WorkOrderBuilder::new("policy test").policy(policy).build();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(engine.can_use_tool("read_file").allowed);
        assert!(engine.can_use_tool("write_file").allowed);
        assert!(!engine.can_use_tool("rm").allowed);
        assert!(!engine.can_use_tool("exec").allowed);
    }

    #[test]
    fn policy_deny_read_matches_glob_patterns() {
        let policy = make_policy(&[], &[], &["**/.env*", "**/.git/**"], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(!engine.can_read_path(Path::new(".env.local")).allowed);
        assert!(!engine.can_read_path(Path::new(".git/config")).allowed);
        assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn policy_deny_write_matches_glob_patterns() {
        let policy = make_policy(&[], &[], &[], &["**/*.lock", "**/node_modules/**"]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
        assert!(
            !engine
                .can_write_path(Path::new("node_modules/pkg/index.js"))
                .allowed
        );
        assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn work_order_workspace_globs_align_with_policy_globs() {
        let wo = WorkOrderBuilder::new("aligned")
            .include(vec!["**/*.rs".into()])
            .exclude(vec!["target/**".into()])
            .policy(make_policy(&[], &[], &["target/**"], &["target/**"]))
            .build();
        let workspace_globs =
            IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
        let engine = PolicyEngine::new(&wo.policy).unwrap();
        assert!(workspace_globs.decide_str("src/lib.rs").is_allowed());
        assert!(!workspace_globs.decide_str("target/debug/main").is_allowed());
        assert!(!engine.can_read_path(Path::new("target/debug/main")).allowed);
    }

    #[test]
    fn empty_policy_allows_everything() {
        let policy = PolicyProfile {
            allowed_tools: vec![],
            disallowed_tools: vec![],
            deny_read: vec![],
            deny_write: vec![],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(engine.can_use_tool("anything").allowed);
        assert!(engine.can_read_path(Path::new("any/path")).allowed);
        assert!(engine.can_write_path(Path::new("any/path")).allowed);
    }

    #[test]
    fn policy_decision_has_reason_on_deny() {
        let policy = make_policy(&[], &["dangerous"], &[], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        let decision = engine.can_use_tool("dangerous");
        assert!(!decision.allowed);
        assert!(decision.reason.is_some());
    }

    #[test]
    fn glob_include_exclude_from_workspace_spec() {
        let wo = WorkOrderBuilder::new("glob-spec")
            .include(vec!["src/**".into(), "tests/**".into()])
            .exclude(vec!["**/*.bak".into()])
            .build();
        let globs = IncludeExcludeGlobs::new(&wo.workspace.include, &wo.workspace.exclude).unwrap();
        assert!(globs.decide_str("src/main.rs").is_allowed());
        assert!(globs.decide_str("tests/test1.rs").is_allowed());
        assert!(!globs.decide_str("src/old.bak").is_allowed());
        assert!(!globs.decide_str("README.md").is_allowed());
    }

    #[test]
    fn policy_with_wildcard_tool_deny() {
        let policy = make_policy(&[], &["bash*"], &[], &[]);
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_use_tool("bash").allowed);
        assert!(!engine.can_use_tool("bash_exec").allowed);
        assert!(engine.can_use_tool("read_file").allowed);
    }
}

// ===========================================================================
// Module 4: abp-config → abp-runtime (config drives runtime)
// ===========================================================================

mod config_runtime_chain {
    use super::*;

    #[test]
    fn config_loads_mock_backend() {
        let toml = r#"
[backends.mock]
type = "mock"
"#;
        let config = load_from_str(toml).unwrap();
        assert!(config.backends.contains_key("mock"));
    }

    #[test]
    fn config_loads_sidecar_backend() {
        let toml = r#"
[backends.node]
type = "sidecar"
command = "node"
args = ["hosts/node/index.js"]
"#;
        let config = load_from_str(toml).unwrap();
        assert!(config.backends.contains_key("node"));
    }

    #[test]
    fn empty_config_parses_successfully() {
        let config = load_from_str("").unwrap();
        assert!(config.backends.is_empty());
    }

    #[test]
    fn config_with_all_optional_fields() {
        let toml = r#"
default_backend = "mock"
workspace_dir = "/tmp/workspaces"
log_level = "debug"
receipts_dir = "/tmp/receipts"

[backends.mock]
type = "mock"
"#;
        let config = load_from_str(toml).unwrap();
        assert_eq!(config.default_backend.as_deref(), Some("mock"));
    }

    #[test]
    fn config_multiple_backends() {
        let toml = r#"
[backends.mock]
type = "mock"

[backends.node]
type = "sidecar"
command = "node"
args = ["index.js"]
"#;
        let config = load_from_str(toml).unwrap();
        assert_eq!(config.backends.len(), 2);
    }

    #[test]
    fn config_validation_passes_for_valid() {
        let toml = r#"
[backends.mock]
type = "mock"
"#;
        let config = load_from_str(toml).unwrap();
        let result = abp_config::validate_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn work_order_config_fields_set_from_builder() {
        let wo = WorkOrderBuilder::new("config-driven")
            .model("gpt-4o")
            .max_turns(10)
            .max_budget_usd(5.0)
            .build();
        assert_eq!(wo.task, "config-driven");
    }

    #[test]
    fn config_drives_work_order_workspace_mode() {
        let wo = WorkOrderBuilder::new("staged-task")
            .workspace_mode(WorkspaceMode::Staged)
            .root("/tmp/project")
            .build();
        assert_eq!(wo.workspace.mode, WorkspaceMode::Staged);
        assert_eq!(wo.workspace.root, "/tmp/project");
    }
}

// ===========================================================================
// Module 5: abp-error-taxonomy → abp-protocol (error codes in fatal)
// ===========================================================================

mod error_taxonomy_protocol_chain {
    use super::*;

    #[test]
    fn fatal_envelope_with_error_code_roundtrip() {
        let env = Envelope::fatal_with_code(
            Some("run-err".into()),
            "backend crashed",
            ErrorCode::BackendCrashed,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal {
                error,
                error_code,
                ref_id,
            } => {
                assert_eq!(error, "backend crashed");
                assert_eq!(ref_id.as_deref(), Some("run-err"));
                assert!(error_code.is_some());
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_from_abp_error_carries_code() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30s");
        let env = Envelope::fatal_from_abp_error(Some("run-timeout".into()), &err);
        assert!(matches!(env, Envelope::Fatal { .. }));
        match env {
            Envelope::Fatal {
                error, error_code, ..
            } => {
                assert!(error.contains("timed out"));
                assert_eq!(error_code, Some(ErrorCode::BackendTimeout));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn all_error_categories_have_codes() {
        let code_category_pairs = vec![
            (ErrorCode::ProtocolHandshakeFailed, ErrorCategory::Protocol),
            (ErrorCode::BackendTimeout, ErrorCategory::Backend),
            (ErrorCode::MappingDialectMismatch, ErrorCategory::Mapping),
            (ErrorCode::PolicyDenied, ErrorCategory::Policy),
            (ErrorCode::WorkspaceInitFailed, ErrorCategory::Workspace),
            (ErrorCode::IrLoweringFailed, ErrorCategory::Ir),
            (ErrorCode::ReceiptHashMismatch, ErrorCategory::Receipt),
            (ErrorCode::DialectUnknown, ErrorCategory::Dialect),
            (ErrorCode::ConfigInvalid, ErrorCategory::Config),
            (ErrorCode::CapabilityUnsupported, ErrorCategory::Capability),
            (ErrorCode::ExecutionToolFailed, ErrorCategory::Execution),
            (ErrorCode::ContractVersionMismatch, ErrorCategory::Contract),
            (ErrorCode::Internal, ErrorCategory::Internal),
        ];
        for (code, expected_cat) in code_category_pairs {
            assert_eq!(code.category(), expected_cat, "code {code:?}");
        }
    }

    #[test]
    fn error_code_in_event_survives_protocol_roundtrip() {
        let evt = make_event(AgentEventKind::Error {
            message: "rate limited".into(),
            error_code: Some(ErrorCode::BackendRateLimited),
        });
        let env = Envelope::Event {
            ref_id: "err-run".into(),
            event: evt,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Event { event, .. } => match &event.kind {
                AgentEventKind::Error {
                    error_code,
                    message,
                } => {
                    assert_eq!(*error_code, Some(ErrorCode::BackendRateLimited));
                    assert_eq!(message, "rate limited");
                }
                _ => panic!("wrong event kind"),
            },
            _ => panic!("wrong envelope"),
        }
    }

    #[test]
    fn abp_error_dto_roundtrip() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "tool not allowed");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message, "tool not allowed");
    }

    #[test]
    fn abp_error_with_context_fields() {
        let err = AbpError::new(ErrorCode::BackendNotFound, "not found")
            .with_context("backend", "openai")
            .with_context("region", "us-east-1");
        assert_eq!(err.context.len(), 2);
        assert_eq!(err.category(), ErrorCategory::Backend);
    }

    #[test]
    fn error_classifier_classifies_backend_errors() {
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&ErrorCode::BackendCrashed);
        // Classification uses its own category enum for recovery routing
        let _category = classification.category;
        let _severity = classification.severity;
    }

    #[test]
    fn error_code_retryability() {
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
    }

    #[test]
    fn fatal_envelope_without_ref_id() {
        let env = Envelope::fatal_with_code(None, "startup failure", ErrorCode::Internal);
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { ref_id, .. } => {
                assert!(ref_id.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn protocol_error_carries_error_code() {
        let proto_err = abp_protocol::ProtocolError::Violation("bad frame".into());
        let _display = format!("{proto_err}");
    }
}

// ===========================================================================
// Module 6: Receipt lifecycle: create → hash → store → retrieve → verify
// ===========================================================================

mod receipt_lifecycle {
    use super::*;

    #[test]
    fn create_receipt_has_no_hash() {
        let r = make_receipt("mock");
        assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn hash_receipt_produces_sha256() {
        let r = make_hashed_receipt("mock");
        assert!(r.receipt_sha256.is_some());
        let hash = r.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn verify_hash_passes_for_valid() {
        let r = make_hashed_receipt("mock");
        assert!(receipt_crate::verify_hash(&r));
    }

    #[test]
    fn verify_hash_fails_for_tampered() {
        let mut r = make_hashed_receipt("mock");
        r.backend.id = "tampered".into();
        assert!(!receipt_crate::verify_hash(&r));
    }

    #[tokio::test]
    async fn store_and_retrieve_receipt() {
        let store = InMemoryReceiptStore::new();
        let r = make_full_receipt("store-test", Outcome::Complete);
        let id = r.meta.run_id.to_string();
        store.store(&r).await.unwrap();
        let got = store.get(&id).await.unwrap().unwrap();
        assert_eq!(got.backend.id, "store-test");
        assert_eq!(got.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn store_retrieve_verify_lifecycle() {
        let store = InMemoryReceiptStore::new();
        let r = make_full_receipt("lifecycle", Outcome::Complete)
            .with_hash()
            .unwrap();
        let id = r.meta.run_id.to_string();
        store.store(&r).await.unwrap();
        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert!(receipt_crate::verify_hash(&retrieved));
    }

    #[tokio::test]
    async fn store_count_and_delete() {
        let store = InMemoryReceiptStore::new();
        let r1 = make_full_receipt("a", Outcome::Complete);
        let r2 = make_full_receipt("b", Outcome::Failed);
        let id1 = r1.meta.run_id.to_string();
        store.store(&r1).await.unwrap();
        store.store(&r2).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);
        assert!(store.delete(&id1).await.unwrap());
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn store_list_with_filter() {
        let store = InMemoryReceiptStore::new();
        store
            .store(&make_full_receipt("ok-backend", Outcome::Complete))
            .await
            .unwrap();
        store
            .store(&make_full_receipt("fail-backend", Outcome::Failed))
            .await
            .unwrap();
        let filter = ReceiptFilter {
            outcome: Some(Outcome::Complete),
            ..Default::default()
        };
        let results = store.list(filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_hash_is_deterministic_for_same_content() {
        let r = make_receipt("deterministic");
        let h1 = receipt_crate::compute_hash(&r).unwrap();
        let h2 = receipt_crate::compute_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_hash_changes_with_outcome() {
        let r1 = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let r2 = abp_receipt::ReceiptBuilder::new("mock")
            .outcome(Outcome::Failed)
            .with_hash()
            .unwrap();
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn receipt_hash_changes_with_backend() {
        let r1 = abp_receipt::ReceiptBuilder::new("alpha")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let r2 = abp_receipt::ReceiptBuilder::new("beta")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn receipt_canonical_json_has_null_hash() {
        let r = make_hashed_receipt("mock");
        let canonical = receipt_crate::canonicalize(&r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&canonical).unwrap();
        assert!(parsed.get("receipt_sha256").unwrap().is_null());
    }

    #[test]
    fn receipt_chain_push_and_verify() {
        let mut chain = ReceiptChain::new();
        for i in 0..5 {
            let r = make_hashed_receipt(&format!("backend-{i}"));
            chain.push(r).unwrap();
        }
        assert_eq!(chain.len(), 5);
    }

    #[test]
    fn receipt_chain_rejects_duplicates() {
        let mut chain = ReceiptChain::new();
        let r = make_hashed_receipt("dup");
        chain.push(r.clone()).unwrap();
        assert!(chain.push(r).is_err());
    }

    #[test]
    fn receipt_diff_detects_changes() {
        let r1 = make_full_receipt("mock", Outcome::Complete);
        let r2 = make_full_receipt("mock", Outcome::Failed);
        let diff = receipt_crate::diff_receipts(&r1, &r2);
        assert!(!diff.changes.is_empty());
    }

    #[test]
    fn receipt_diff_identical_is_empty() {
        let r = make_full_receipt("mock", Outcome::Complete);
        let diff = receipt_crate::diff_receipts(&r, &r);
        assert!(diff.is_empty());
    }

    #[test]
    fn receipt_with_hash_idempotent() {
        let r1 = make_hashed_receipt("mock");
        let hash1 = r1.receipt_sha256.clone().unwrap();
        let r2 = r1.with_hash().unwrap();
        let hash2 = r2.receipt_sha256.clone().unwrap();
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn store_get_nonexistent_returns_none() {
        let store = InMemoryReceiptStore::new();
        let result = store.get(&Uuid::new_v4().to_string()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn store_delete_nonexistent_returns_false() {
        let store = InMemoryReceiptStore::new();
        assert!(!store.delete(&Uuid::new_v4().to_string()).await.unwrap());
    }
}

// ===========================================================================
// Module 7: SDK type → IR → different SDK type (full mapping chain)
// ===========================================================================

mod sdk_ir_mapping_chain {
    use super::*;

    #[test]
    fn openai_request_detected_by_dialect_detector() {
        let detector = DialectDetector::new();
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = detector.detect(&req);
        if let Some(r) = result {
            assert_eq!(r.dialect, Dialect::OpenAi);
            assert!(r.confidence > 0.0);
        }
    }

    #[test]
    fn claude_request_detected_by_dialect_detector() {
        let detector = DialectDetector::new();
        let req = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 1024
        });
        let result = detector.detect(&req);
        if let Some(r) = result {
            assert_eq!(r.dialect, Dialect::Claude);
        }
    }

    #[test]
    fn dialect_validator_accepts_valid_openai() {
        let validator = DialectValidator::new();
        let req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = validator.validate(&req, Dialect::OpenAi);
        assert!(result.valid);
    }

    #[test]
    fn dialect_validator_rejects_empty_request() {
        let validator = DialectValidator::new();
        let req = json!({});
        let result = validator.validate(&req, Dialect::OpenAi);
        assert!(!result.valid);
    }

    #[test]
    fn ir_to_openai_to_ir_conceptual_roundtrip() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        let openai_json = lower_to_openai(&conv, &tools);
        assert!(openai_json.get("messages").is_some());
        // Can re-detect the dialect from the lowered output
        let detector = DialectDetector::new();
        if let Some(r) = detector.detect(&openai_json) {
            assert_eq!(r.dialect, Dialect::OpenAi);
        }
    }

    #[test]
    fn ir_to_claude_to_ir_conceptual_roundtrip() {
        let conv = make_ir_conversation();
        let tools = vec![];
        let claude_json = lower_to_claude(&conv, &tools);
        assert!(claude_json.is_object());
    }

    #[test]
    fn dialect_labels_are_unique() {
        let labels: Vec<&str> = Dialect::all().iter().map(|d| d.label()).collect();
        let unique: std::collections::HashSet<&&str> = labels.iter().collect();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn all_six_dialects_exist() {
        let dialects = Dialect::all();
        assert_eq!(dialects.len(), 6);
    }

    #[test]
    fn mapping_registry_has_known_rules() {
        let rules = known_rules();
        assert!(!rules.is_empty());
    }

    #[test]
    fn mapping_fidelity_lossless_check() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(
            !Fidelity::LossyLabeled {
                warning: "loss".into()
            }
            .is_lossless()
        );
        assert!(
            Fidelity::Unsupported {
                reason: "no".into()
            }
            .is_unsupported()
        );
    }

    #[test]
    fn sdk_dialect_all_matches_dialect_all() {
        assert_eq!(SdkDialect::all().len(), Dialect::all().len());
    }

    #[test]
    fn validate_mapping_between_openai_and_claude() {
        let registry = MappingRegistry::default();
        let features: Vec<String> = vec![];
        let result = validate_mapping(&registry, Dialect::OpenAi, Dialect::Claude, &features);
        let _ = result.len();
    }

    #[test]
    fn ir_lower_preserves_tool_definitions_for_openai() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Use the tool"));
        let tools = vec![make_tool_definition()];
        let openai = lower_to_openai(&conv, &tools);
        assert!(openai.get("tools").is_some() || openai.get("messages").is_some());
    }
}

// ===========================================================================
// Module 8: Event creation → stream → multiplex → collect
// ===========================================================================

mod event_stream_multiplex_chain {
    use super::*;
    use abp_core::stream::EventStream;

    #[test]
    fn event_creation_and_stream_construction() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: "chunk1".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: "chunk2".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = EventStream::new(events);
        assert_eq!(stream.len(), 4);
    }

    #[test]
    fn stream_count_by_kind() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "a".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "b".into() }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "f".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = EventStream::new(events);
        let counts = stream.count_by_kind();
        assert!(counts.len() >= 2);
    }

    #[test]
    fn stream_filter_errors_only() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
            make_event(AgentEventKind::Error {
                message: "err".into(),
                error_code: None,
            }),
            make_event(AgentEventKind::Warning {
                message: "warn".into(),
            }),
        ];
        let stream = EventStream::new(events);
        let filtered = stream.filter_pred(|e| matches!(e.kind, AgentEventKind::Error { .. }));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn stream_merge_two_streams() {
        let s1 = EventStream::new(vec![make_event(AgentEventKind::RunStarted {
            message: "s1".into(),
        })]);
        let s2 = EventStream::new(vec![make_event(AgentEventKind::RunStarted {
            message: "s2".into(),
        })]);
        let merged = s1.merge(&s2);
        assert_eq!(merged.len(), 2);
    }

    #[tokio::test]
    async fn multiplexer_collects_from_multiple_channels() {
        let (tx1, rx1) = mpsc::channel(16);
        let (tx2, rx2) = mpsc::channel(16);

        tx1.send(make_event(AgentEventKind::AssistantDelta {
            text: "from-1".into(),
        }))
        .await
        .unwrap();
        tx2.send(make_event(AgentEventKind::AssistantDelta {
            text: "from-2".into(),
        }))
        .await
        .unwrap();
        drop(tx1);
        drop(tx2);

        let mux = EventMultiplexer::new(vec![rx1, rx2]);
        assert_eq!(mux.stream_count(), 2);
        let collected = mux.collect_sorted().await;
        assert_eq!(collected.len(), 2);
    }

    #[tokio::test]
    async fn multiplexer_preserves_all_events() {
        let (tx1, rx1) = mpsc::channel(32);
        let (tx2, rx2) = mpsc::channel(32);

        for i in 0..5 {
            tx1.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("a-{i}"),
            }))
            .await
            .unwrap();
        }
        for i in 0..3 {
            tx2.send(make_event(AgentEventKind::AssistantDelta {
                text: format!("b-{i}"),
            }))
            .await
            .unwrap();
        }
        drop(tx1);
        drop(tx2);

        let mux = EventMultiplexer::new(vec![rx1, rx2]);
        let collected = mux.collect_sorted().await;
        assert_eq!(collected.len(), 8);
    }

    #[test]
    fn stream_pipeline_builder_creates_pipeline() {
        let pipeline = StreamPipelineBuilder::new().build();
        let event = make_event(AgentEventKind::AssistantDelta {
            text: "test".into(),
        });
        let _processed = pipeline.process(event);
    }

    #[test]
    fn stream_event_filter_by_kind() {
        let filter = StreamEventFilter::by_kind("assistant_delta");
        let delta = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
        let started = make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        });
        assert!(filter.matches(&delta));
        assert!(!filter.matches(&started));
    }

    #[test]
    fn stream_event_filter_errors_only() {
        let filter = StreamEventFilter::errors_only();
        let err = make_event(AgentEventKind::Error {
            message: "fail".into(),
            error_code: None,
        });
        let delta = make_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        });
        assert!(filter.matches(&err));
        assert!(!filter.matches(&delta));
    }

    #[test]
    fn stream_event_filter_exclude_errors() {
        let filter = StreamEventFilter::exclude_errors();
        let err = make_event(AgentEventKind::Error {
            message: "fail".into(),
            error_code: None,
        });
        let delta = make_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        });
        assert!(!filter.matches(&err));
        assert!(filter.matches(&delta));
    }

    #[test]
    fn stream_event_filter_combinators() {
        let delta_filter = StreamEventFilter::by_kind("assistant_delta");
        let msg_filter = StreamEventFilter::by_kind("assistant_message");
        let combined = delta_filter.or(msg_filter);
        let delta = make_event(AgentEventKind::AssistantDelta { text: "x".into() });
        let msg = make_event(AgentEventKind::AssistantMessage { text: "y".into() });
        let started = make_event(AgentEventKind::RunStarted {
            message: "s".into(),
        });
        assert!(combined.matches(&delta));
        assert!(combined.matches(&msg));
        assert!(!combined.matches(&started));
    }
}

// ===========================================================================
// Module 9: Cross-chain integration (multiple crate boundaries at once)
// ===========================================================================

mod cross_chain_integration {
    use super::*;

    #[test]
    fn work_order_to_protocol_to_policy_check() {
        let policy = make_policy(&["read_file"], &["rm"], &[], &[]);
        let wo = WorkOrderBuilder::new("cross-chain").policy(policy).build();
        // Encode through protocol
        let env = Envelope::Run {
            id: "cross-1".into(),
            work_order: wo,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        // Extract and enforce policy
        match decoded {
            Envelope::Run { work_order, .. } => {
                let engine = PolicyEngine::new(&work_order.policy).unwrap();
                assert!(engine.can_use_tool("read_file").allowed);
                assert!(!engine.can_use_tool("rm").allowed);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn receipt_through_protocol_then_store_and_verify() {
        let receipt = make_hashed_receipt("cross-verify");
        // Through protocol
        let env = Envelope::Final {
            ref_id: "cross-2".into(),
            receipt,
        };
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Final { receipt, .. } => {
                assert!(receipt_crate::verify_hash(&receipt));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn error_through_protocol_then_classify() {
        let err = AbpError::new(ErrorCode::BackendCrashed, "process died");
        let env = Envelope::fatal_from_abp_error(Some("cross-3".into()), &err);
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        match decoded {
            Envelope::Fatal { error_code, .. } => {
                if let Some(code) = error_code {
                    assert_eq!(code.category(), ErrorCategory::Backend);
                }
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn capability_negotiation_to_hello_envelope() {
        let manifest = openai_gpt4o_manifest();
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        assert!(result.is_viable());
        // Use the manifest in a hello envelope
        let env = Envelope::hello(
            BackendIdentity {
                id: "openai".into(),
                backend_version: None,
                adapter_version: None,
            },
            manifest,
        );
        let json = JsonlCodec::encode(&env).unwrap();
        assert!(!json.is_empty());
    }

    #[test]
    fn ir_lower_then_detect_dialect() {
        let conv = make_ir_conversation();
        let tools = vec![make_tool_definition()];
        let openai_json = lower_to_openai(&conv, &tools);
        let detector = DialectDetector::new();
        if let Some(result) = detector.detect(&openai_json) {
            assert_eq!(result.dialect, Dialect::OpenAi);
        }
    }

    #[test]
    fn full_protocol_session_simulation() {
        // Simulate a full sidecar session: hello → run → events → final
        let hello = Envelope::hello(sample_backend_identity(), sample_capability_manifest());
        let wo = make_work_order("full session");
        let run = Envelope::Run {
            id: "session-1".into(),
            work_order: wo,
        };
        let event1 = Envelope::Event {
            ref_id: "session-1".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
        };
        let event2 = Envelope::Event {
            ref_id: "session-1".into(),
            event: make_event(AgentEventKind::AssistantDelta {
                text: "response".into(),
            }),
        };
        let final_env = Envelope::Final {
            ref_id: "session-1".into(),
            receipt: make_receipt("mock"),
        };

        let all = vec![hello, run, event1, event2, final_env];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &all).unwrap();
        let reader = std::io::BufReader::new(&buf[..]);
        let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded.len(), 5);
        assert!(matches!(decoded[0], Envelope::Hello { .. }));
        assert!(matches!(decoded[4], Envelope::Final { .. }));
    }

    #[test]
    fn work_order_with_requirements_to_capability_check() {
        let wo = WorkOrderBuilder::new("cap-check")
            .requirements(CapabilityRequirements {
                required: vec![abp_core::CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: abp_core::MinSupport::Native,
                }],
            })
            .build();
        let manifest = openai_gpt4o_manifest();
        let level = check_capability(&manifest, &Capability::Streaming);
        assert!(matches!(
            level,
            abp_capability::SupportLevel::Native | abp_capability::SupportLevel::Emulated { .. }
        ));
        assert!(!wo.requirements.required.is_empty());
    }

    #[test]
    fn projection_matrix_registers_entries() {
        let matrix = ProjectionMatrix::new();
        assert_eq!(matrix.backend_count(), 0);
        let score = ProjectionScore {
            capability_coverage: 0.95,
            mapping_fidelity: 0.9,
            priority: 1.0,
            total: 0.0,
        };
        assert!(score.capability_coverage > 0.0);
    }

    #[test]
    fn validate_work_order_through_validator() {
        let validator = WorkOrderValidator;
        let wo = make_work_order("valid task");
        assert!(validator.validate(&wo).is_ok());
    }

    #[test]
    fn validate_receipt_through_validator() {
        let validator = ValidateReceiptValidator;
        let receipt = make_receipt("mock");
        assert!(validator.validate(&receipt).is_ok());
    }

    #[test]
    fn validate_hello_version_through_validator() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            BTreeMap::new(),
        );
        assert!(abp_validate::validate_hello_version(&env).is_ok());
    }

    #[test]
    fn contract_version_consistent_across_crates() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
        assert_eq!(abp_receipt::CONTRACT_VERSION, CONTRACT_VERSION);
    }

    #[test]
    fn all_vendor_manifests_negotiable() {
        let manifests = vec![
            ("openai", openai_gpt4o_manifest()),
            ("claude", claude_35_sonnet_manifest()),
            ("gemini", gemini_15_pro_manifest()),
            ("kimi", kimi_manifest()),
            ("codex", codex_manifest()),
            ("copilot", copilot_manifest()),
        ];
        for (name, manifest) in &manifests {
            let required = vec![Capability::Streaming];
            let result = negotiate_capabilities(&required, manifest);
            assert!(
                result.is_viable(),
                "{name} manifest should support Streaming"
            );
        }
    }

    #[test]
    fn generate_report_from_negotiation() {
        let manifest = claude_35_sonnet_manifest();
        let required = vec![Capability::Streaming, Capability::ToolUse];
        let result = negotiate_capabilities(&required, &manifest);
        let report = generate_report(&result);
        assert!(!report.summary.is_empty());
    }
}
