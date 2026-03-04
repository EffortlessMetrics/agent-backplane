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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(
    clippy::useless_vec,
    clippy::needless_borrows_for_generic_args,
    clippy::collapsible_if
)]
//! Deep comprehensive tests for the JSONL sidecar protocol.
//!
//! Covers every Envelope variant, tag discriminator "t", ref_id correlation,
//! CONTRACT_VERSION validation, all AgentEventKind variants, receipt hashing,
//! JSONL stream parsing, invalid input, partial lines, ordering constraints,
//! large payloads, unicode handling, and deterministic serialization.

use std::collections::BTreeMap;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile,
    ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrderBuilder, WorkspaceMode, receipt_hash,
};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn backend_full(id: &str, bv: &str, av: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some(bv.into()),
        adapter_version: Some(av.into()),
    }
}

fn hello_env() -> Envelope {
    Envelope::hello(backend("test-sidecar"), CapabilityManifest::new())
}

fn hello_env_with_caps(caps: CapabilityManifest) -> Envelope {
    Envelope::hello(backend("test-sidecar"), caps)
}

fn run_env(task: &str) -> (String, Envelope) {
    let wo = WorkOrderBuilder::new(task)
        .root(".")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    (id, env)
}

fn event_env(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn msg_event_env(ref_id: &str, text: &str) -> Envelope {
    event_env(
        ref_id,
        AgentEventKind::AssistantMessage { text: text.into() },
    )
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("test-sidecar")
            .outcome(Outcome::Complete)
            .build(),
    }
}

fn fatal_env(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    }
}

fn encode(env: &Envelope) -> String {
    JsonlCodec::encode(env).unwrap()
}

fn decode(line: &str) -> Envelope {
    JsonlCodec::decode(line).unwrap()
}

fn roundtrip(env: &Envelope) -> Envelope {
    let json = encode(env);
    JsonlCodec::decode(json.trim()).unwrap()
}

fn to_value(env: &Envelope) -> serde_json::Value {
    serde_json::to_value(env).unwrap()
}

// ===========================================================================
// 1. Envelope serialization roundtrips for all variants
// ===========================================================================

#[test]
fn roundtrip_hello() {
    let env = hello_env();
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Hello { .. }));
}

#[test]
fn roundtrip_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let env = hello_env_with_caps(caps);
    let rt = roundtrip(&env);
    if let Envelope::Hello { capabilities, .. } = rt {
        assert!(capabilities.contains_key(&Capability::ToolRead));
        assert!(capabilities.contains_key(&Capability::Streaming));
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn roundtrip_hello_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        backend("pt"),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let rt = roundtrip(&env);
    if let Envelope::Hello { mode, .. } = rt {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn roundtrip_run() {
    let (_, env) = run_env("test task");
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Run { .. }));
}

#[test]
fn roundtrip_run_preserves_task() {
    let (_, env) = run_env("Refactor the auth module");
    let rt = roundtrip(&env);
    if let Envelope::Run { work_order, .. } = rt {
        assert_eq!(work_order.task, "Refactor the auth module");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn roundtrip_run_preserves_id() {
    let (id, env) = run_env("id check");
    let rt = roundtrip(&env);
    if let Envelope::Run { id: rt_id, .. } = rt {
        assert_eq!(rt_id, id);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn roundtrip_event_assistant_message() {
    let env = msg_event_env("run-1", "hello world");
    let rt = roundtrip(&env);
    if let Envelope::Event { ref_id, event } = rt {
        assert_eq!(ref_id, "run-1");
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantMessage { .. }
        ));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_event_assistant_delta() {
    let env = event_env(
        "run-1",
        AgentEventKind::AssistantDelta { text: "tok".into() },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantDelta { text } = event.kind {
            assert_eq!(text, "tok");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn roundtrip_final() {
    let env = final_env("run-1");
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Final { .. }));
}

#[test]
fn roundtrip_final_preserves_ref_id() {
    let env = final_env("run-abc-123");
    let rt = roundtrip(&env);
    if let Envelope::Final { ref_id, .. } = rt {
        assert_eq!(ref_id, "run-abc-123");
    } else {
        panic!("expected Final");
    }
}

#[test]
fn roundtrip_fatal_with_ref_id() {
    let env = fatal_env(Some("run-1"), "something broke");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { ref_id, error, .. } = rt {
        assert_eq!(ref_id, Some("run-1".into()));
        assert_eq!(error, "something broke");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn roundtrip_fatal_without_ref_id() {
    let env = fatal_env(None, "startup error");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { ref_id, error, .. } = rt {
        assert!(ref_id.is_none());
        assert_eq!(error, "startup error");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn roundtrip_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error_code, .. } = rt {
        assert_eq!(
            error_code,
            Some(abp_error::ErrorCode::ProtocolVersionMismatch)
        );
    } else {
        panic!("expected Fatal");
    }
}

// ===========================================================================
// 2. Tag field "t" discriminator verification
// ===========================================================================

#[test]
fn hello_json_has_t_field() {
    let v = to_value(&hello_env());
    assert_eq!(v["t"], "hello");
}

#[test]
fn run_json_has_t_field() {
    let (_, env) = run_env("t field test");
    let v = to_value(&env);
    assert_eq!(v["t"], "run");
}

#[test]
fn event_json_has_t_field() {
    let env = msg_event_env("r1", "test");
    let v = to_value(&env);
    assert_eq!(v["t"], "event");
}

#[test]
fn final_json_has_t_field() {
    let env = final_env("r1");
    let v = to_value(&env);
    assert_eq!(v["t"], "final");
}

#[test]
fn fatal_json_has_t_field() {
    let env = fatal_env(None, "err");
    let v = to_value(&env);
    assert_eq!(v["t"], "fatal");
}

#[test]
fn tag_field_is_t_not_type() {
    let env = hello_env();
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
    assert!(!json.contains(r#""type":"hello""#));
}

#[test]
fn all_variants_use_snake_case_tag() {
    let tags: Vec<&str> = vec!["hello", "run", "event", "final", "fatal"];
    let envelopes: Vec<Envelope> = vec![
        hello_env(),
        run_env("x").1,
        msg_event_env("r", "x"),
        final_env("r"),
        fatal_env(None, "x"),
    ];
    for (env, expected_tag) in envelopes.iter().zip(tags.iter()) {
        let v = to_value(env);
        assert_eq!(v["t"].as_str().unwrap(), *expected_tag);
    }
}

#[test]
fn decode_with_explicit_t_hello() {
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = decode(json);
    assert!(matches!(env, Envelope::Hello { .. }));
}

#[test]
fn decode_with_explicit_t_fatal() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    let env = decode(json);
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn decode_fails_with_wrong_tag_name() {
    let json = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

#[test]
fn decode_fails_with_unknown_variant() {
    let json = r#"{"t":"unknown_variant","data":123}"#;
    let result = JsonlCodec::decode(json);
    assert!(result.is_err());
}

// ===========================================================================
// 3. Hello envelope validation (contract version check)
// ===========================================================================

#[test]
fn hello_contains_contract_version() {
    let env = hello_env();
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_factory_always_sets_current_version() {
    let env = Envelope::hello(backend("any"), CapabilityManifest::new());
    if let Envelope::Hello {
        contract_version, ..
    } = env
    {
        assert_eq!(contract_version, "abp/v0.1");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_roundtrip_preserves_contract_version() {
    let env = hello_env();
    let rt = roundtrip(&env);
    if let Envelope::Hello {
        contract_version, ..
    } = rt
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_custom_version_roundtrip() {
    let env = Envelope::Hello {
        contract_version: "abp/v99.0".into(),
        backend: backend("test"),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    let rt = roundtrip(&env);
    if let Envelope::Hello {
        contract_version, ..
    } = rt
    {
        assert_eq!(contract_version, "abp/v99.0");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn hello_mode_defaults_to_mapped() {
    let env = Envelope::hello(backend("test"), CapabilityManifest::new());
    if let Envelope::Hello { mode, .. } = env {
        assert_eq!(mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn hello_passthrough_mode_serializes_correctly() {
    let env = Envelope::hello_with_mode(
        backend("pt"),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let v = to_value(&env);
    assert_eq!(v["mode"], "passthrough");
}

#[test]
fn hello_backend_identity_fields() {
    let env = Envelope::hello(
        backend_full("sidecar:node", "2.0.0", "0.5.0"),
        CapabilityManifest::new(),
    );
    let v = to_value(&env);
    assert_eq!(v["backend"]["id"], "sidecar:node");
    assert_eq!(v["backend"]["backend_version"], "2.0.0");
    assert_eq!(v["backend"]["adapter_version"], "0.5.0");
}

#[test]
fn hello_with_empty_capabilities_serializes() {
    let env = hello_env();
    let v = to_value(&env);
    assert_eq!(v["capabilities"], json!({}));
}

#[test]
fn hello_with_many_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::McpClient, SupportLevel::Unsupported);
    let env = hello_env_with_caps(caps.clone());
    let rt = roundtrip(&env);
    if let Envelope::Hello { capabilities, .. } = rt {
        assert_eq!(capabilities.len(), caps.len());
        assert!(matches!(
            capabilities.get(&Capability::ToolBash),
            Some(SupportLevel::Emulated)
        ));
    } else {
        panic!("expected Hello");
    }
}

// ===========================================================================
// 4. Run envelope with full WorkOrder payload
// ===========================================================================

#[test]
fn run_contains_work_order_id() {
    let (id, env) = run_env("task");
    if let Envelope::Run {
        id: run_id,
        work_order,
    } = env
    {
        assert_eq!(run_id, id);
        assert_eq!(work_order.id.to_string(), id);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_preserves_full_work_order() {
    let wo = WorkOrderBuilder::new("comprehensive task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .workspace_mode(WorkspaceMode::Staged)
        .include(vec!["*.rs".into(), "*.toml".into()])
        .exclude(vec!["target/**".into()])
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "focus on error handling".into(),
            }],
        })
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into(), "write".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec!["/etc/**".into()],
            deny_write: vec!["/sys/**".into()],
            allow_network: vec!["api.example.com".into()],
            deny_network: vec!["evil.com".into()],
            require_approval_for: vec!["delete_file".into()],
        })
        .model("claude-3-opus")
        .max_turns(50)
        .max_budget_usd(5.0)
        .build();

    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };

    let rt = roundtrip(&env);
    if let Envelope::Run { work_order: wo, .. } = rt {
        assert_eq!(wo.task, "comprehensive task");
        assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
        assert_eq!(wo.workspace.root, "/workspace");
        assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
        assert_eq!(wo.workspace.include, vec!["*.rs", "*.toml"]);
        assert_eq!(wo.workspace.exclude, vec!["target/**"]);
        assert_eq!(wo.context.files, vec!["src/main.rs"]);
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.policy.allowed_tools, vec!["read", "write"]);
        assert_eq!(wo.policy.disallowed_tools, vec!["bash"]);
        assert_eq!(wo.config.model, Some("claude-3-opus".into()));
        assert_eq!(wo.config.max_turns, Some(50));
        assert_eq!(wo.config.max_budget_usd, Some(5.0));
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_minimal_work_order() {
    let wo = WorkOrderBuilder::new("minimal").build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Run { .. }));
}

#[test]
fn run_work_order_with_vendor_config() {
    let mut vendor = BTreeMap::new();
    vendor.insert("abp".into(), json!({"mode": "passthrough"}));
    vendor.insert("openai".into(), json!({"temperature": 0.7}));

    let wo = WorkOrderBuilder::new("vendor config")
        .config(RuntimeConfig {
            model: Some("gpt-4o".into()),
            vendor,
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        })
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    let rt = roundtrip(&env);
    if let Envelope::Run { work_order, .. } = rt {
        assert_eq!(work_order.config.vendor["openai"]["temperature"], 0.7);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_work_order_with_env_vars() {
    let mut env_vars = BTreeMap::new();
    env_vars.insert("API_KEY".into(), "secret".into());
    env_vars.insert("DEBUG".into(), "1".into());

    let wo = WorkOrderBuilder::new("env test")
        .config(RuntimeConfig {
            model: None,
            vendor: BTreeMap::new(),
            env: env_vars,
            max_budget_usd: None,
            max_turns: None,
        })
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    let rt = roundtrip(&env);
    if let Envelope::Run { work_order, .. } = rt {
        assert_eq!(work_order.config.env["API_KEY"], "secret");
        assert_eq!(work_order.config.env["DEBUG"], "1");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn run_work_order_with_requirements() {
    let wo = WorkOrderBuilder::new("caps required")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    let rt = roundtrip(&env);
    if let Envelope::Run { work_order, .. } = rt {
        assert_eq!(work_order.requirements.required.len(), 2);
    } else {
        panic!("expected Run");
    }
}

// ===========================================================================
// 5. Event envelope with all AgentEventKind variants
// ===========================================================================

#[test]
fn event_run_started() {
    let env = event_env(
        "r1",
        AgentEventKind::RunStarted {
            message: "Starting run".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::RunStarted { message } = event.kind {
            assert_eq!(message, "Starting run");
        } else {
            panic!("expected RunStarted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_run_completed() {
    let env = event_env(
        "r1",
        AgentEventKind::RunCompleted {
            message: "Done".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        assert!(matches!(event.kind, AgentEventKind::RunCompleted { .. }));
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_delta() {
    let env = event_env(
        "r1",
        AgentEventKind::AssistantDelta {
            text: "streaming token".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantDelta { text } = event.kind {
            assert_eq!(text, "streaming token");
        } else {
            panic!("expected AssistantDelta");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_assistant_message() {
    let env = event_env(
        "r1",
        AgentEventKind::AssistantMessage {
            text: "full message".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, "full message");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs"}),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } = event.kind
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id, Some("tu-1".into()));
            assert!(parent_tool_use_id.is_none());
            assert_eq!(input["path"], "src/lib.rs");
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_call_with_parent() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu-2".into()),
            parent_tool_use_id: Some("tu-1".into()),
            input: json!({"path": "out.txt", "content": "data"}),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::ToolCall {
            parent_tool_use_id, ..
        } = event.kind
        {
            assert_eq!(parent_tool_use_id, Some("tu-1".into()));
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_success() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::ToolResult {
            is_error, output, ..
        } = event.kind
        {
            assert!(!is_error);
            assert_eq!(output["content"], "fn main() {}");
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_tool_result_error() {
    let env = event_env(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: json!({"error": "permission denied"}),
            is_error: true,
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::ToolResult { is_error, .. } = event.kind {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_file_changed() {
    let env = event_env(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added new function".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::FileChanged { path, summary } = event.kind {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(summary, "Added new function");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed() {
    let env = event_env(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } = event.kind
        {
            assert_eq!(command, "cargo test");
            assert_eq!(exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("test result: ok"));
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_command_executed_no_exit_code() {
    let env = event_env(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "sleep 100".into(),
            exit_code: None,
            output_preview: None,
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::CommandExecuted {
            exit_code,
            output_preview,
            ..
        } = event.kind
        {
            assert!(exit_code.is_none());
            assert!(output_preview.is_none());
        } else {
            panic!("expected CommandExecuted");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_warning() {
    let env = event_env(
        "r1",
        AgentEventKind::Warning {
            message: "deprecated API".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::Warning { message } = event.kind {
            assert_eq!(message, "deprecated API");
        } else {
            panic!("expected Warning");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error() {
    let env = event_env(
        "r1",
        AgentEventKind::Error {
            message: "something went wrong".into(),
            error_code: None,
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::Error {
            message,
            error_code,
        } = event.kind
        {
            assert_eq!(message, "something went wrong");
            assert!(error_code.is_none());
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_error_with_code() {
    let env = event_env(
        "r1",
        AgentEventKind::Error {
            message: "policy denied".into(),
            error_code: Some(abp_error::ErrorCode::PolicyDenied),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::Error { error_code, .. } = event.kind {
            assert_eq!(error_code, Some(abp_error::ErrorCode::PolicyDenied));
        } else {
            panic!("expected Error");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_with_extension_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        json!({"role": "assistant", "content": "hi"}),
    );
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        },
    };
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        let ext = event.ext.unwrap();
        assert_eq!(ext["raw_message"]["role"], "assistant");
    } else {
        panic!("expected Event");
    }
}

#[test]
fn event_type_tag_uses_type_not_t() {
    // AgentEventKind uses #[serde(tag = "type")] — different from Envelope's "t"
    let env = msg_event_env("r1", "test");
    let v = to_value(&env);
    assert_eq!(v["t"], "event"); // Envelope discriminator
    assert_eq!(v["event"]["type"], "assistant_message"); // AgentEventKind discriminator
}

// ===========================================================================
// 6. Final envelope with Receipt (including hash)
// ===========================================================================

#[test]
fn final_receipt_has_backend_id() {
    let env = final_env("r1");
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert_eq!(receipt.backend.id, "test-sidecar");
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_hash() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();

    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.clone().unwrap();
    assert_eq!(hash.len(), 64);

    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert_eq!(receipt.receipt_sha256.unwrap().len(), 64);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_hash_deterministic() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();

    let hash1 = receipt_hash(&receipt).unwrap();
    let hash2 = receipt_hash(&receipt).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn final_receipt_hash_excludes_self() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::nil())
        .build();

    let hash_before = receipt_hash(&receipt).unwrap();

    let mut receipt_with = receipt.clone();
    receipt_with.receipt_sha256 = Some("fake_hash_12345".into());
    let hash_after = receipt_hash(&receipt_with).unwrap();

    // Hash should be the same because receipt_sha256 is nulled before hashing
    assert_eq!(hash_before, hash_after);
}

#[test]
fn final_receipt_with_trace_events() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();

    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert_eq!(receipt.trace.len(), 3);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_artifacts() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Partial)
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "changes.diff".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();

    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert_eq!(receipt.artifacts.len(), 2);
        assert_eq!(receipt.artifacts[0].kind, "patch");
        assert_eq!(receipt.outcome, Outcome::Partial);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_verification() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .verification(VerificationReport {
            git_diff: Some("diff --git a/f.rs b/f.rs\n+fn new()".into()),
            git_status: Some("M src/f.rs\n".into()),
            harness_ok: true,
        })
        .build();

    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert!(receipt.verification.harness_ok);
        assert!(receipt.verification.git_diff.is_some());
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_with_usage() {
    let receipt = ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .usage_raw(json!({"prompt_tokens": 100, "completion_tokens": 50}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        })
        .build();

    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert_eq!(receipt.usage.input_tokens, Some(100));
        assert_eq!(receipt.usage.output_tokens, Some(50));
        assert_eq!(receipt.usage.estimated_cost_usd, Some(0.01));
    } else {
        panic!("expected Final");
    }
}

#[test]
fn final_receipt_outcomes_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let receipt = ReceiptBuilder::new("test-sidecar")
            .outcome(outcome.clone())
            .build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let rt = roundtrip(&env);
        if let Envelope::Final { receipt, .. } = rt {
            assert_eq!(receipt.outcome, outcome);
        } else {
            panic!("expected Final");
        }
    }
}

#[test]
fn final_receipt_execution_modes() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let receipt = ReceiptBuilder::new("test-sidecar").mode(mode).build();
        let env = Envelope::Final {
            ref_id: "r1".into(),
            receipt,
        };
        let rt = roundtrip(&env);
        if let Envelope::Final { receipt, .. } = rt {
            assert_eq!(receipt.mode, mode);
        } else {
            panic!("expected Final");
        }
    }
}

// ===========================================================================
// 7. Fatal envelope with error messages
// ===========================================================================

#[test]
fn fatal_simple_error() {
    let env = fatal_env(None, "out of memory");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error, "out of memory");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_with_ref_id() {
    let env = fatal_env(Some("run-42"), "crash");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { ref_id, error, .. } = rt {
        assert_eq!(ref_id, Some("run-42".into()));
        assert_eq!(error, "crash");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_null_ref_id_serialization() {
    let env = fatal_env(None, "err");
    let v = to_value(&env);
    assert!(v["ref_id"].is_null());
}

#[test]
fn fatal_error_code_skipped_when_none() {
    let env = fatal_env(None, "err");
    let json = serde_json::to_string(&env).unwrap();
    assert!(!json.contains("error_code"));
}

#[test]
fn fatal_error_code_present_when_some() {
    let env = Envelope::fatal_with_code(
        None,
        "invalid",
        abp_error::ErrorCode::ProtocolInvalidEnvelope,
    );
    let v = to_value(&env);
    assert!(v["error_code"].is_string());
}

#[test]
fn fatal_error_code_accessor() {
    let env = Envelope::fatal_with_code(None, "boom", abp_error::ErrorCode::BackendCrashed);
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendCrashed));
}

#[test]
fn fatal_error_code_accessor_none_for_other_variants() {
    let env = hello_env();
    assert!(env.error_code().is_none());
}

#[test]
fn fatal_empty_error_message() {
    let env = fatal_env(None, "");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error, "");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_multiline_error_message() {
    let msg = "Error on line 1\nError on line 2\nStack trace follows";
    let env = fatal_env(Some("r1"), msg);
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error, msg);
    } else {
        panic!("expected Fatal");
    }
}

// ===========================================================================
// 8. ref_id correlation enforcement
// ===========================================================================

#[test]
fn event_ref_id_matches_run_id() {
    let (run_id, _) = run_env("ref test");
    let event = msg_event_env(&run_id, "correlated");
    if let Envelope::Event { ref_id, .. } = &event {
        assert_eq!(ref_id, &run_id);
    }
}

#[test]
fn final_ref_id_matches_run_id() {
    let (run_id, _) = run_env("ref test");
    let fin = final_env(&run_id);
    if let Envelope::Final { ref_id, .. } = &fin {
        assert_eq!(ref_id, &run_id);
    }
}

#[test]
fn fatal_ref_id_matches_run_id() {
    let (run_id, _) = run_env("ref test");
    let fat = fatal_env(Some(&run_id), "error");
    if let Envelope::Fatal { ref_id, .. } = &fat {
        assert_eq!(ref_id.as_deref(), Some(run_id.as_str()));
    }
}

#[test]
fn ref_id_preserved_through_roundtrip() {
    let ref_id = "custom-ref-abc-123";
    let env = msg_event_env(ref_id, "data");
    let rt = roundtrip(&env);
    if let Envelope::Event { ref_id: rt_ref, .. } = rt {
        assert_eq!(rt_ref, ref_id);
    }
}

#[test]
fn ref_id_is_string_in_json() {
    let env = msg_event_env("r1", "x");
    let v = to_value(&env);
    assert!(v["ref_id"].is_string());
}

#[test]
fn ref_id_uuid_format() {
    let wo = WorkOrderBuilder::new("uuid ref").build();
    let uuid_str = wo.id.to_string();
    let env = msg_event_env(&uuid_str, "ok");
    let rt = roundtrip(&env);
    if let Envelope::Event { ref_id, .. } = rt {
        assert!(Uuid::parse_str(&ref_id).is_ok());
    }
}

#[test]
fn multiple_events_same_ref_id() {
    let ref_id = "run-1";
    let events: Vec<Envelope> = (0..5)
        .map(|i| msg_event_env(ref_id, &format!("msg {i}")))
        .collect();

    for env in &events {
        if let Envelope::Event { ref_id: r, .. } = env {
            assert_eq!(r, ref_id);
        }
    }
}

#[test]
fn final_and_events_share_ref_id() {
    let ref_id = "shared-ref";
    let ev = msg_event_env(ref_id, "data");
    let fin = final_env(ref_id);

    if let Envelope::Event { ref_id: ev_ref, .. } = &ev
        && let Envelope::Final {
            ref_id: fin_ref, ..
        } = &fin
    {
        assert_eq!(ev_ref, fin_ref);
    }
}

// ===========================================================================
// 9. Invalid JSON handling
// ===========================================================================

#[test]
fn decode_empty_string() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_whitespace_only() {
    let result = JsonlCodec::decode("   ");
    assert!(result.is_err());
}

#[test]
fn decode_plain_text() {
    let result = JsonlCodec::decode("not valid json at all");
    assert!(result.is_err());
}

#[test]
fn decode_valid_json_but_missing_t() {
    let result = JsonlCodec::decode(r#"{"hello": "world"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_array_instead_of_object() {
    let result = JsonlCodec::decode("[1, 2, 3]");
    assert!(result.is_err());
}

#[test]
fn decode_number() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

#[test]
fn decode_null() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

#[test]
fn decode_boolean() {
    let result = JsonlCodec::decode("true");
    assert!(result.is_err());
}

#[test]
fn decode_incomplete_json() {
    let result = JsonlCodec::decode(
        r#"{"t":"hello","contract_version":#);
    assert!(result.is_err());
}

#[test]
fn decode_extra_comma() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":null,"error":"x",}"#,
    );
    assert!(result.is_err());
}

#[test]
fn decode_missing_required_fields() {
    // hello without contract_version
    let result = JsonlCodec::decode(
        r#"{"t":"hello","backend":{"id":"x","backend_version":null,"adapter_version":null}}"#,
    );
    assert!(result.is_err());
}

#[test]
fn decode_wrong_field_types() {
    let result = JsonlCodec::decode(r#"{"t":"fatal","ref_id":123,"error":"x"}"#);
    assert!(result.is_err());
}

// ===========================================================================
// 10. Partial line handling
// ===========================================================================

#[test]
fn encode_always_ends_with_newline() {
    let env = hello_env();
    let line = encode(&env);
    assert!(line.ends_with('\n'));
}

#[test]
fn decode_works_without_trailing_newline() {
    let env = hello_env();
    let line = encode(&env);
    // Remove trailing newline
    let trimmed = line.trim_end().to_string();
    let decoded = JsonlCodec::decode(&trimmed);
    assert!(decoded.is_ok());
}

#[test]
fn decode_works_with_trailing_whitespace() {
    let env = hello_env();
    let line = encode(&env);
    let padded = format!("{}   ", line.trim());
    // decode should handle trimmed input
    let decoded = JsonlCodec::decode(padded.trim());
    assert!(decoded.is_ok());
}

#[test]
fn stream_skips_blank_lines() {
    let hello = encode(&hello_env());
    let fatal = encode(&fatal_env(None, "err"));
    let input = format!("{hello}\n\n{fatal}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn stream_handles_only_blank_lines() {
    let input = "\n\n\n\n";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 0);
}

#[test]
fn stream_handles_empty_input() {
    let input = "";
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 0);
}

// ===========================================================================
// 11. Multiple envelopes in sequence
// ===========================================================================

#[test]
fn full_session_sequence() {
    let hello = hello_env();
    let (run_id, run) = run_env("full session");
    let evt1 = event_env(
        &run_id,
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let evt2 = msg_event_env(&run_id, "working on it");
    let evt3 = event_env(
        &run_id,
        AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        },
    );
    let evt4 = event_env(
        &run_id,
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: json!({"content": "fn main() {}"}),
            is_error: false,
        },
    );
    let fin = final_env(&run_id);

    let mut buf = Vec::new();
    let envelopes = [&hello, &run, &evt1, &evt2, &evt3, &evt4, &fin];
    JsonlCodec::encode_many_to_writer(
        &mut buf,
        &envelopes.iter().cloned().cloned().collect::<Vec<_>>(),
    )
    .unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 7);
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Event { .. }));
    assert!(matches!(decoded[4], Envelope::Event { .. }));
    assert!(matches!(decoded[5], Envelope::Event { .. }));
    assert!(matches!(decoded[6], Envelope::Final { .. }));
}

#[test]
fn multiple_envelopes_encode_to_writer() {
    let envs = vec![
        hello_env(),
        fatal_env(None, "err1"),
        fatal_env(None, "err2"),
    ];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.trim().split('\n').collect();
    assert_eq!(lines.len(), 3);
}

#[test]
fn encode_to_writer_single() {
    let env = hello_env();
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.ends_with('\n'));
    assert!(output.contains("\"t\":\"hello\""));
}

#[test]
fn decode_stream_multi_line_input() {
    let hello_line = encode(&hello_env());
    let fatal_line = encode(&fatal_env(None, "err"));
    let input = format!("{hello_line}{fatal_line}");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn session_with_fatal_instead_of_final() {
    let hello = hello_env();
    let (run_id, run) = run_env("failing session");
    let evt = event_env(
        &run_id,
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let fat = fatal_env(Some(&run_id), "sidecar crashed");

    let envs = vec![hello, run, evt, fat];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 4);
    assert!(matches!(decoded[3], Envelope::Fatal { .. }));
}

#[test]
fn many_events_in_sequence() {
    let ref_id = "run-many";
    let events: Vec<Envelope> = (0..100)
        .map(|i| msg_event_env(ref_id, &format!("token {i}")))
        .collect();

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &events).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded.len(), 100);
}

// ===========================================================================
// 12. Envelope ordering constraints (hello must be first)
// ===========================================================================

#[test]
fn hello_is_first_in_valid_session() {
    let envs = vec![
        hello_env(),
        run_env("task").1,
        msg_event_env("r1", "data"),
        final_env("r1"),
    ];
    assert!(matches!(envs[0], Envelope::Hello { .. }));
}

#[test]
fn can_detect_hello_variant() {
    let hello = hello_env();
    let is_hello = matches!(&hello, Envelope::Hello { .. });
    assert!(is_hello);

    let (_, run) = run_env("x");
    let is_hello = matches!(&run, Envelope::Hello { .. });
    assert!(!is_hello);
}

#[test]
fn hello_before_any_run() {
    let sequence = vec![hello_env(), run_env("first").1];
    assert!(matches!(sequence[0], Envelope::Hello { .. }));
    assert!(matches!(sequence[1], Envelope::Run { .. }));
}

#[test]
fn fatal_can_come_before_hello() {
    // Fatal with no ref_id can be sent before hello if sidecar fails to start
    let env = fatal_env(None, "failed to initialize");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { ref_id, .. } = rt {
        assert!(ref_id.is_none());
    }
}

#[test]
fn stream_ordering_hello_run_events_final() {
    let hello = hello_env();
    let (rid, run) = run_env("ordered");
    let ev = msg_event_env(&rid, "data");
    let fin = final_env(&rid);

    let sequence = vec![hello, run, ev, fin];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &sequence).unwrap();

    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Verify ordering
    assert!(matches!(decoded[0], Envelope::Hello { .. }));
    assert!(matches!(decoded[1], Envelope::Run { .. }));
    assert!(matches!(decoded[2], Envelope::Event { .. }));
    assert!(matches!(decoded[3], Envelope::Final { .. }));
}

// ===========================================================================
// 13. Large payload handling
// ===========================================================================

#[test]
fn large_assistant_message() {
    let large_text = "x".repeat(100_000);
    let env = msg_event_env("r1", &large_text);
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text.len(), 100_000);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_tool_output() {
    let big_output = serde_json::Value::String("y".repeat(50_000));
    let env = event_env(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: None,
            output: big_output.clone(),
            is_error: false,
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::ToolResult { output, .. } = event.kind {
            assert_eq!(output.as_str().unwrap().len(), 50_000);
        } else {
            panic!("expected ToolResult");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn large_work_order_with_many_context_files() {
    let files: Vec<String> = (0..1000).map(|i| format!("src/file_{i}.rs")).collect();
    let wo = WorkOrderBuilder::new("big context")
        .context(ContextPacket {
            files,
            snippets: vec![],
        })
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run {
        id: id.clone(),
        work_order: wo,
    };
    let rt = roundtrip(&env);
    if let Envelope::Run { work_order, .. } = rt {
        assert_eq!(work_order.context.files.len(), 1000);
    } else {
        panic!("expected Run");
    }
}

#[test]
fn large_receipt_with_many_trace_events() {
    let mut builder = ReceiptBuilder::new("test-sidecar").outcome(Outcome::Complete);
    for i in 0..500 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    if let Envelope::Final { receipt, .. } = rt {
        assert_eq!(receipt.trace.len(), 500);
    } else {
        panic!("expected Final");
    }
}

#[test]
fn large_error_message() {
    let msg = "E".repeat(1_000_000);
    let env = fatal_env(None, &msg);
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error.len(), 1_000_000);
    } else {
        panic!("expected Fatal");
    }
}

// ===========================================================================
// 14. Unicode in envelope fields
// ===========================================================================

#[test]
fn unicode_in_task() {
    let wo = WorkOrderBuilder::new("修复认证模块 🛠️").build();
    let id = wo.id.to_string();
    let env = Envelope::Run { id, work_order: wo };
    let rt = roundtrip(&env);
    if let Envelope::Run { work_order, .. } = rt {
        assert_eq!(work_order.task, "修复认证模块 🛠️");
    } else {
        panic!("expected Run");
    }
}

#[test]
fn unicode_in_assistant_message() {
    let env = msg_event_env("r1", "こんにちは世界 🌍");
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, "こんにちは世界 🌍");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn unicode_in_error_message() {
    let env = fatal_env(None, "Ошибка: файл не найден 📁");
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error, "Ошибка: файл не найден 📁");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn unicode_in_backend_id() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "sidecar:日本語".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let rt = roundtrip(&env);
    if let Envelope::Hello { backend, .. } = rt {
        assert_eq!(backend.id, "sidecar:日本語");
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn unicode_in_file_path() {
    let env = event_env(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/données/résultat.rs".into(),
            summary: "Änderung an der Datei".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::FileChanged { path, summary } = event.kind {
            assert_eq!(path, "src/données/résultat.rs");
            assert_eq!(summary, "Änderung an der Datei");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn emoji_in_warning() {
    let env = event_env(
        "r1",
        AgentEventKind::Warning {
            message: "⚠️ Rate limit approaching 🔥".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::Warning { message } = event.kind {
            assert!(message.contains("⚠️"));
            assert!(message.contains("🔥"));
        } else {
            panic!("expected Warning");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn unicode_escape_sequences_in_json() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"\u0048\u0065\u006c\u006c\u006f"}"#;
    let env = decode(json);
    if let Envelope::Fatal { error, .. } = env {
        assert_eq!(error, "Hello");
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn mixed_scripts_roundtrip() {
    let text = "Hello مرحبا こんにちは Привет 你好 🌐";
    let env = msg_event_env("r1", text);
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text: t } = event.kind {
            assert_eq!(t, text);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

// ===========================================================================
// 15. Empty and minimal envelopes
// ===========================================================================

#[test]
fn minimal_hello() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Hello { .. }));
}

#[test]
fn minimal_run() {
    let wo = WorkOrderBuilder::new("x").build();
    let id = wo.id.to_string();
    let env = Envelope::Run { id, work_order: wo };
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Run { .. }));
}

#[test]
fn minimal_event() {
    let env = msg_event_env("r", "");
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, "");
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn minimal_fatal() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "".into(),
        error_code: None,
    };
    let rt = roundtrip(&env);
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = rt
    {
        assert!(ref_id.is_none());
        assert_eq!(error, "");
        assert!(error_code.is_none());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn minimal_final() {
    let receipt = ReceiptBuilder::new("m").build();
    let env = Envelope::Final {
        ref_id: "r".into(),
        receipt,
    };
    let rt = roundtrip(&env);
    assert!(matches!(rt, Envelope::Final { .. }));
}

#[test]
fn empty_capabilities_manifest() {
    let caps = CapabilityManifest::new();
    assert!(caps.is_empty());
    let env = hello_env_with_caps(caps);
    let v = to_value(&env);
    assert_eq!(v["capabilities"], json!({}));
}

#[test]
fn single_char_fields() {
    let env = Envelope::Fatal {
        ref_id: Some("r".into()),
        error: "e".into(),
        error_code: None,
    };
    let rt = roundtrip(&env);
    if let Envelope::Fatal { ref_id, error, .. } = rt {
        assert_eq!(ref_id, Some("r".into()));
        assert_eq!(error, "e");
    } else {
        panic!("expected Fatal");
    }
}

// ===========================================================================
// Additional: Version negotiation
// ===========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version("abp/0.1"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/v1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn compatible_versions_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.99"));
}

#[test]
fn incompatible_versions_different_major() {
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
}

#[test]
fn incompatible_versions_invalid_format() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
    assert!(!is_compatible_version("invalid", "invalid"));
}

// ===========================================================================
// Additional: JSON structure verification
// ===========================================================================

#[test]
fn hello_json_structure() {
    let env = hello_env();
    let v = to_value(&env);
    assert!(v.is_object());
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("t"));
    assert!(obj.contains_key("contract_version"));
    assert!(obj.contains_key("backend"));
    assert!(obj.contains_key("capabilities"));
    assert!(obj.contains_key("mode"));
}

#[test]
fn run_json_structure() {
    let (_, env) = run_env("json struct");
    let v = to_value(&env);
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("t"));
    assert!(obj.contains_key("id"));
    assert!(obj.contains_key("work_order"));
}

#[test]
fn event_json_structure() {
    let env = msg_event_env("r1", "test");
    let v = to_value(&env);
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("t"));
    assert!(obj.contains_key("ref_id"));
    assert!(obj.contains_key("event"));
}

#[test]
fn final_json_structure() {
    let env = final_env("r1");
    let v = to_value(&env);
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("t"));
    assert!(obj.contains_key("ref_id"));
    assert!(obj.contains_key("receipt"));
}

#[test]
fn fatal_json_structure() {
    let env = fatal_env(None, "err");
    let v = to_value(&env);
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("t"));
    assert!(obj.contains_key("ref_id"));
    assert!(obj.contains_key("error"));
}

// ===========================================================================
// Additional: Deterministic serialization
// ===========================================================================

#[test]
fn btreemap_deterministic_serialization() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);

    let env1 = hello_env_with_caps(caps.clone());
    let env2 = hello_env_with_caps(caps);

    let json1 = serde_json::to_string(&env1).unwrap();
    let json2 = serde_json::to_string(&env2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn vendor_config_btreemap_sorted() {
    let mut vendor = BTreeMap::new();
    vendor.insert("zzz".into(), json!(1));
    vendor.insert("aaa".into(), json!(2));
    vendor.insert("mmm".into(), json!(3));

    let wo = WorkOrderBuilder::new("sorted test")
        .config(RuntimeConfig {
            model: None,
            vendor,
            env: BTreeMap::new(),
            max_budget_usd: None,
            max_turns: None,
        })
        .build();
    let id = wo.id.to_string();
    let env = Envelope::Run { id, work_order: wo };
    let json = serde_json::to_string(&env).unwrap();

    let aaa_pos = json.find("\"aaa\"").unwrap();
    let mmm_pos = json.find("\"mmm\"").unwrap();
    let zzz_pos = json.find("\"zzz\"").unwrap();
    assert!(aaa_pos < mmm_pos);
    assert!(mmm_pos < zzz_pos);
}

#[test]
fn capabilities_btreemap_sorted() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let env = hello_env_with_caps(caps);
    let json = serde_json::to_string(&env).unwrap();

    // BTreeMap<Capability, _> is ordered by Capability's Ord (discriminant order),
    // not by the snake_case string name. Declaration order: Streaming, ToolRead, ToolBash.
    let streaming_pos = json.find("\"streaming\"").unwrap();
    let tool_read_pos = json.find("\"tool_read\"").unwrap();
    let tool_bash_pos = json.find("\"tool_bash\"").unwrap();
    assert!(streaming_pos < tool_read_pos);
    assert!(tool_read_pos < tool_bash_pos);
}

// ===========================================================================
// Additional: Edge cases and cross-cutting concerns
// ===========================================================================

#[test]
fn special_characters_in_error_message() {
    let msg = r#"Error: "unexpected" token at line 1, col 5 \ / <>&"#;
    let env = fatal_env(None, msg);
    let rt = roundtrip(&env);
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error, msg);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn newline_in_error_message() {
    let msg = "line1\nline2\nline3";
    let env = fatal_env(None, msg);
    // Newlines within JSON strings are escaped in JSONL
    let line = encode(&env);
    assert!(line.trim().ends_with('}'));
    // But the decoded value has actual newlines
    let rt = decode(line.trim());
    if let Envelope::Fatal { error, .. } = rt {
        assert_eq!(error, msg);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn tab_characters_in_message() {
    let msg = "col1\tcol2\tcol3";
    let env = msg_event_env("r1", msg);
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text, msg);
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn null_bytes_in_json_string() {
    // serde_json handles null bytes in strings
    let env = msg_event_env("r1", "before\0after");
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert!(text.contains('\0'));
        } else {
            panic!("expected AssistantMessage");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn backslash_in_path() {
    let env = event_env(
        "r1",
        AgentEventKind::FileChanged {
            path: r"src\main.rs".into(),
            summary: "windows path".into(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::FileChanged { path, .. } = event.kind {
            assert_eq!(path, r"src\main.rs");
        } else {
            panic!("expected FileChanged");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn nested_json_in_tool_input() {
    let deeply_nested = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": [1, 2, {"level5": true}]
                }
            }
        }
    });
    let env = event_env(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "complex_tool".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: deeply_nested.clone(),
        },
    );
    let rt = roundtrip(&env);
    if let Envelope::Event { event, .. } = rt {
        if let AgentEventKind::ToolCall { input, .. } = event.kind {
            assert_eq!(
                input["level1"]["level2"]["level3"]["level4"][2]["level5"],
                true
            );
        } else {
            panic!("expected ToolCall");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn decode_stream_with_invalid_line_fails_at_that_line() {
    let good = encode(&hello_env());
    let bad = "not json\n";
    let input = format!("{good}{bad}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn run_id_and_work_order_id_consistency() {
    let wo = WorkOrderBuilder::new("consistency check").build();
    let wo_id = wo.id.to_string();
    let env = Envelope::Run {
        id: wo_id.clone(),
        work_order: wo,
    };
    if let Envelope::Run { id, work_order } = env {
        assert_eq!(id, work_order.id.to_string());
    }
}

#[test]
fn receipt_meta_contract_version_matches() {
    let receipt = ReceiptBuilder::new("test").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_sha256_none_by_default() {
    let receipt = ReceiptBuilder::new("test").build();
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn receipt_with_hash_produces_64_char_hex() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn all_event_kinds_have_type_tag() {
    let kinds: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted {
            message: "s".into(),
        },
        AgentEventKind::RunCompleted {
            message: "c".into(),
        },
        AgentEventKind::AssistantDelta { text: "d".into() },
        AgentEventKind::AssistantMessage { text: "m".into() },
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!({}),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "f".into(),
            summary: "s".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
            output_preview: None,
        },
        AgentEventKind::Warning {
            message: "w".into(),
        },
        AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        },
    ];

    let expected_types = vec![
        "run_started",
        "run_completed",
        "assistant_delta",
        "assistant_message",
        "tool_call",
        "tool_result",
        "file_changed",
        "command_executed",
        "warning",
        "error",
    ];

    for (kind, expected_type) in kinds.into_iter().zip(expected_types.iter()) {
        let event = AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            *expected_type,
            "AgentEventKind type tag mismatch"
        );
    }
}

#[test]
fn event_kind_type_tag_is_snake_case() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let v = serde_json::to_value(&event).unwrap();
    let type_val = v["type"].as_str().unwrap();
    assert_eq!(type_val, "run_started");
    assert!(!type_val.contains('-'));
    assert!(!type_val.chars().any(|c| c.is_uppercase()));
}

#[test]
fn encode_single_envelope_no_extra_newlines() {
    let env = hello_env();
    let encoded = encode(&env);
    // Should have exactly one newline at the end
    assert_eq!(encoded.matches('\n').count(), 1);
    assert!(encoded.ends_with('\n'));
}

#[test]
fn decode_tolerates_leading_whitespace_when_trimmed() {
    let env = hello_env();
    let line = encode(&env);
    let padded = format!("  {}", line.trim());
    let decoded = JsonlCodec::decode(padded.trim());
    assert!(decoded.is_ok());
}
