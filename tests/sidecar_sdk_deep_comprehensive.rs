#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for the abp-sidecar-sdk crate: registration helpers,
//! type definitions, serialization, protocol interactions, capability
//! declaration, WorkOrder→SDK mapping, SDK→Receipt mapping, error handling,
//! version compatibility, hello/run/event/final/fatal envelope construction,
//! event streaming, and configuration passthrough.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use abp_core::*;
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState};
use abp_host::process::{ProcessConfig, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::RetryConfig;
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_integrations::SidecarBackend;
use abp_protocol::builder::EnvelopeBuilder;
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use abp_runtime::Runtime;
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn test_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("0.1.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "hello world".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: "/tmp/test".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

fn test_receipt() -> Receipt {
    ReceiptBuilder::new("test-sidecar")
        .outcome(Outcome::Complete)
        .build()
}

fn rich_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Refactor auth module")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/workspace")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build()
}

// ===========================================================================
// 1. SDK Type Definitions and Serialization
// ===========================================================================

#[test]
fn sidecar_spec_new_defaults() {
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_with_args() {
    let mut spec = SidecarSpec::new("python");
    spec.args = vec!["host.py".into(), "--verbose".into()];
    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.args[0], "host.py");
}

#[test]
fn sidecar_spec_with_env() {
    let mut spec = SidecarSpec::new("node");
    spec.env.insert("API_KEY".into(), "secret".into());
    spec.env.insert("DEBUG".into(), "true".into());
    assert_eq!(spec.env.len(), 2);
    assert_eq!(spec.env["API_KEY"], "secret");
}

#[test]
fn sidecar_spec_with_cwd() {
    let mut spec = SidecarSpec::new("bash");
    spec.cwd = Some("/tmp/work".into());
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn sidecar_spec_serialization_roundtrip() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["host.js".into()];
    spec.env.insert("KEY".into(), "val".into());
    spec.cwd = Some("/work".into());

    let json = serde_json::to_string(&spec).unwrap();
    let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.command, "node");
    assert_eq!(deser.args, vec!["host.js"]);
    assert_eq!(deser.env["KEY"], "val");
    assert_eq!(deser.cwd.as_deref(), Some("/work"));
}

#[test]
fn sidecar_spec_debug_output() {
    let spec = SidecarSpec::new("node");
    let debug = format!("{:?}", spec);
    assert!(debug.contains("node"));
}

#[test]
fn sidecar_spec_clone() {
    let spec = SidecarSpec::new("node");
    let cloned = spec.clone();
    assert_eq!(cloned.command, spec.command);
}

#[test]
fn sidecar_hello_serialization() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_identity(),
        capabilities: test_capabilities(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains("test-sidecar"));
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn sidecar_hello_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_identity(),
        capabilities: test_capabilities(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let deser: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.contract_version, CONTRACT_VERSION);
    assert_eq!(deser.backend.id, "test-sidecar");
}

#[test]
fn sidecar_hello_empty_capabilities() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_identity(),
        capabilities: CapabilityManifest::new(),
    };
    assert!(hello.capabilities.is_empty());
}

// ===========================================================================
// 2. Sidecar Registration and Discovery
// ===========================================================================

#[test]
fn sidecar_config_new() {
    let cfg = SidecarConfig::new("my-sidecar", "node");
    assert_eq!(cfg.name, "my-sidecar");
    assert_eq!(cfg.command, "node");
    assert!(cfg.args.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.working_dir.is_none());
}

#[test]
fn sidecar_config_validate_ok() {
    let cfg = SidecarConfig::new("test", "node");
    assert!(cfg.validate().is_ok());
}

#[test]
fn sidecar_config_validate_empty_name() {
    let cfg = SidecarConfig::new("", "node");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_validate_empty_command() {
    let cfg = SidecarConfig::new("test", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_to_spec() {
    let mut cfg = SidecarConfig::new("my-sidecar", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("KEY".into(), "val".into());
    cfg.working_dir = Some(PathBuf::from("/tmp/work"));

    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env["KEY"], "val");
    assert!(spec.cwd.is_some());
}

#[test]
fn sidecar_config_to_spec_no_cwd() {
    let cfg = SidecarConfig::new("test", "python");
    let spec = cfg.to_spec();
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_registry_empty() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
    assert!(reg.get("node").is_none());
}

#[test]
fn sidecar_registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    let cfg = SidecarConfig::new("node", "node");
    reg.register(cfg).unwrap();
    assert!(reg.get("node").is_some());
    assert_eq!(reg.get("node").unwrap().command, "node");
}

#[test]
fn sidecar_registry_register_duplicate_fails() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    let err = reg.register(SidecarConfig::new("node", "node"));
    assert!(err.is_err());
}

#[test]
fn sidecar_registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("python", "python"))
        .unwrap();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    reg.register(SidecarConfig::new("bash", "bash")).unwrap();
    let names = reg.list();
    assert_eq!(names, vec!["bash", "node", "python"]);
}

#[test]
fn sidecar_registry_remove() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    assert!(reg.remove("node"));
    assert!(!reg.remove("node"));
    assert!(reg.get("node").is_none());
}

#[test]
fn sidecar_registry_remove_nonexistent() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("doesnotexist"));
}

#[test]
fn sidecar_registry_discover_from_dir() {
    let dir = tempfile::tempdir().unwrap();
    let node_dir = dir.path().join("my_node");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// node sidecar").unwrap();

    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.get("my_node").is_some());
    assert_eq!(reg.get("my_node").unwrap().command, "node");
}

#[test]
fn sidecar_registry_discover_python_host() {
    let dir = tempfile::tempdir().unwrap();
    let py_dir = dir.path().join("my_python");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "# python sidecar").unwrap();

    let reg = SidecarRegistry::discover_from_dir(dir.path()).unwrap();
    assert!(reg.get("my_python").is_some());
    assert_eq!(reg.get("my_python").unwrap().command, "python");
}

#[test]
fn sidecar_registry_discover_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn sidecar_registry_discover_ignores_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("not_a_dir.js"), "").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn sidecar_registry_discover_ignores_dirs_without_host() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("empty_sidecar");
    std::fs::create_dir(&sub).unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

// ===========================================================================
// 3. Capability Declaration via SDK Types
// ===========================================================================

#[test]
fn capability_manifest_empty() {
    let m = CapabilityManifest::new();
    assert!(m.is_empty());
}

#[test]
fn capability_manifest_insert_and_query() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    assert!(m.contains_key(&Capability::ToolRead));
    assert!(m.contains_key(&Capability::Streaming));
    assert!(!m.contains_key(&Capability::ToolWrite));
}

#[test]
fn capability_manifest_all_capabilities() {
    let mut m = CapabilityManifest::new();
    let caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ];
    for cap in &caps {
        m.insert(cap.clone(), SupportLevel::Native);
    }
    assert_eq!(m.len(), caps.len());
}

#[test]
fn support_level_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_emulated() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_unsupported() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_satisfies_restricted() {
    let restricted = SupportLevel::Restricted {
        reason: "rate limited".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn capability_manifest_serialization() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    let json = serde_json::to_string(&m).unwrap();
    let deser: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.len(), 2);
}

// ===========================================================================
// 4. WorkOrder → SDK Request Mapping
// ===========================================================================

#[test]
fn work_order_serialization_roundtrip() {
    let wo = test_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let deser: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.task, "hello world");
    assert_eq!(deser.id, Uuid::nil());
}

#[test]
fn work_order_builder_basic() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
}

#[test]
fn work_order_builder_all_fields() {
    let wo = rich_work_order();
    assert_eq!(wo.task, "Refactor auth module");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(10));
    assert_eq!(wo.config.max_budget_usd, Some(5.0));
}

#[test]
fn work_order_to_run_envelope() {
    let wo = test_work_order();
    let run_id = "run-001".to_string();
    let env = Envelope::Run {
        id: run_id.clone(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"run\""));
    assert!(line.contains("run-001"));
    assert!(line.contains("hello world"));
}

#[test]
fn work_order_with_context_packet() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "Use async/await".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("code review").context(ctx).build();
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets.len(), 1);
}

#[test]
fn work_order_with_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.config".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["delete".into()],
    };
    let wo = WorkOrderBuilder::new("task").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools.len(), 2);
    assert_eq!(wo.policy.disallowed_tools.len(), 1);
}

#[test]
fn work_order_with_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 2);
}

#[test]
fn work_order_execution_lane_patch_first() {
    let wo = WorkOrderBuilder::new("patch task")
        .lane(ExecutionLane::PatchFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
}

#[test]
fn work_order_execution_lane_workspace_first() {
    let wo = WorkOrderBuilder::new("workspace task")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
}

#[test]
fn work_order_workspace_modes() {
    let wo_pass = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    assert!(matches!(wo_pass.workspace.mode, WorkspaceMode::PassThrough));

    let wo_staged = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    assert!(matches!(wo_staged.workspace.mode, WorkspaceMode::Staged));
}

// ===========================================================================
// 5. SDK Response → Receipt Mapping
// ===========================================================================

#[test]
fn receipt_builder_basic() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_none());
}

#[test]
fn receipt_builder_with_hash() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_deterministic() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_builder_all_fields() {
    let receipt = ReceiptBuilder::new("sidecar")
        .backend_version("1.0.0")
        .adapter_version("0.2.0")
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"tokens": 100}))
        .build();

    assert_eq!(receipt.backend.id, "sidecar");
    assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0.0"));
    assert_eq!(receipt.backend.adapter_version.as_deref(), Some("0.2.0"));
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_with_trace_events() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(receipt.trace.len(), 1);
}

#[test]
fn receipt_with_artifacts() {
    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.diff".into(),
    };
    let receipt = ReceiptBuilder::new("mock").add_artifact(artifact).build();
    assert_eq!(receipt.artifacts.len(), 1);
    assert_eq!(receipt.artifacts[0].kind, "patch");
}

#[test]
fn receipt_outcomes_serialization() {
    for outcome in &[Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(outcome).unwrap();
        let deser: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(&deser, outcome);
    }
}

#[test]
fn receipt_to_final_envelope() {
    let receipt = test_receipt();
    let env = EnvelopeBuilder::final_receipt(receipt)
        .ref_id("run-001")
        .build()
        .unwrap();
    match &env {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-001");
            assert_eq!(receipt.backend.id, "test-sidecar");
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn receipt_serialization_roundtrip() {
    let receipt = ReceiptBuilder::new("test")
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_string(&receipt).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.backend.id, "test");
    assert_eq!(deser.outcome, Outcome::Complete);
}

// ===========================================================================
// 6. Error Handling in SDK Layer
// ===========================================================================

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    let msg = format!("{err}");
    assert!(msg.contains("spawn"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("bad handshake".into());
    assert!(format!("{err}").contains("bad handshake"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("out of memory".into());
    assert!(format!("{err}").contains("out of memory"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    assert!(format!("{err}").contains("1"));
}

#[test]
fn host_error_exited_no_code() {
    let err = HostError::Exited { code: None };
    let msg = format!("{err}");
    assert!(msg.contains("None") || msg.contains("unexpectedly"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: std::time::Duration::from_secs(30),
    };
    assert!(format!("{err}").contains("30"));
}

#[test]
fn host_error_sidecar_crashed() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(137),
        stderr: "killed".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("137"));
    assert!(msg.contains("killed"));
}

#[test]
fn protocol_error_json() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_error_violation() {
    let err = ProtocolError::Violation("test".into());
    assert!(format!("{err}").contains("test"));
}

#[test]
fn protocol_error_unexpected_message() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("hello"));
    assert!(msg.contains("run"));
}

// ===========================================================================
// 7. SDK Version Compatibility
// ===========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
}

#[test]
fn parse_version_invalid() {
    assert_eq!(parse_version("invalid"), None);
    assert_eq!(parse_version("abp/v"), None);
    assert_eq!(parse_version("abp/vx.y"), None);
    assert_eq!(parse_version("v0.1"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn is_compatible_same_major() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    assert!(is_compatible_version("abp/v1.0", "abp/v1.5"));
}

#[test]
fn is_incompatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn is_compatible_invalid_returns_false() {
    assert!(!is_compatible_version("invalid", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "invalid"));
}

#[test]
fn contract_version_constant() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    let parsed = parse_version(CONTRACT_VERSION);
    assert_eq!(parsed, Some((0, 1)));
}

#[test]
fn protocol_version_parse_and_display() {
    let pv = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(pv.major, 0);
    assert_eq!(pv.minor, 1);
    let s = pv.to_string();
    assert_eq!(s, "abp/v0.1");
}

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01 < v02);
    assert!(v02 < v10);
}

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(range.contains(&v02));
}

#[test]
fn negotiate_version_compatible() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&local, &remote);
    assert!(result.is_ok());
}

#[test]
fn negotiate_version_incompatible() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    let result = negotiate_version(&local, &remote);
    assert!(result.is_err());
}

// ===========================================================================
// 8. Sidecar Hello Message Construction
// ===========================================================================

#[test]
fn hello_envelope_construction() {
    let env = Envelope::hello(test_identity(), test_capabilities());
    match &env {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-sidecar");
            assert!(capabilities.contains_key(&Capability::Streaming));
            assert_eq!(*mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_with_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        test_identity(),
        test_capabilities(),
        ExecutionMode::Passthrough,
    );
    match &env {
        Envelope::Hello { mode, .. } => assert_eq!(*mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_serialization() {
    let env = Envelope::hello(test_identity(), test_capabilities());
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"t\":\"hello\""));
    assert!(line.contains(CONTRACT_VERSION));
}

#[test]
fn hello_envelope_decode() {
    let env = Envelope::hello(test_identity(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn hello_builder_minimal() {
    let env = EnvelopeBuilder::hello()
        .backend("test-backend")
        .build()
        .unwrap();
    match &env {
        Envelope::Hello { backend, .. } => assert_eq!(backend.id, "test-backend"),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_with_all_fields() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let env = EnvelopeBuilder::hello()
        .backend("my-sidecar")
        .version("2.0")
        .adapter_version("1.0")
        .mode(ExecutionMode::Passthrough)
        .capabilities(caps)
        .build()
        .unwrap();

    match &env {
        Envelope::Hello {
            backend,
            mode,
            capabilities,
            ..
        } => {
            assert_eq!(backend.id, "my-sidecar");
            assert_eq!(backend.backend_version.as_deref(), Some("2.0"));
            assert_eq!(backend.adapter_version.as_deref(), Some("1.0"));
            assert_eq!(*mode, ExecutionMode::Passthrough);
            assert!(capabilities.contains_key(&Capability::ToolRead));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_builder_missing_backend_fails() {
    let err = EnvelopeBuilder::hello().build().unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("backend")
    );
}

// ===========================================================================
// 9. Run Envelope Construction
// ===========================================================================

#[test]
fn run_envelope_construction() {
    let wo = test_work_order();
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"run\""));
}

#[test]
fn run_envelope_builder() {
    let wo = test_work_order();
    let env = EnvelopeBuilder::run(wo)
        .ref_id("custom-id")
        .build()
        .unwrap();
    match &env {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "custom-id");
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_builder_default_id() {
    let wo = test_work_order();
    let wo_id = wo.id.to_string();
    let env = EnvelopeBuilder::run(wo).build().unwrap();
    match &env {
        Envelope::Run { id, .. } => assert_eq!(id, &wo_id),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_roundtrip() {
    let wo = test_work_order();
    let env = Envelope::Run {
        id: "run-002".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-002");
            assert_eq!(work_order.task, "hello world");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_envelope_with_rich_work_order() {
    let wo = rich_work_order();
    let env = Envelope::Run {
        id: "rich-run".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("Refactor auth module"));
    assert!(line.contains("gpt-4"));
}

// ===========================================================================
// 10. Event Streaming Through SDK Types
// ===========================================================================

#[test]
fn event_envelope_assistant_message() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    };
    let env = EnvelopeBuilder::event(event)
        .ref_id("run-001")
        .build()
        .unwrap();
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"event\""));
    assert!(line.contains("Hello!"));
}

#[test]
fn event_envelope_assistant_delta() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("assistant_delta"));
}

#[test]
fn event_envelope_tool_call() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-001".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("tool_call"));
    assert!(line.contains("read_file"));
}

#[test]
fn event_envelope_tool_result() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-001".into()),
            output: serde_json::json!({"content": "fn main() {}"}),
            is_error: false,
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("tool_result"));
}

#[test]
fn event_envelope_file_changed() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added new function".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("file_changed"));
}

#[test]
fn event_envelope_command_executed() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("All tests passed".into()),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("command_executed"));
    assert!(line.contains("cargo test"));
}

#[test]
fn event_envelope_warning() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "Rate limit approaching".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("warning"));
}

#[test]
fn event_envelope_error() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "Backend timeout".into(),
            error_code: None,
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"type\":\"error\""));
}

#[test]
fn event_envelope_run_started() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "Starting work".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("run_started"));
}

#[test]
fn event_envelope_run_completed() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "Done".into(),
        },
        ext: None,
    };
    let env = Envelope::Event {
        ref_id: "run-001".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("run_completed"));
}

#[test]
fn event_with_extension_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"original": "data"}),
    );
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("raw_message"));
}

#[test]
fn event_without_extension_data_skips_ext() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("raw_message"));
}

#[test]
fn event_builder_missing_ref_id_fails() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let err = EnvelopeBuilder::event(event).build().unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("ref_id")
    );
}

#[test]
fn multiple_events_encode_as_jsonl_stream() {
    let events: Vec<Envelope> = (0..5)
        .map(|i| Envelope::Event {
            ref_id: "run-001".into(),
            event: AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{i}"),
                },
                ext: None,
            },
        })
        .collect();

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &events).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 5);
}

// ===========================================================================
// 11. Final/Fatal Envelope Handling
// ===========================================================================

#[test]
fn final_envelope_construction() {
    let receipt = test_receipt();
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"final\""));
}

#[test]
fn final_envelope_roundtrip() {
    let receipt = test_receipt();
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-001");
            assert_eq!(receipt.backend.id, "test-sidecar");
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn final_builder_missing_ref_id_fails() {
    let receipt = test_receipt();
    let err = EnvelopeBuilder::final_receipt(receipt).build().unwrap_err();
    assert_eq!(
        err,
        abp_protocol::builder::BuilderError::MissingField("ref_id")
    );
}

#[test]
fn fatal_envelope_construction() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "out of memory".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("\"t\":\"fatal\""));
    assert!(line.contains("out of memory"));
}

#[test]
fn fatal_envelope_no_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "startup failure".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains("null"));
}

#[test]
fn fatal_envelope_roundtrip() {
    let env = Envelope::Fatal {
        ref_id: Some("run-002".into()),
        error: "timeout".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-002"));
            assert_eq!(error, "timeout");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_builder_basic() {
    let env = EnvelopeBuilder::fatal("something broke").build().unwrap();
    match &env {
        Envelope::Fatal { error, ref_id, .. } => {
            assert_eq!(error, "something broke");
            assert!(ref_id.is_none());
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_builder_with_ref_id() {
    let env = EnvelopeBuilder::fatal("error")
        .ref_id("run-001")
        .build()
        .unwrap();
    match &env {
        Envelope::Fatal { ref_id, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-001"));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("run-001".into()),
        "backend crashed",
        abp_error::ErrorCode::BackendCrashed,
    );
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendCrashed));
}

#[test]
fn fatal_error_code_none_for_non_fatal() {
    let env = Envelope::hello(test_identity(), CapabilityManifest::new());
    assert!(env.error_code().is_none());
}

// ===========================================================================
// 12. Configuration Passthrough
// ===========================================================================

#[test]
fn runtime_config_default() {
    let cfg = RuntimeConfig::default();
    assert!(cfg.model.is_none());
    assert!(cfg.vendor.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.max_budget_usd.is_none());
    assert!(cfg.max_turns.is_none());
}

#[test]
fn runtime_config_vendor_passthrough() {
    let mut cfg = RuntimeConfig::default();
    cfg.vendor.insert(
        "openai".into(),
        serde_json::json!({"temperature": 0.7, "top_p": 0.9}),
    );
    cfg.vendor
        .insert("anthropic".into(), serde_json::json!({"max_tokens": 4096}));

    let json = serde_json::to_string(&cfg).unwrap();
    let deser: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.vendor.len(), 2);
    assert!(deser.vendor.contains_key("openai"));
}

#[test]
fn runtime_config_env_passthrough() {
    let mut cfg = RuntimeConfig::default();
    cfg.env.insert("OPENAI_API_KEY".into(), "sk-test".into());
    cfg.env.insert("DEBUG".into(), "true".into());

    let json = serde_json::to_string(&cfg).unwrap();
    let deser: RuntimeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.env["OPENAI_API_KEY"], "sk-test");
}

#[test]
fn work_order_config_survives_envelope_roundtrip() {
    let mut wo = test_work_order();
    wo.config.model = Some("claude-3".into());
    wo.config.max_budget_usd = Some(10.0);
    wo.config.max_turns = Some(20);
    wo.config
        .vendor
        .insert("anthropic".into(), serde_json::json!({"max_tokens": 8192}));

    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();

    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.config.model.as_deref(), Some("claude-3"));
            assert_eq!(work_order.config.max_budget_usd, Some(10.0));
            assert_eq!(work_order.config.max_turns, Some(20));
            assert!(work_order.config.vendor.contains_key("anthropic"));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serialization() {
    let mapped = ExecutionMode::Mapped;
    let pass = ExecutionMode::Passthrough;
    let j1 = serde_json::to_string(&mapped).unwrap();
    let j2 = serde_json::to_string(&pass).unwrap();
    assert_eq!(j1, "\"mapped\"");
    assert_eq!(j2, "\"passthrough\"");
}

// ===========================================================================
// 13. Runtime Registration Integration
// ===========================================================================

#[test]
fn runtime_register_sidecar_backend() {
    let mut rt = Runtime::new();
    let spec = SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    rt.register_backend("sidecar:node", backend);
    assert!(rt.backend_names().contains(&"sidecar:node".to_string()));
}

#[test]
fn runtime_register_multiple_sidecars() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "sidecar:node",
        SidecarBackend::new(SidecarSpec::new("node")),
    );
    rt.register_backend(
        "sidecar:python",
        SidecarBackend::new(SidecarSpec::new("python")),
    );
    let names = rt.backend_names();
    assert!(names.contains(&"sidecar:node".to_string()));
    assert!(names.contains(&"sidecar:python".to_string()));
}

#[test]
fn runtime_with_default_backends_has_mock() {
    let rt = Runtime::with_default_backends();
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

#[test]
fn runtime_backend_lookup() {
    let mut rt = Runtime::new();
    rt.register_backend(
        "sidecar:test",
        SidecarBackend::new(SidecarSpec::new("echo")),
    );
    assert!(rt.backend("sidecar:test").is_some());
    assert!(rt.backend("nonexistent").is_none());
}

// ===========================================================================
// 14. SDK Script Resolution
// ===========================================================================

#[test]
fn sidecar_script_resolution() {
    let root = Path::new("/hosts");
    let path = abp_sidecar_sdk::sidecar_script(root, "node/host.js");
    assert_eq!(path, PathBuf::from("/hosts/node/host.js"));
}

#[test]
fn sidecar_script_resolution_nested() {
    let root = Path::new("/app/hosts");
    let path = abp_sidecar_sdk::sidecar_script(root, "claude/host.js");
    assert_eq!(path, PathBuf::from("/app/hosts/claude/host.js"));
}

// ===========================================================================
// 15. Lifecycle State Machine
// ===========================================================================

#[test]
fn lifecycle_initial_state() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_valid_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

#[test]
fn lifecycle_invalid_transition() {
    let mut mgr = LifecycleManager::new();
    let err = mgr.transition(LifecycleState::Running, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_already_in_state() {
    let mgr = LifecycleManager::new();
    let mut mgr = mgr;
    let err = mgr
        .transition(LifecycleState::Uninitialized, None)
        .unwrap_err();
    assert!(matches!(err, LifecycleError::AlreadyInState(_)));
}

#[test]
fn lifecycle_failed_from_any_state() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Failed, Some("crash".into()))
        .unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Failed);
}

#[test]
fn lifecycle_history_tracking() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(mgr.history().len(), 2);
    assert_eq!(mgr.history()[0].from, LifecycleState::Uninitialized);
    assert_eq!(mgr.history()[0].to, LifecycleState::Starting);
}

// ===========================================================================
// 16. Health Monitoring
// ===========================================================================

#[test]
fn health_monitor_empty() {
    let mon = HealthMonitor::new();
    assert_eq!(mon.total_checks(), 0);
    assert!(!mon.all_healthy());
}

#[test]
fn health_monitor_record_healthy() {
    let mut mon = HealthMonitor::new();
    mon.record_check("node", HealthStatus::Healthy, None);
    assert!(mon.all_healthy());
    assert_eq!(mon.total_checks(), 1);
}

#[test]
fn health_monitor_unhealthy() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "node",
        HealthStatus::Unhealthy {
            reason: "crash".into(),
        },
        None,
    );
    assert!(!mon.all_healthy());
    assert_eq!(mon.unhealthy_sidecars().len(), 1);
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "node",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    mon.record_check(
        "node",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    let check = mon.get_status("node").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut mon = HealthMonitor::new();
    mon.record_check("node", HealthStatus::Healthy, None);
    mon.record_check("node", HealthStatus::Healthy, None);
    mon.record_check(
        "node",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    let uptime = mon.uptime_percentage("node");
    assert!((uptime - 66.666).abs() < 1.0);
}

#[test]
fn health_monitor_report() {
    let mut mon = HealthMonitor::new();
    mon.record_check("a", HealthStatus::Healthy, None);
    mon.record_check(
        "b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = mon.generate_report();
    assert_eq!(report.checks.len(), 2);
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

// ===========================================================================
// 17. Pool Configuration
// ===========================================================================

#[test]
fn pool_config_defaults() {
    let cfg = PoolConfig::default();
    assert_eq!(cfg.min_size, 1);
    assert_eq!(cfg.max_size, 4);
}

#[test]
fn pool_config_serialization() {
    let cfg = PoolConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let deser: PoolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.min_size, cfg.min_size);
    assert_eq!(deser.max_size, cfg.max_size);
}

#[test]
fn pool_entry_states() {
    assert_eq!(PoolEntryState::Idle, PoolEntryState::Idle);
    assert_ne!(PoolEntryState::Idle, PoolEntryState::Busy);
    assert_ne!(PoolEntryState::Busy, PoolEntryState::Draining);
    assert_ne!(PoolEntryState::Draining, PoolEntryState::Failed);
}

// ===========================================================================
// 18. Process Configuration
// ===========================================================================

#[test]
fn process_config_default() {
    let cfg = ProcessConfig::default();
    assert!(cfg.working_dir.is_none());
    assert!(cfg.env_vars.is_empty());
    assert!(cfg.timeout.is_none());
    assert!(cfg.inherit_env);
}

#[test]
fn process_config_serialization() {
    let cfg = ProcessConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let deser: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert!(deser.inherit_env);
}

#[test]
fn process_status_variants() {
    let not_started = ProcessStatus::NotStarted;
    let running = ProcessStatus::Running { pid: 1234 };
    let exited = ProcessStatus::Exited { code: 0 };
    let killed = ProcessStatus::Killed;

    let json_ns = serde_json::to_string(&not_started).unwrap();
    let json_r = serde_json::to_string(&running).unwrap();
    let json_e = serde_json::to_string(&exited).unwrap();
    let json_k = serde_json::to_string(&killed).unwrap();

    assert!(json_ns.contains("not_started"));
    assert!(json_r.contains("1234"));
    assert!(json_e.contains("0"));
    assert!(json_k.contains("killed"));
}

// ===========================================================================
// 19. Retry Configuration
// ===========================================================================

#[test]
fn retry_config_defaults() {
    let cfg = RetryConfig::default();
    assert_eq!(cfg.max_retries, 3);
    assert!(cfg.jitter_factor >= 0.0 && cfg.jitter_factor <= 1.0);
}

#[test]
fn retry_config_serialization() {
    let cfg = RetryConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let deser: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.max_retries, 3);
}

// ===========================================================================
// 20. Backend Identity and SidecarBackend
// ===========================================================================

#[test]
fn backend_identity_serialization() {
    let id = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.2.0".into()),
    };
    let json = serde_json::to_string(&id).unwrap();
    let deser: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.id, "sidecar:node");
    assert_eq!(deser.backend_version.as_deref(), Some("1.0.0"));
}

#[test]
fn sidecar_backend_new() {
    let spec = SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    assert_eq!(backend.spec.command, "node");
}

#[test]
fn sidecar_backend_clone() {
    let spec = SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    let cloned = backend.clone();
    assert_eq!(cloned.spec.command, "node");
}

#[test]
fn sidecar_backend_debug() {
    let spec = SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    let debug = format!("{:?}", backend);
    assert!(debug.contains("node"));
}

#[test]
fn sidecar_backend_with_full_spec() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["hosts/node/host.js".into()];
    spec.env.insert("NODE_ENV".into(), "production".into());
    spec.cwd = Some("/app".into());

    let backend = SidecarBackend::new(spec);
    assert_eq!(backend.spec.args.len(), 1);
    assert_eq!(backend.spec.env["NODE_ENV"], "production");
    assert_eq!(backend.spec.cwd.as_deref(), Some("/app"));
}

// ===========================================================================
// 21. JSONL Codec Integration
// ===========================================================================

#[test]
fn jsonl_decode_stream() {
    let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, fatal]).unwrap();

    let reader = std::io::BufReader::new(buf.as_slice());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn jsonl_decode_stream_skips_blanks() {
    let input = "\n\n{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n\n";
    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn jsonl_encode_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
}

// ===========================================================================
// 22. Full Protocol Flow Simulation
// ===========================================================================

#[test]
fn full_protocol_flow_hello_run_events_final() {
    let hello = Envelope::hello(test_identity(), test_capabilities());
    let wo = test_work_order();
    let run = Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    };
    let event = Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Working...".into(),
            },
            ext: None,
        },
    };
    let final_env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: test_receipt(),
    };

    let envelopes = vec![hello, run, event, final_env];
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &envelopes).unwrap();

    let reader = std::io::BufReader::new(buf.as_slice());
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
fn protocol_flow_hello_then_fatal() {
    let hello = Envelope::hello(test_identity(), CapabilityManifest::new());
    let fatal = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "unrecoverable".into(),
        error_code: None,
    };

    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[hello, fatal]).unwrap();

    let reader = std::io::BufReader::new(buf.as_slice());
    let decoded: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(decoded.len(), 2);
    assert!(matches!(decoded[1], Envelope::Fatal { .. }));
}

// ===========================================================================
// 23. Canonical JSON and Hashing
// ===========================================================================

#[test]
fn canonical_json_sorted_keys() {
    let json = canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
    assert!(json.starts_with("{\"a\":1"));
}

#[test]
fn sha256_hex_length() {
    let hex = sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
}

#[test]
fn receipt_hash_excludes_sha256_field() {
    let r1 = ReceiptBuilder::new("test").build();
    let h1 = receipt_hash(&r1).unwrap();

    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("should-be-ignored".into());
    let h2 = receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

// ===========================================================================
// 24. Sidecar Config Serialization
// ===========================================================================

#[test]
fn sidecar_config_serialization_roundtrip() {
    let mut cfg = SidecarConfig::new("my-sidecar", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("KEY".into(), "val".into());
    cfg.working_dir = Some(PathBuf::from("/tmp"));

    let json = serde_json::to_string(&cfg).unwrap();
    let deser: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.name, "my-sidecar");
    assert_eq!(deser.command, "node");
    assert_eq!(deser.args, vec!["host.js"]);
}

#[test]
fn sidecar_config_empty_args_env_in_json() {
    let cfg = SidecarConfig::new("test", "python");
    let json = serde_json::to_string(&cfg).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["name"], "test");
    assert_eq!(v["command"], "python");
}

// ===========================================================================
// 25. Edge Cases and Boundary Conditions
// ===========================================================================

#[test]
fn empty_task_work_order() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
}

#[test]
fn very_long_task_work_order() {
    let long_task = "x".repeat(10_000);
    let wo = WorkOrderBuilder::new(&long_task).build();
    assert_eq!(wo.task.len(), 10_000);
}

#[test]
fn unicode_task_work_order() {
    let wo = WorkOrderBuilder::new("修复认证模块 🔧").build();
    assert!(wo.task.contains('🔧'));
}

#[test]
fn unicode_backend_identity() {
    let id = BackendIdentity {
        id: "サイドカー".into(),
        backend_version: None,
        adapter_version: None,
    };
    let json = serde_json::to_string(&id).unwrap();
    let deser: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.id, "サイドカー");
}

#[test]
fn empty_trace_receipt() {
    let receipt = ReceiptBuilder::new("test").build();
    assert!(receipt.trace.is_empty());
    assert!(receipt.artifacts.is_empty());
}

#[test]
fn receipt_with_many_trace_events() {
    let mut builder = ReceiptBuilder::new("test");
    for i in 0..100 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    assert_eq!(receipt.trace.len(), 100);
}

#[test]
fn sidecar_spec_empty_command() {
    let spec = SidecarSpec::new("");
    assert_eq!(spec.command, "");
}

#[test]
fn envelope_discriminator_is_t_not_type() {
    let env = Envelope::hello(test_identity(), CapabilityManifest::new());
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains("\"t\":"));
    // The protocol envelope uses "t", not "type"
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("t").is_some());
}

#[test]
fn agent_event_kind_discriminator_is_type() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("type").is_some());
    assert_eq!(v["type"], "assistant_message");
}

#[test]
fn usage_normalized_defaults() {
    let usage = UsageNormalized::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.estimated_cost_usd.is_none());
}

#[test]
fn verification_report_default() {
    let vr = VerificationReport::default();
    assert!(vr.git_diff.is_none());
    assert!(vr.git_status.is_none());
    assert!(!vr.harness_ok);
}
