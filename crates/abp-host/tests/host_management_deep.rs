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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for sidecar host management in abp-host.
//!
//! Covers: SidecarSpec construction, process lifecycle, JSONL framing,
//! handshake, error handling, protocol state, event streaming, run completion,
//! fatal handling, sidecar registration, multiple sidecars, and configuration.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{compute_delay, is_retryable, RetryConfig, RetryMetadata};
use abp_host::{HostError, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn test_backend_identity() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolUse, SupportLevel::Native);
    m
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
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

fn test_receipt(run_id: &str) -> Receipt {
    let now = Utc::now();
    let _ = run_id; // used for semantic clarity; receipt uses Uuid
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: test_backend_identity(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::Value::Null,
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn test_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_hello_json() -> String {
    let env = Envelope::hello(test_backend_identity(), test_capabilities());
    JsonlCodec::encode(&env).unwrap()
}

fn make_event_json(ref_id: &str, kind: AgentEventKind) -> String {
    let env = Envelope::Event {
        ref_id: ref_id.into(),
        event: test_agent_event(kind),
    };
    JsonlCodec::encode(&env).unwrap()
}

fn make_final_json(ref_id: &str) -> String {
    let env = Envelope::Final {
        ref_id: ref_id.into(),
        receipt: test_receipt(ref_id),
    };
    JsonlCodec::encode(&env).unwrap()
}

fn make_fatal_json(ref_id: Option<&str>, error: &str) -> String {
    let env = Envelope::Fatal {
        ref_id: ref_id.map(String::from),
        error: error.into(),
        error_code: None,
    };
    JsonlCodec::encode(&env).unwrap()
}

fn mock_script_path() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests")
        .join("mock_sidecar.py")
        .to_string_lossy()
        .into_owned()
}

fn python_cmd() -> Option<String> {
    for cmd in &["python3", "python"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return Some(cmd.to_string());
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. SidecarSpec construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn spec_new_sets_command_only() {
    let spec = SidecarSpec::new("my-sidecar");
    assert_eq!(spec.command, "my-sidecar");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn spec_with_args() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["host.js".into(), "--port".into(), "3000".into()];
    assert_eq!(spec.args.len(), 3);
    assert_eq!(spec.args[0], "host.js");
}

#[test]
fn spec_with_custom_env() {
    let mut spec = SidecarSpec::new("python3");
    spec.env.insert("API_KEY".into(), "secret".into());
    spec.env.insert("DEBUG".into(), "true".into());
    assert_eq!(spec.env.len(), 2);
    assert_eq!(spec.env["API_KEY"], "secret");
}

#[test]
fn spec_with_cwd() {
    let mut spec = SidecarSpec::new("bash");
    spec.cwd = Some("/workspace/project".into());
    assert_eq!(spec.cwd.as_deref(), Some("/workspace/project"));
}

#[test]
fn spec_serde_roundtrip_full() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["index.js".into()];
    spec.env.insert("PORT".into(), "8080".into());
    spec.cwd = Some("/tmp".into());

    let json = serde_json::to_string(&spec).unwrap();
    let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.command, "node");
    assert_eq!(deser.args, vec!["index.js"]);
    assert_eq!(deser.env["PORT"], "8080");
    assert_eq!(deser.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn spec_serde_roundtrip_minimal() {
    let spec = SidecarSpec::new("echo");
    let json = serde_json::to_string(&spec).unwrap();
    let deser: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.command, "echo");
    assert!(deser.args.is_empty());
}

#[test]
fn spec_clone_is_independent() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["a.js".into()];
    let mut clone = spec.clone();
    clone.args.push("b.js".into());
    assert_eq!(spec.args.len(), 1);
    assert_eq!(clone.args.len(), 2);
}

#[test]
fn spec_env_uses_btreemap_deterministic_order() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("Z_VAR".into(), "z".into());
    spec.env.insert("A_VAR".into(), "a".into());
    spec.env.insert("M_VAR".into(), "m".into());
    let keys: Vec<_> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A_VAR", "M_VAR", "Z_VAR"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Process lifecycle (ProcessInfo, ProcessConfig, ProcessStatus)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn process_info_starts_not_started() {
    let spec = SidecarSpec::new("node");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
}

#[test]
fn process_info_not_running_initially() {
    let info = ProcessInfo::new(SidecarSpec::new("x"), ProcessConfig::default());
    assert!(!info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_running_with_pid() {
    let mut info = ProcessInfo::new(SidecarSpec::new("x"), ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 12345 };
    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_exited_is_terminated() {
    let mut info = ProcessInfo::new(SidecarSpec::new("x"), ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_status_killed_is_terminated() {
    let mut info = ProcessInfo::new(SidecarSpec::new("x"), ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_status_timed_out_is_terminated() {
    let mut info = ProcessInfo::new(SidecarSpec::new("x"), ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

#[test]
fn process_config_default_inherits_env() {
    let config = ProcessConfig::default();
    assert!(config.inherit_env);
    assert!(config.env_vars.is_empty());
    assert!(config.timeout.is_none());
    assert!(config.working_dir.is_none());
}

#[test]
fn process_config_custom_timeout() {
    let mut config = ProcessConfig::default();
    config.timeout = Some(Duration::from_secs(30));
    assert_eq!(config.timeout, Some(Duration::from_secs(30)));
}

#[test]
fn process_config_serde_roundtrip() {
    let mut config = ProcessConfig::default();
    config.working_dir = Some(PathBuf::from("/workspace"));
    config.env_vars.insert("KEY".into(), "val".into());
    config.timeout = Some(Duration::from_millis(5000));
    config.inherit_env = false;

    let json = serde_json::to_string(&config).unwrap();
    let deser: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.working_dir.as_deref(), Some(Path::new("/workspace")));
    assert_eq!(deser.env_vars["KEY"], "val");
    assert!(!deser.inherit_env);
}

#[test]
fn process_status_serde_roundtrip() {
    let statuses = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 42 },
        ProcessStatus::Exited { code: 1 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];
    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let deser: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deser, status);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. JSONL framing: encode/decode envelopes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn jsonl_encode_hello_has_tag_t() {
    let env = Envelope::hello(test_backend_identity(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"hello""#));
    assert!(line.ends_with('\n'));
}

#[test]
fn jsonl_encode_run_has_tag_t() {
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: test_work_order(),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"run""#));
}

#[test]
fn jsonl_encode_event_has_tag_t() {
    let event = test_agent_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"event""#));
}

#[test]
fn jsonl_encode_final_has_tag_t() {
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt: test_receipt("r1"),
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"final""#));
}

#[test]
fn jsonl_encode_fatal_has_tag_t() {
    let env = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"fatal""#));
}

#[test]
fn jsonl_decode_hello_roundtrip() {
    let original = Envelope::hello(test_backend_identity(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn jsonl_decode_fatal_roundtrip() {
    let original = Envelope::Fatal {
        ref_id: Some("r1".into()),
        error: "something failed".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&original).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, ref_id, .. } => {
            assert_eq!(error, "something failed");
            assert_eq!(ref_id, Some("r1".into()));
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn jsonl_decode_invalid_json_is_error() {
    let result = JsonlCodec::decode("this is not json");
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_empty_object_is_error() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn jsonl_decode_stream_multiple_envelopes() {
    let hello = make_hello_json();
    let fatal = make_fatal_json(None, "err");
    let input = format!("{hello}{fatal}");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn jsonl_decode_stream_skips_empty_lines() {
    let hello = make_hello_json();
    let input = format!("\n\n{hello}\n\n");
    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Handshake: hello envelope exchange
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hello_envelope_contains_contract_version() {
    let env = Envelope::hello(test_backend_identity(), CapabilityManifest::new());
    match &env {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_carries_backend_identity() {
    let backend = BackendIdentity {
        id: "custom-backend".into(),
        backend_version: Some("2.0".into()),
        adapter_version: None,
    };
    let env = Envelope::hello(backend.clone(), CapabilityManifest::new());
    match env {
        Envelope::Hello { backend: b, .. } => {
            assert_eq!(b.id, "custom-backend");
            assert_eq!(b.backend_version, Some("2.0".into()));
            assert!(b.adapter_version.is_none());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_carries_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let env = Envelope::hello(test_backend_identity(), caps);
    match env {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 2);
            assert!(capabilities.contains_key(&Capability::ToolUse));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_mode_sets_execution_mode() {
    let env = Envelope::hello_with_mode(
        test_backend_identity(),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    match env {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn sidecar_hello_struct_fields() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_backend_identity(),
        capabilities: test_capabilities(),
    };
    assert_eq!(hello.contract_version, CONTRACT_VERSION);
    assert_eq!(hello.backend.id, "test-backend");
    assert!(!hello.capabilities.is_empty());
}

#[test]
fn sidecar_hello_serde_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.to_string(),
        backend: test_backend_identity(),
        capabilities: CapabilityManifest::new(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let deser: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.contract_version, hello.contract_version);
    assert_eq!(deser.backend.id, hello.backend.id);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Error handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn host_error_spawn_display_contains_message() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "binary not found",
    ));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
    assert!(msg.contains("binary not found"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("unexpected Run envelope".into());
    assert!(err.to_string().contains("violation"));
    assert!(err.to_string().contains("unexpected Run envelope"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("out of memory".into());
    assert!(err.to_string().contains("fatal"));
    assert!(err.to_string().contains("out of memory"));
}

#[test]
fn host_error_exited_with_code() {
    let err = HostError::Exited { code: Some(137) };
    let msg = err.to_string();
    assert!(msg.contains("exited"));
    assert!(msg.contains("137"));
}

#[test]
fn host_error_exited_without_code() {
    let err = HostError::Exited { code: None };
    assert!(err.to_string().contains("None"));
}

#[test]
fn host_error_sidecar_crashed() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"));
    assert!(msg.contains("segfault"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"));
    assert!(msg.contains("30"));
}

#[test]
fn host_error_protocol_from_protocol_error() {
    let pe = ProtocolError::Violation("bad message".into());
    let he = HostError::Protocol(pe);
    assert!(he.to_string().contains("protocol"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(err.to_string().contains("stdin"));
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(err.to_string().contains("stdout"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Protocol state: lifecycle state machine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_starts_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_valid_sequence_uninit_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Running);
}

#[test]
fn lifecycle_running_to_ready_is_valid() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_invalid_transition_returns_error() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Running, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        LifecycleError::InvalidTransition { from, to } => {
            assert_eq!(from, LifecycleState::Uninitialized);
            assert_eq!(to, LifecycleState::Running);
        }
        other => panic!("expected InvalidTransition, got: {other}"),
    }
}

#[test]
fn lifecycle_same_state_is_error() {
    let mgr = LifecycleManager::new();
    let mut mgr = mgr;
    let result = mgr.transition(LifecycleState::Uninitialized, None);
    match result.unwrap_err() {
        LifecycleError::AlreadyInState(s) => {
            assert_eq!(s, LifecycleState::Uninitialized);
        }
        other => panic!("expected AlreadyInState, got: {other}"),
    }
}

#[test]
fn lifecycle_failed_from_any_state() {
    for start_state in &[
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ] {
        let mut mgr = LifecycleManager::new();
        // Walk to the desired start state.
        match start_state {
            LifecycleState::Uninitialized => {}
            LifecycleState::Starting => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
            }
            LifecycleState::Ready => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
            }
            LifecycleState::Running => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Running, None).unwrap();
            }
            LifecycleState::Stopping => {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Running, None).unwrap();
                mgr.transition(LifecycleState::Stopping, None).unwrap();
            }
            _ => {}
        }
        assert!(
            mgr.transition(LifecycleState::Failed, Some("crash".into()))
                .is_ok(),
            "should allow Failed from {start_state:?}"
        );
    }
}

#[test]
fn lifecycle_history_tracks_transitions() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let history = mgr.history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].from, LifecycleState::Uninitialized);
    assert_eq!(history[0].to, LifecycleState::Starting);
    assert_eq!(history[0].reason.as_deref(), Some("boot"));
    assert_eq!(history[1].from, LifecycleState::Starting);
    assert_eq!(history[1].to, LifecycleState::Ready);
}

#[test]
fn lifecycle_uptime_none_before_ready() {
    let mgr = LifecycleManager::new();
    assert!(mgr.uptime().is_none());
}

#[test]
fn lifecycle_uptime_some_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert!(mgr.uptime().is_some());
}

#[test]
fn lifecycle_display_all_states() {
    let states = vec![
        (LifecycleState::Uninitialized, "uninitialized"),
        (LifecycleState::Starting, "starting"),
        (LifecycleState::Ready, "ready"),
        (LifecycleState::Running, "running"),
        (LifecycleState::Stopping, "stopping"),
        (LifecycleState::Stopped, "stopped"),
        (LifecycleState::Failed, "failed"),
    ];
    for (state, expected) in states {
        assert_eq!(state.to_string(), expected);
    }
}

#[test]
fn lifecycle_stopping_to_stopped() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopped);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Event streaming: encode/decode multiple events
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn event_stream_multiple_events_decode() {
    let ref_id = "run-42";
    let mut input = String::new();
    input.push_str(&make_event_json(
        ref_id,
        AgentEventKind::RunStarted {
            message: "start".into(),
        },
    ));
    input.push_str(&make_event_json(
        ref_id,
        AgentEventKind::AssistantDelta {
            text: "hello ".into(),
        },
    ));
    input.push_str(&make_event_json(
        ref_id,
        AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        },
    ));
    input.push_str(&make_event_json(
        ref_id,
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    ));

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 4);

    for env in &envelopes {
        match env {
            Envelope::Event { ref_id: rid, .. } => assert_eq!(rid, ref_id),
            _ => panic!("expected Event"),
        }
    }
}

#[test]
fn event_stream_tool_call_and_result() {
    let ref_id = "run-tc";
    let tool_call = test_agent_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "src/main.rs"}),
    });
    let tool_result = test_agent_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-1".into()),
        output: serde_json::json!({"content": "fn main() {}"}),
        is_error: false,
    });

    let env1 = Envelope::Event {
        ref_id: ref_id.into(),
        event: tool_call,
    };
    let env2 = Envelope::Event {
        ref_id: ref_id.into(),
        event: tool_result,
    };

    let line1 = JsonlCodec::encode(&env1).unwrap();
    let line2 = JsonlCodec::encode(&env2).unwrap();
    let input = format!("{line1}{line2}");

    let reader = BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
}

#[test]
fn event_with_ext_field_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"vendor_specific": true}),
    );
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };

    let env = Envelope::Event {
        ref_id: "r1".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => {
            assert!(event.ext.is_some());
            assert!(event.ext.unwrap().contains_key("raw_message"));
        }
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Run completion: final envelope → Receipt
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn final_envelope_carries_receipt() {
    let ref_id = "run-final";
    let json = make_final_json(ref_id);
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final {
            ref_id: rid,
            receipt,
        } => {
            assert_eq!(rid, ref_id);
            assert_eq!(receipt.meta.run_id, Uuid::nil());
            assert!(matches!(receipt.outcome, Outcome::Complete));
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn receipt_has_backend_identity() {
    let receipt = test_receipt("r1");
    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.backend.backend_version, Some("1.0.0".into()));
}

#[test]
fn receipt_has_contract_version() {
    let receipt = test_receipt("r1");
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_sha256_initially_none() {
    let receipt = test_receipt("r1");
    assert!(receipt.receipt_sha256.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Fatal handling: fatal envelope → error propagation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_envelope_decode() {
    let json = make_fatal_json(Some("r1"), "catastrophic failure");
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("r1".into()));
            assert_eq!(error, "catastrophic failure");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_with_no_ref_id() {
    let json = make_fatal_json(None, "startup error");
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "startup error");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r1".into()),
        "auth failed",
        abp_error::ErrorCode::BackendAuthFailed,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::BackendAuthFailed)
    );
}

#[test]
fn host_error_fatal_carries_message() {
    let err = HostError::Fatal("sidecar panicked".into());
    assert!(err.to_string().contains("sidecar panicked"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Sidecar registration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("test-sc", "node")).unwrap();
    let cfg = reg.get("test-sc").unwrap();
    assert_eq!(cfg.command, "node");
}

#[test]
fn registry_duplicate_rejected() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "cmd1")).unwrap();
    assert!(reg.register(SidecarConfig::new("sc", "cmd2")).is_err());
}

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("z-sc", "z")).unwrap();
    reg.register(SidecarConfig::new("a-sc", "a")).unwrap();
    reg.register(SidecarConfig::new("m-sc", "m")).unwrap();
    assert_eq!(reg.list(), vec!["a-sc", "m-sc", "z-sc"]);
}

#[test]
fn registry_remove_and_re_register() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "old")).unwrap();
    assert!(reg.remove("sc"));
    reg.register(SidecarConfig::new("sc", "new")).unwrap();
    assert_eq!(reg.get("sc").unwrap().command, "new");
}

#[test]
fn registry_empty_has_no_entries() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
    assert!(reg.get("any").is_none());
}

#[test]
fn registry_validation_rejects_empty_name() {
    let mut reg = SidecarRegistry::default();
    assert!(reg.register(SidecarConfig::new("", "cmd")).is_err());
}

#[test]
fn registry_validation_rejects_empty_command() {
    let mut reg = SidecarRegistry::default();
    assert!(reg.register(SidecarConfig::new("name", "")).is_err());
}

#[test]
fn registry_discover_from_temp_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let node_dir = tmp.path().join("my-node");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// mock").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert_eq!(reg.list(), vec!["my-node"]);
    assert_eq!(reg.get("my-node").unwrap().command, "node");
}

#[test]
fn registry_discover_ignores_non_host_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("no-host");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("README.md"), "# nothing").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_discover_nonexistent_dir_error() {
    assert!(SidecarRegistry::from_config_dir(Path::new("/no/such/dir")).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Multiple sidecars: different sidecar types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn registry_multiple_sidecar_types() {
    let mut reg = SidecarRegistry::default();
    let mut node_cfg = SidecarConfig::new("node", "node");
    node_cfg.args = vec!["host.js".into()];

    let mut py_cfg = SidecarConfig::new("python", "python3");
    py_cfg.args = vec!["host.py".into()];

    let mut bash_cfg = SidecarConfig::new("bash", "bash");
    bash_cfg.args = vec!["host.sh".into()];

    reg.register(node_cfg).unwrap();
    reg.register(py_cfg).unwrap();
    reg.register(bash_cfg).unwrap();

    assert_eq!(reg.list().len(), 3);
    assert_eq!(reg.get("node").unwrap().command, "node");
    assert_eq!(reg.get("python").unwrap().command, "python3");
    assert_eq!(reg.get("bash").unwrap().command, "bash");
}

#[test]
fn discover_multiple_host_types() {
    let tmp = tempfile::tempdir().unwrap();

    let node_dir = tmp.path().join("node-sidecar");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "//").unwrap();

    let py_dir = tmp.path().join("python-sidecar");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "#").unwrap();

    let bash_dir = tmp.path().join("bash-sidecar");
    std::fs::create_dir(&bash_dir).unwrap();
    std::fs::write(bash_dir.join("host.sh"), "#!").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert_eq!(reg.list().len(), 3);
    assert_eq!(reg.get("node-sidecar").unwrap().command, "node");
    assert_eq!(reg.get("python-sidecar").unwrap().command, "python");
    assert_eq!(reg.get("bash-sidecar").unwrap().command, "bash");
}

#[test]
fn config_to_spec_preserves_all_fields() {
    let mut cfg = SidecarConfig::new("my-node", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("PORT".into(), "3000".into());
    cfg.working_dir = Some(PathBuf::from("/workspace"));

    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env["PORT"], "3000");
    assert_eq!(spec.cwd.as_deref(), Some("/workspace"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Configuration: SidecarSpec fields, environment variables
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_config_with_env_and_working_dir() {
    let mut cfg = SidecarConfig::new("test", "cmd");
    cfg.env.insert("VAR1".into(), "val1".into());
    cfg.env.insert("VAR2".into(), "val2".into());
    cfg.working_dir = Some(PathBuf::from("/opt/sidecar"));
    cfg.args = vec!["--flag".into()];

    assert!(cfg.validate().is_ok());
    assert_eq!(cfg.env.len(), 2);
}

#[test]
fn sidecar_config_serde_with_all_fields() {
    let mut env = BTreeMap::new();
    env.insert("K".into(), "V".into());
    let cfg = SidecarConfig {
        name: "full".into(),
        command: "full-cmd".into(),
        args: vec!["--arg1".into()],
        env,
        working_dir: Some(PathBuf::from("/full/path")),
    };

    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("full-cmd"));
    assert!(json.contains("--arg1"));

    let deser: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.name, "full");
    assert_eq!(deser.working_dir.as_deref(), Some(Path::new("/full/path")));
}

#[test]
fn sidecar_config_deserialize_minimal_json() {
    let json = r#"{"name":"n","command":"c"}"#;
    let cfg: SidecarConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.name, "n");
    assert_eq!(cfg.command, "c");
    assert!(cfg.args.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.working_dir.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Health monitoring
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn health_monitor_record_and_query() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc-1",
        HealthStatus::Healthy,
        Some(Duration::from_millis(5)),
    );
    let check = monitor.get_status("sc-1").unwrap();
    assert_eq!(check.status, HealthStatus::Healthy);
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc-1",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    monitor.record_check(
        "sc-1",
        HealthStatus::Unhealthy {
            reason: "still down".into(),
        },
        None,
    );
    let check = monitor.get_status("sc-1").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_failure_resets_on_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sc-1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    let check = monitor.get_status("sc-1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_all_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check("sc-2", HealthStatus::Healthy, None);
    assert!(monitor.all_healthy());
}

#[test]
fn health_monitor_not_all_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc-2",
        HealthStatus::Unhealthy {
            reason: "bad".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());
}

#[test]
fn health_monitor_empty_not_all_healthy() {
    let monitor = HealthMonitor::new();
    assert!(!monitor.all_healthy());
}

#[test]
fn health_monitor_unhealthy_sidecars() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc-2",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    let unhealthy = monitor.unhealthy_sidecars();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].name, "sc-2");
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check("sc-1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    let pct = monitor.uptime_percentage("sc-1");
    assert!((pct - 75.0).abs() < 0.01);
}

#[test]
fn health_monitor_uptime_unknown_name() {
    let monitor = HealthMonitor::new();
    assert!((monitor.uptime_percentage("ghost")).abs() < 0.01);
}

#[test]
fn health_report_overall_unknown_when_empty() {
    let monitor = HealthMonitor::new();
    let report = monitor.generate_report();
    assert_eq!(report.overall, HealthStatus::Unknown);
}

#[test]
fn health_report_overall_unhealthy_when_any_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc-2",
        HealthStatus::Unhealthy {
            reason: "bad".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn health_report_overall_degraded() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("sc-1", HealthStatus::Healthy, None);
    monitor.record_check(
        "sc-2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// Pool management
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pool_add_and_acquire() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.id, "s1");
    assert_eq!(entry.state, PoolEntryState::Busy);
}

#[test]
fn pool_max_size_enforced() {
    let mut config = PoolConfig::default();
    config.max_size = 2;
    let pool = SidecarPool::new(config);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3"));
}

#[test]
fn pool_release_makes_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    pool.release(&entry.id);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_mark_failed() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.mark_failed("s1");
    let stats = pool.stats();
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.idle, 0);
}

#[test]
fn pool_drain_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.drain("s1");
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_remove_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(pool.total_count(), 0);
}

#[test]
fn pool_stats_comprehensive() {
    let mut config = PoolConfig::default();
    config.max_size = 10;
    let pool = SidecarPool::new(config);
    pool.add("idle1");
    pool.add("idle2");
    pool.add("busy1");
    pool.add("failed1");
    pool.add("drain1");

    pool.acquire(); // makes one busy
    pool.mark_failed("failed1");
    pool.drain("drain1");

    let stats = pool.stats();
    assert_eq!(stats.total, 5);
    assert!(stats.busy >= 1);
    assert!(stats.failed >= 1);
    assert!(stats.draining >= 1);
}

#[test]
fn pool_utilization_empty() {
    let stats = abp_host::pool::PoolStats {
        total: 0,
        idle: 0,
        busy: 0,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn pool_utilization_half() {
    let stats = abp_host::pool::PoolStats {
        total: 4,
        idle: 2,
        busy: 2,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}

// ═══════════════════════════════════════════════════════════════════════════
// Retry configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn retry_config_defaults() {
    let cfg = RetryConfig::default();
    assert_eq!(cfg.max_retries, 3);
    assert!(cfg.base_delay > Duration::ZERO);
    assert!(cfg.max_delay > cfg.base_delay);
    assert!(cfg.overall_timeout > Duration::ZERO);
    assert!(cfg.jitter_factor >= 0.0 && cfg.jitter_factor <= 1.0);
}

#[test]
fn compute_delay_exponential() {
    let cfg = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.0, // no jitter for deterministic test
    };

    let d0 = compute_delay(&cfg, 0);
    let d1 = compute_delay(&cfg, 1);
    let d2 = compute_delay(&cfg, 2);

    assert_eq!(d0, Duration::from_millis(100));
    assert_eq!(d1, Duration::from_millis(200));
    assert_eq!(d2, Duration::from_millis(400));
}

#[test]
fn compute_delay_capped_at_max() {
    let cfg = RetryConfig {
        max_retries: 10,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(500),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.0,
    };

    let d5 = compute_delay(&cfg, 5);
    assert!(d5 <= Duration::from_millis(500));
}

#[test]
fn is_retryable_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_timeout_error() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(5),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_exited_error() {
    let err = HostError::Exited { code: Some(1) };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_crashed_error() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "crash".into(),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_not_retryable_violation() {
    let err = HostError::Violation("bad".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_fatal() {
    let err = HostError::Fatal("boom".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_protocol() {
    let pe = ProtocolError::Violation("bad".into());
    let err = HostError::Protocol(pe);
    assert!(!is_retryable(&err));
}

#[test]
fn retry_metadata_defaults() {
    let meta = RetryMetadata::default();
    assert_eq!(meta.total_attempts, 0);
    assert!(meta.failed_attempts.is_empty());
    assert_eq!(meta.total_duration, Duration::ZERO);
}

#[test]
fn retry_metadata_to_receipt_metadata() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(150),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(150u64));
}

#[test]
fn retry_config_serde_roundtrip() {
    let cfg = RetryConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let deser: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.max_retries, cfg.max_retries);
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration tests (require Python)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn integration_spawn_invalid_binary() {
    let spec = SidecarSpec::new("nonexistent_binary_abcxyz_999");
    let result = abp_host::SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Spawn(_)));
}

#[tokio::test]
async fn integration_spawn_and_hello() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path()];

    let client = abp_host::SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(client.hello.contract_version, "abp/v0.1");
}

#[tokio::test]
async fn integration_run_receives_events_and_final() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path()];

    let client = abp_host::SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();

    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    use tokio_stream::StreamExt;
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());

    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn integration_fatal_mode() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "fatal".into()];

    let client = abp_host::SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();

    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    use tokio_stream::StreamExt;
    let _events: Vec<_> = sidecar_run.events.collect().await;

    let result = sidecar_run.receipt.await.unwrap();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Fatal(_)));
}

#[tokio::test]
async fn integration_multi_events_mode() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "multi_events".into()];

    let client = abp_host::SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();

    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    use tokio_stream::StreamExt;
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5);

    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn integration_no_hello_mode_returns_error() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "no_hello".into()];

    let result = abp_host::SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn integration_bad_json_midstream() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "bad_json_midstream".into()];

    let client = abp_host::SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();

    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    use tokio_stream::StreamExt;
    let _events: Vec<_> = sidecar_run.events.collect().await;

    let result = sidecar_run.receipt.await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn integration_env_var_passed_to_sidecar() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };

    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "echo_env".into()];
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());

    let client = abp_host::SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();

    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();

    use tokio_stream::StreamExt;
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());

    let has_env_var = events.iter().any(|e| {
        if let AgentEventKind::RunStarted { message } = &e.kind {
            message.contains("hello_from_test")
        } else {
            false
        }
    });
    assert!(has_env_var, "expected env var in event: {events:?}");

    sidecar_run.receipt.await.unwrap().unwrap();
    sidecar_run.wait.await.unwrap().unwrap();
}
