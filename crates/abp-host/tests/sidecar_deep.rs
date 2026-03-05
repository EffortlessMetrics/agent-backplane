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
//! Comprehensive deep tests for sidecar host management in `abp-host`.
//!
//! Covers: SidecarConfig construction/validation, serde roundtrips,
//! SidecarSpec builder patterns, protocol handshake simulation,
//! event parsing, error handling, registry management, lifecycle state
//! machine, pool management, process tracking, retry configuration,
//! health monitoring, and edge cases (empty output, malformed JSONL,
//! unexpected envelopes).

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_host::health::{HealthMonitor, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{RetryConfig, RetryMetadata, compute_delay, is_retryable};
use abp_host::{HostError, SidecarClient, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "deep-test".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: "sidecar deep test".into(),
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

fn test_receipt(run_id: Uuid, wo_id: Uuid) -> Receipt {
    let now = chrono::Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: wo_id,
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: test_backend(),
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

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: chrono::Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn mock_script_path() -> String {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
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

macro_rules! require_python {
    () => {
        match python_cmd() {
            Some(cmd) => cmd,
            None => {
                eprintln!("SKIP: python not found");
                return;
            }
        }
    };
}

fn mock_spec(py: &str) -> SidecarSpec {
    mock_spec_with_mode(py, "default")
}

fn mock_spec_with_mode(py: &str, mode: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![mock_script_path(), mode.to_string()];
    spec
}

// ═══════════════════════════════════════════════════════════════════════
// 1. SidecarConfig construction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_new_sets_name_and_command() {
    let cfg = SidecarConfig::new("mysc", "node");
    assert_eq!(cfg.name, "mysc");
    assert_eq!(cfg.command, "node");
}

#[test]
fn config_new_defaults_are_empty() {
    let cfg = SidecarConfig::new("sc", "cmd");
    assert!(cfg.args.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.working_dir.is_none());
}

#[test]
fn config_new_accepts_string_refs() {
    let name = String::from("myname");
    let cmd = String::from("mycmd");
    let cfg = SidecarConfig::new(&name, &cmd);
    assert_eq!(cfg.name, "myname");
    assert_eq!(cfg.command, "mycmd");
}

#[test]
fn config_new_accepts_owned_strings() {
    let cfg = SidecarConfig::new(String::from("a"), String::from("b"));
    assert_eq!(cfg.name, "a");
    assert_eq!(cfg.command, "b");
}

#[test]
fn config_fields_are_mutable() {
    let mut cfg = SidecarConfig::new("sc", "cmd");
    cfg.args = vec!["--verbose".into()];
    cfg.env.insert("KEY".into(), "VAL".into());
    cfg.working_dir = Some(PathBuf::from("/tmp"));
    assert_eq!(cfg.args.len(), 1);
    assert_eq!(cfg.env["KEY"], "VAL");
    assert_eq!(cfg.working_dir.as_deref(), Some(Path::new("/tmp")));
}

#[test]
fn config_clone_is_independent() {
    let mut original = SidecarConfig::new("sc", "cmd");
    original.args = vec!["a".into()];
    let mut cloned = original.clone();
    cloned.args.push("b".into());
    assert_eq!(original.args.len(), 1);
    assert_eq!(cloned.args.len(), 2);
}

#[test]
fn config_debug_contains_fields() {
    let cfg = SidecarConfig::new("test-sc", "test-cmd");
    let dbg = format!("{:?}", cfg);
    assert!(dbg.contains("test-sc"), "got: {dbg}");
    assert!(dbg.contains("test-cmd"), "got: {dbg}");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. SidecarConfig validation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_validate_succeeds_for_valid() {
    let cfg = SidecarConfig::new("sc", "cmd");
    assert!(cfg.validate().is_ok());
}

#[test]
fn config_validate_empty_name_fails() {
    let cfg = SidecarConfig::new("", "cmd");
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("name"), "got: {err}");
}

#[test]
fn config_validate_empty_command_fails() {
    let cfg = SidecarConfig::new("sc", "");
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("command"), "got: {err}");
}

#[test]
fn config_validate_both_empty_reports_name_first() {
    let cfg = SidecarConfig::new("", "");
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("name"), "got: {err}");
}

#[test]
fn config_validate_whitespace_name_is_ok() {
    // Whitespace-only names pass validation (not empty).
    let cfg = SidecarConfig::new(" ", "cmd");
    assert!(cfg.validate().is_ok());
}

#[test]
fn config_validate_with_args_and_env() {
    let mut cfg = SidecarConfig::new("sc", "cmd");
    cfg.args = vec!["--flag".into()];
    cfg.env.insert("K".into(), "V".into());
    assert!(cfg.validate().is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. SidecarConfig serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_serde_roundtrip_minimal() {
    let cfg = SidecarConfig::new("node-sc", "node");
    let json = serde_json::to_string(&cfg).unwrap();
    let de: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(de.name, "node-sc");
    assert_eq!(de.command, "node");
    assert!(de.args.is_empty());
    assert!(de.env.is_empty());
    assert!(de.working_dir.is_none());
}

#[test]
fn config_serde_roundtrip_full() {
    let mut env = BTreeMap::new();
    env.insert("A".into(), "1".into());
    env.insert("B".into(), "2".into());
    let cfg = SidecarConfig {
        name: "full".into(),
        command: "python3".into(),
        args: vec!["host.py".into(), "--debug".into()],
        env,
        working_dir: Some(PathBuf::from("/workspace/dir")),
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let de: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(de.name, "full");
    assert_eq!(de.command, "python3");
    assert_eq!(de.args, vec!["host.py", "--debug"]);
    assert_eq!(de.env.len(), 2);
    assert_eq!(de.env["A"], "1");
    assert_eq!(de.working_dir.as_deref(), Some(Path::new("/workspace/dir")));
}

#[test]
fn config_deserialize_missing_optionals_uses_defaults() {
    let json = r#"{"name":"n","command":"c"}"#;
    let cfg: SidecarConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.name, "n");
    assert_eq!(cfg.command, "c");
    assert!(cfg.args.is_empty());
    assert!(cfg.env.is_empty());
    assert!(cfg.working_dir.is_none());
}

#[test]
fn config_serde_preserves_empty_args() {
    let cfg = SidecarConfig::new("x", "y");
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains(r#""args":[]"#) || json.contains(r#""args": []"#));
}

#[test]
fn config_serde_with_special_characters() {
    let mut cfg = SidecarConfig::new("sc/with:special", "cmd with spaces");
    cfg.args = vec!["arg=value".into(), "path/to/file".into()];
    cfg.env.insert("KEY_WITH_EQUALS=".into(), "val=ue".into());
    let json = serde_json::to_string(&cfg).unwrap();
    let de: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(de.name, "sc/with:special");
    assert_eq!(de.command, "cmd with spaces");
    assert_eq!(de.env["KEY_WITH_EQUALS="], "val=ue");
}

// ═══════════════════════════════════════════════════════════════════════
// 4. SidecarConfig to_spec conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_to_spec_basic() {
    let cfg = SidecarConfig::new("sc", "node");
    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn config_to_spec_with_all_fields() {
    let mut cfg = SidecarConfig::new("sc", "python3");
    cfg.args = vec!["host.py".into()];
    cfg.env.insert("PORT".into(), "3000".into());
    cfg.working_dir = Some(PathBuf::from("/work"));
    let spec = cfg.to_spec();
    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args, vec!["host.py"]);
    assert_eq!(spec.env["PORT"], "3000");
    assert_eq!(spec.cwd.as_deref(), Some("/work"));
}

#[test]
fn config_to_spec_drops_name() {
    let cfg = SidecarConfig::new("my-name", "cmd");
    let spec = cfg.to_spec();
    assert_eq!(spec.command, "cmd");
    // SidecarSpec has no name field — name is a registry concern.
}

// ═══════════════════════════════════════════════════════════════════════
// 5. SidecarSpec construction and serde
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn spec_new_sets_command() {
    let spec = SidecarSpec::new("my-binary");
    assert_eq!(spec.command, "my-binary");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn spec_serde_roundtrip_full() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["index.js".into()];
    spec.env.insert("NODE_ENV".into(), "test".into());
    spec.cwd = Some("/project".into());
    let json = serde_json::to_string(&spec).unwrap();
    let de: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(de.command, "node");
    assert_eq!(de.args, vec!["index.js"]);
    assert_eq!(de.env["NODE_ENV"], "test");
    assert_eq!(de.cwd.as_deref(), Some("/project"));
}

#[test]
fn spec_serde_minimal_json() {
    let json = r#"{"command":"echo","args":[],"env":{},"cwd":null}"#;
    let spec: SidecarSpec = serde_json::from_str(json).unwrap();
    assert_eq!(spec.command, "echo");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. SidecarHello serde
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_hello_serde_roundtrip() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
    };
    let json = serde_json::to_string(&hello).unwrap();
    let de: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(de.contract_version, CONTRACT_VERSION);
    assert_eq!(de.backend.id, "deep-test");
}

#[test]
fn sidecar_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: caps,
    };
    let json = serde_json::to_string(&hello).unwrap();
    let de: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(de.capabilities.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Protocol handshake simulation (no process spawning)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_envelope_encodes_with_t_tag() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&hello).unwrap();
    assert!(line.contains(r#""t":"hello""#));
    assert!(line.ends_with('\n'));
}

#[test]
fn hello_envelope_roundtrip() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            mode,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "deep-test");
            assert_eq!(mode, ExecutionMode::Mapped);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        test_backend(),
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Passthrough),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn run_envelope_roundtrip() {
    let wo = test_work_order();
    let run = Envelope::Run {
        id: "run-deep-1".into(),
        work_order: wo,
    };
    let line = JsonlCodec::encode(&run).unwrap();
    assert!(line.contains(r#""t":"run""#));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-deep-1");
            assert_eq!(work_order.task, "sidecar deep test");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn event_envelope_roundtrip() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"event""#));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-1");
            assert!(matches!(
                event.kind,
                AgentEventKind::AssistantMessage { .. }
            ));
        }
        _ => panic!("expected Event"),
    }
}

#[test]
fn final_envelope_roundtrip() {
    let receipt = test_receipt(Uuid::new_v4(), Uuid::nil());
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"final""#));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-1");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        _ => panic!("expected Final"),
    }
}

#[test]
fn fatal_envelope_with_ref_id() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "crash".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.contains(r#""t":"fatal""#));
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id, Some("run-1".into()));
            assert_eq!(error, "crash");
        }
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn fatal_envelope_without_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "global fail".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "global fail");
        }
        _ => panic!("expected Fatal"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Event parsing from sidecar stdout (decode_stream)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_stream_parses_multiple_lines() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let event = make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    );
    let mut buf = String::new();
    buf.push_str(&JsonlCodec::encode(&hello).unwrap());
    buf.push_str(&JsonlCodec::encode(&event).unwrap());

    let reader = BufReader::new(buf.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Event { .. }));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let mut buf = String::new();
    buf.push('\n');
    buf.push_str(&JsonlCodec::encode(&hello).unwrap());
    buf.push_str("\n\n");

    let reader = BufReader::new(buf.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn decode_stream_returns_error_on_malformed_line() {
    let input = "not valid json\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn decode_stream_empty_input() {
    let reader = BufReader::new("".as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect::<Vec<_>>();
    assert!(results.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Error handling – HostError variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    let msg = err.to_string();
    assert!(msg.contains("spawn"), "got: {msg}");
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken",
    ));
    assert!(err.to_string().contains("stdout"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken",
    ));
    assert!(err.to_string().contains("stdin"));
}

#[test]
fn host_error_protocol_display() {
    let pe = ProtocolError::Violation("bad envelope".into());
    let err = HostError::Protocol(pe);
    let msg = err.to_string();
    assert!(msg.contains("protocol"), "got: {msg}");
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("bad handshake".into());
    let msg = err.to_string();
    assert!(msg.contains("violation"), "got: {msg}");
    assert!(msg.contains("bad handshake"), "got: {msg}");
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("boom".into());
    let msg = err.to_string();
    assert!(msg.contains("fatal"), "got: {msg}");
    assert!(msg.contains("boom"), "got: {msg}");
}

#[test]
fn host_error_exited_with_code() {
    let err = HostError::Exited { code: Some(42) };
    let msg = err.to_string();
    assert!(msg.contains("exited"), "got: {msg}");
    assert!(msg.contains("42"), "got: {msg}");
}

#[test]
fn host_error_exited_without_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("exited"), "got: {msg}");
    assert!(msg.contains("None"), "got: {msg}");
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"), "got: {msg}");
    assert!(msg.contains("segfault"), "got: {msg}");
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"), "got: {msg}");
    assert!(msg.contains("30"), "got: {msg}");
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Error handling – spawn failure
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn spawn_nonexistent_binary_returns_spawn_error() {
    let spec = SidecarSpec::new("nonexistent-binary-xyz-sidecar-deep");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Spawn(_)),
        "expected Spawn error"
    );
}

#[tokio::test]
async fn spawn_empty_command_returns_error() {
    let spec = SidecarSpec::new("");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Protocol errors – malformed JSONL
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_malformed_json_returns_protocol_error() {
    let result = JsonlCodec::decode("{not valid json}");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
}

#[test]
fn decode_empty_string_returns_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn decode_valid_json_but_wrong_envelope_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":"stuff"}"#);
    assert!(result.is_err());
}

#[test]
fn decode_missing_t_field_returns_error() {
    let result = JsonlCodec::decode(r#"{"type":"hello","data":1}"#);
    assert!(result.is_err());
}

#[test]
fn decode_null_returns_error() {
    let result = JsonlCodec::decode("null");
    assert!(result.is_err());
}

#[test]
fn decode_array_returns_error() {
    let result = JsonlCodec::decode("[1,2,3]");
    assert!(result.is_err());
}

#[test]
fn decode_number_returns_error() {
    let result = JsonlCodec::decode("42");
    assert!(result.is_err());
}

#[test]
fn decode_boolean_returns_error() {
    let result = JsonlCodec::decode("true");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Sidecar registration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("mysc", "cmd")).unwrap();
    let cfg = reg.get("mysc").unwrap();
    assert_eq!(cfg.command, "cmd");
}

#[test]
fn registry_duplicate_is_error() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "cmd1")).unwrap();
    let err = reg.register(SidecarConfig::new("sc", "cmd2")).unwrap_err();
    assert!(err.to_string().contains("already registered"), "got: {err}");
}

#[test]
fn registry_invalid_config_rejected() {
    let mut reg = SidecarRegistry::default();
    let err = reg.register(SidecarConfig::new("", "cmd")).unwrap_err();
    assert!(err.to_string().contains("name"), "got: {err}");
    assert!(reg.list().is_empty());
}

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("zebra", "z")).unwrap();
    reg.register(SidecarConfig::new("alpha", "a")).unwrap();
    reg.register(SidecarConfig::new("middle", "m")).unwrap();
    assert_eq!(reg.list(), vec!["alpha", "middle", "zebra"]);
}

#[test]
fn registry_remove_returns_true_for_existing() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "cmd")).unwrap();
    assert!(reg.remove("sc"));
    assert!(reg.get("sc").is_none());
}

#[test]
fn registry_remove_returns_false_for_nonexistent() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("ghost"));
}

#[test]
fn registry_re_register_after_remove() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("sc", "cmd1")).unwrap();
    reg.remove("sc");
    reg.register(SidecarConfig::new("sc", "cmd2")).unwrap();
    assert_eq!(reg.get("sc").unwrap().command, "cmd2");
}

#[test]
fn registry_empty_has_no_entries() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
    assert!(reg.get("anything").is_none());
}

#[test]
fn registry_case_sensitive_names() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("SC", "upper")).unwrap();
    reg.register(SidecarConfig::new("sc", "lower")).unwrap();
    assert_eq!(reg.get("SC").unwrap().command, "upper");
    assert_eq!(reg.get("sc").unwrap().command, "lower");
    assert!(reg.get("Sc").is_none());
}

#[test]
fn registry_many_registrations() {
    let mut reg = SidecarRegistry::default();
    for i in 0..100 {
        reg.register(SidecarConfig::new(format!("s{i:04}"), format!("c{i}")))
            .unwrap();
    }
    assert_eq!(reg.list().len(), 100);
    assert_eq!(reg.list()[0], "s0000");
    assert_eq!(reg.list()[99], "s0099");
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Registry discovery from directory
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_discover_temp_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let node_dir = tmp.path().join("my-node");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// js").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    let cfg = reg.get("my-node").unwrap();
    assert_eq!(cfg.command, "node");
    assert!(cfg.args[0].contains("host.js"));
}

#[test]
fn registry_discover_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_discover_nonexistent_dir_is_error() {
    let result = SidecarRegistry::from_config_dir(Path::new("/no/such/dir/xyz123"));
    assert!(result.is_err());
}

#[test]
fn registry_discover_ignores_non_script_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empty-sc");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("README.md"), "nothing").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_discover_host_py() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("pysc");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("host.py"), "# py").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    let cfg = reg.get("pysc").unwrap();
    assert_eq!(cfg.command, "python");
}

#[test]
fn registry_discover_host_sh() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("shsc");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("host.sh"), "#!/bin/bash").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    let cfg = reg.get("shsc").unwrap();
    assert_eq!(cfg.command, "bash");
}

#[test]
fn registry_discover_prioritises_js_over_py() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("both");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("host.js"), "// js").unwrap();
    std::fs::write(dir.join("host.py"), "# py").unwrap();

    let reg = SidecarRegistry::from_config_dir(tmp.path()).unwrap();
    assert_eq!(reg.get("both").unwrap().command, "node");
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Lifecycle state machine
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_starts_uninitialized() {
    let lm = LifecycleManager::new();
    assert_eq!(*lm.state(), LifecycleState::Uninitialized);
    assert!(lm.history().is_empty());
    assert!(lm.uptime().is_none());
}

#[test]
fn lifecycle_valid_transitions() {
    let mut lm = LifecycleManager::new();
    lm.transition(LifecycleState::Starting, None).unwrap();
    assert_eq!(*lm.state(), LifecycleState::Starting);

    lm.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*lm.state(), LifecycleState::Ready);

    lm.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*lm.state(), LifecycleState::Running);

    lm.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*lm.state(), LifecycleState::Ready);

    lm.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*lm.state(), LifecycleState::Stopping);

    lm.transition(LifecycleState::Stopped, None).unwrap();
    assert_eq!(*lm.state(), LifecycleState::Stopped);

    assert_eq!(lm.history().len(), 6);
}

#[test]
fn lifecycle_invalid_transition_returns_error() {
    let mut lm = LifecycleManager::new();
    let err = lm.transition(LifecycleState::Running, None).unwrap_err();
    assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
}

#[test]
fn lifecycle_same_state_returns_already_in_state() {
    let mut lm = LifecycleManager::new();
    let err = lm
        .transition(LifecycleState::Uninitialized, None)
        .unwrap_err();
    assert!(matches!(err, LifecycleError::AlreadyInState(_)));
}

#[test]
fn lifecycle_failed_from_any_state() {
    for start in [
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ] {
        let mut lm = LifecycleManager::new();
        // Get to the start state.
        match start {
            LifecycleState::Uninitialized => {}
            LifecycleState::Starting => {
                lm.transition(LifecycleState::Starting, None).unwrap();
            }
            LifecycleState::Ready => {
                lm.transition(LifecycleState::Starting, None).unwrap();
                lm.transition(LifecycleState::Ready, None).unwrap();
            }
            LifecycleState::Running => {
                lm.transition(LifecycleState::Starting, None).unwrap();
                lm.transition(LifecycleState::Ready, None).unwrap();
                lm.transition(LifecycleState::Running, None).unwrap();
            }
            LifecycleState::Stopping => {
                lm.transition(LifecycleState::Starting, None).unwrap();
                lm.transition(LifecycleState::Ready, None).unwrap();
                lm.transition(LifecycleState::Stopping, None).unwrap();
            }
            _ => {}
        }
        assert_eq!(*lm.state(), start);
        lm.transition(LifecycleState::Failed, Some("test".into()))
            .unwrap();
        assert_eq!(*lm.state(), LifecycleState::Failed);
    }
}

#[test]
fn lifecycle_state_display() {
    assert_eq!(
        format!("{}", LifecycleState::Uninitialized),
        "uninitialized"
    );
    assert_eq!(format!("{}", LifecycleState::Starting), "starting");
    assert_eq!(format!("{}", LifecycleState::Ready), "ready");
    assert_eq!(format!("{}", LifecycleState::Running), "running");
    assert_eq!(format!("{}", LifecycleState::Stopping), "stopping");
    assert_eq!(format!("{}", LifecycleState::Stopped), "stopped");
    assert_eq!(format!("{}", LifecycleState::Failed), "failed");
}

#[test]
fn lifecycle_history_records_reason() {
    let mut lm = LifecycleManager::new();
    lm.transition(LifecycleState::Starting, Some("spawn".into()))
        .unwrap();
    let h = lm.history();
    assert_eq!(h[0].from, LifecycleState::Uninitialized);
    assert_eq!(h[0].to, LifecycleState::Starting);
    assert_eq!(h[0].reason.as_deref(), Some("spawn"));
}

#[test]
fn lifecycle_uptime_after_ready() {
    let mut lm = LifecycleManager::new();
    lm.transition(LifecycleState::Starting, None).unwrap();
    lm.transition(LifecycleState::Ready, None).unwrap();
    let uptime = lm.uptime();
    assert!(uptime.is_some());
}

#[test]
fn lifecycle_serde_state() {
    let state = LifecycleState::Ready;
    let json = serde_json::to_string(&state).unwrap();
    let de: LifecycleState = serde_json::from_str(&json).unwrap();
    assert_eq!(de, LifecycleState::Ready);
}

#[test]
fn lifecycle_default_is_uninitialized() {
    let lm = LifecycleManager::default();
    assert_eq!(*lm.state(), LifecycleState::Uninitialized);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Pool management
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pool_add_and_acquire() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.state, PoolEntryState::Busy);
    assert_eq!(entry.id, "s1");
}

#[test]
fn pool_release_makes_entry_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    pool.release(&entry.id);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_max_size_enforced() {
    let config = PoolConfig {
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3"));
}

#[test]
fn pool_acquire_returns_none_when_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_acquire_returns_none_when_all_busy() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire().unwrap();
    assert!(pool.acquire().is_none());
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
fn pool_remove_nonexistent_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.remove("ghost").is_none());
}

#[test]
fn pool_stats_comprehensive() {
    let config = PoolConfig {
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire(); // s1 -> busy
    pool.mark_failed("s2");
    pool.drain("s3");

    let stats = pool.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.draining, 1);
    assert_eq!(stats.idle, 0);
}

#[test]
fn pool_utilization() {
    let config = PoolConfig {
        max_size: 4,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    pool.add("s2");
    assert_eq!(pool.stats().utilization(), 0.0);

    pool.acquire();
    let stats = pool.stats();
    assert!((stats.utilization() - 0.5).abs() < 0.01);
}

#[test]
fn pool_config_default() {
    let config = PoolConfig::default();
    assert_eq!(config.min_size, 1);
    assert_eq!(config.max_size, 4);
    assert_eq!(config.idle_timeout, Duration::from_secs(300));
    assert_eq!(config.health_check_interval, Duration::from_secs(30));
}

#[test]
fn pool_config_serde_roundtrip() {
    let config = PoolConfig {
        min_size: 2,
        max_size: 8,
        idle_timeout: Duration::from_secs(120),
        health_check_interval: Duration::from_secs(15),
    };
    let json = serde_json::to_string(&config).unwrap();
    let de: PoolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(de.min_size, 2);
    assert_eq!(de.max_size, 8);
    assert_eq!(de.idle_timeout, Duration::from_secs(120));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Process tracking
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_info_new_is_not_started() {
    let spec = SidecarSpec::new("cmd");
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(!info.is_running());
    assert!(!info.is_terminated());
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
}

#[test]
fn process_status_running() {
    let spec = SidecarSpec::new("cmd");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 1234 };
    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_status_exited() {
    let spec = SidecarSpec::new("cmd");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(!info.is_running());
    assert!(info.is_terminated());
}

#[test]
fn process_status_killed() {
    let spec = SidecarSpec::new("cmd");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_status_timed_out() {
    let spec = SidecarSpec::new("cmd");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

#[test]
fn process_config_default() {
    let cfg = ProcessConfig::default();
    assert!(cfg.working_dir.is_none());
    assert!(cfg.env_vars.is_empty());
    assert!(cfg.timeout.is_none());
    assert!(cfg.inherit_env);
}

#[test]
fn process_config_serde_roundtrip() {
    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/work")),
        env_vars: BTreeMap::from([("K".into(), "V".into())]),
        timeout: Some(Duration::from_secs(30)),
        inherit_env: false,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let de: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(de.working_dir.as_deref(), Some(Path::new("/work")));
    assert_eq!(de.env_vars["K"], "V");
    assert_eq!(de.timeout, Some(Duration::from_secs(30)));
    assert!(!de.inherit_env);
}

#[test]
fn process_status_serde_roundtrip() {
    for status in [
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 42 },
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let de: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(de, status);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Retry configuration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_config_default() {
    let cfg = RetryConfig::default();
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.base_delay, Duration::from_millis(100));
    assert_eq!(cfg.max_delay, Duration::from_secs(10));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(60));
    assert!((cfg.jitter_factor - 0.5).abs() < 0.01);
}

#[test]
fn retry_config_serde_roundtrip() {
    let cfg = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(20),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.3,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let de: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(de.max_retries, 5);
    assert_eq!(de.overall_timeout, Duration::from_secs(120));
}

#[test]
fn compute_delay_increases_exponentially() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        ..RetryConfig::default()
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
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(300),
        ..RetryConfig::default()
    };
    let d5 = compute_delay(&cfg, 5);
    assert_eq!(d5, Duration::from_millis(300));
}

#[test]
fn is_retryable_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "nf"));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_stdout_error() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_exited_error() {
    assert!(is_retryable(&HostError::Exited { code: Some(1) }));
}

#[test]
fn is_retryable_timeout_error() {
    assert!(is_retryable(&HostError::Timeout {
        duration: Duration::from_secs(5)
    }));
}

#[test]
fn is_retryable_crashed_error() {
    assert!(is_retryable(&HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "err".into()
    }));
}

#[test]
fn is_not_retryable_protocol_error() {
    let err = HostError::Protocol(ProtocolError::Violation("bad".into()));
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_violation_error() {
    let err = HostError::Violation("bad protocol".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_fatal_error() {
    let err = HostError::Fatal("fatal fail".into());
    assert!(!is_retryable(&err));
}

#[test]
fn retry_metadata_to_receipt_metadata_empty() {
    let meta = RetryMetadata {
        total_attempts: 1,
        failed_attempts: vec![],
        total_duration: Duration::from_millis(50),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(1));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(50));
    assert!(!map.contains_key("retry_failed_attempts"));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Health monitoring
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_monitor_new_is_empty() {
    let hm = HealthMonitor::new();
    assert_eq!(hm.total_checks(), 0);
    assert!(!hm.all_healthy());
}

#[test]
fn health_monitor_record_healthy() {
    let mut hm = HealthMonitor::new();
    hm.record_check("sc1", HealthStatus::Healthy, Some(Duration::from_millis(5)));
    assert_eq!(hm.total_checks(), 1);
    assert!(hm.all_healthy());
    let check = hm.get_status("sc1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_record_unhealthy() {
    let mut hm = HealthMonitor::new();
    hm.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    assert!(!hm.all_healthy());
    assert_eq!(hm.unhealthy_sidecars().len(), 1);
    assert_eq!(hm.get_status("sc1").unwrap().consecutive_failures, 1);
}

#[test]
fn health_monitor_consecutive_failures_increment() {
    let mut hm = HealthMonitor::new();
    for _ in 0..3 {
        hm.record_check(
            "sc1",
            HealthStatus::Unhealthy {
                reason: "bad".into(),
            },
            None,
        );
    }
    assert_eq!(hm.get_status("sc1").unwrap().consecutive_failures, 3);
}

#[test]
fn health_monitor_healthy_resets_failures() {
    let mut hm = HealthMonitor::new();
    hm.record_check(
        "sc1",
        HealthStatus::Unhealthy {
            reason: "bad".into(),
        },
        None,
    );
    hm.record_check("sc1", HealthStatus::Healthy, None);
    assert_eq!(hm.get_status("sc1").unwrap().consecutive_failures, 0);
}

#[test]
fn health_monitor_uptime_percentage() {
    let mut hm = HealthMonitor::new();
    hm.record_check("sc1", HealthStatus::Healthy, None);
    hm.record_check("sc1", HealthStatus::Healthy, None);
    hm.record_check("sc1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    hm.record_check("sc1", HealthStatus::Healthy, None);
    // 3 out of 4 healthy = 75%
    let pct = hm.uptime_percentage("sc1");
    assert!((pct - 75.0).abs() < 0.01);
}

#[test]
fn health_monitor_uptime_unknown_returns_zero() {
    let hm = HealthMonitor::new();
    assert_eq!(hm.uptime_percentage("unknown"), 0.0);
}

#[test]
fn health_monitor_generate_report_empty() {
    let hm = HealthMonitor::new();
    let report = hm.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unknown));
    assert!(report.checks.is_empty());
}

#[test]
fn health_monitor_report_all_healthy() {
    let mut hm = HealthMonitor::new();
    hm.record_check("sc1", HealthStatus::Healthy, None);
    hm.record_check("sc2", HealthStatus::Healthy, None);
    let report = hm.generate_report();
    assert!(matches!(report.overall, HealthStatus::Healthy));
}

#[test]
fn health_monitor_report_one_unhealthy() {
    let mut hm = HealthMonitor::new();
    hm.record_check("sc1", HealthStatus::Healthy, None);
    hm.record_check(
        "sc2",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    let report = hm.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn health_monitor_report_degraded() {
    let mut hm = HealthMonitor::new();
    hm.record_check("sc1", HealthStatus::Healthy, None);
    hm.record_check(
        "sc2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = hm.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_monitor_default_is_new() {
    let hm = HealthMonitor::default();
    assert_eq!(hm.total_checks(), 0);
}

#[test]
fn health_status_serde_roundtrip() {
    for status in [
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        HealthStatus::Unknown,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let de: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(de, status);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Mock sidecar behavior (integration, requires Python)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn mock_sidecar_default_mode() {
    let py = require_python!();
    let spec = mock_spec(&py);
    let client = SidecarClient::spawn(spec).await.unwrap();
    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(client.hello.contract_version, CONTRACT_VERSION);

    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(!events.is_empty());
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn mock_sidecar_multi_events() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert_eq!(events.len(), 5);
    let receipt = sidecar_run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn mock_sidecar_multi_event_kinds() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "multi_event_kinds");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let events: Vec<_> = sidecar_run.events.collect().await;
    assert!(events.len() >= 4, "got {} events", events.len());
    // Verify we got varied event kinds.
    let has_delta = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
    let has_msg = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
    let has_file = events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::FileChanged { .. }));
    assert!(has_delta, "missing AssistantDelta");
    assert!(has_msg, "missing AssistantMessage");
    assert!(has_file, "missing FileChanged");
    sidecar_run.receipt.await.unwrap().unwrap();
    sidecar_run.wait.await.unwrap().unwrap();
}

#[tokio::test]
async fn mock_sidecar_fatal_mode() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "fatal");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let result = sidecar_run.receipt.await.unwrap();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Fatal(_)),
        "expected Fatal, got: {err}"
    );
}

#[tokio::test]
async fn mock_sidecar_no_hello_mode() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "no_hello");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Protocol(_)),
        "expected Protocol error, got: {err}"
    );
}

#[tokio::test]
async fn mock_sidecar_bad_json_midstream() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "bad_json_midstream");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run_id = Uuid::new_v4().to_string();
    let sidecar_run = client.run(run_id, test_work_order()).await.unwrap();
    let _events: Vec<_> = sidecar_run.events.collect().await;
    let result = sidecar_run.receipt.await.unwrap();
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), HostError::Protocol(_)),
        "expected Protocol error on malformed JSON"
    );
}

#[tokio::test]
async fn mock_sidecar_exit_nonzero() {
    let py = require_python!();
    let spec = mock_spec_with_mode(&py, "exit_nonzero");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, HostError::Exited { .. }),
        "expected Exited error, got: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_decode_only_whitespace_lines() {
    let input = "\n  \n\t\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(results.is_empty());
}

#[test]
fn edge_encode_then_decode_preserves_contract_version() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let line = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn edge_large_event_payload() {
    let large_text = "x".repeat(100_000);
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage { text: large_text },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert_eq!(text.len(), 100_000);
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn edge_empty_string_event_text() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: String::new(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert!(text.is_empty());
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn edge_unicode_in_event() {
    let env = make_event(
        "run-1",
        AgentEventKind::AssistantMessage {
            text: "こんにちは 🌍 مرحبا".into(),
        },
    );
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Event { event, .. } = decoded {
        if let AgentEventKind::AssistantMessage { text } = event.kind {
            assert!(text.contains("こんにちは"));
            assert!(text.contains("🌍"));
        } else {
            panic!("wrong kind");
        }
    } else {
        panic!("expected Event");
    }
}

#[test]
fn edge_envelope_with_extra_fields_ignored() {
    // Extra fields in JSON should be silently ignored by serde.
    let json = format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"x","backend_version":null,"adapter_version":null}},"capabilities":{{}},"extra_field":"ignored"}}"#,
        CONTRACT_VERSION
    );
    let decoded = JsonlCodec::decode(&json).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn edge_fatal_with_empty_error_string() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: String::new(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Fatal { error, .. } = decoded {
        assert!(error.is_empty());
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn edge_receipt_with_no_trace() {
    let receipt = test_receipt(Uuid::nil(), Uuid::nil());
    assert!(receipt.trace.is_empty());
    let env = Envelope::Final {
        ref_id: "r1".into(),
        receipt,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(line.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert!(receipt.trace.is_empty());
    } else {
        panic!("expected Final");
    }
}

#[test]
fn edge_spec_with_many_env_vars() {
    let mut spec = SidecarSpec::new("cmd");
    for i in 0..50 {
        spec.env.insert(format!("VAR_{i}"), format!("val_{i}"));
    }
    assert_eq!(spec.env.len(), 50);
    let json = serde_json::to_string(&spec).unwrap();
    let de: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(de.env.len(), 50);
}

#[test]
fn edge_pool_empty_utilization_is_zero() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert_eq!(pool.stats().utilization(), 0.0);
}

#[test]
fn edge_lifecycle_error_display() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Uninitialized,
        to: LifecycleState::Running,
    };
    let msg = err.to_string();
    assert!(msg.contains("invalid"), "got: {msg}");
    assert!(msg.contains("uninitialized"), "got: {msg}");
    assert!(msg.contains("running"), "got: {msg}");

    let err2 = LifecycleError::AlreadyInState(LifecycleState::Ready);
    let msg2 = err2.to_string();
    assert!(msg2.contains("already"), "got: {msg2}");
    assert!(msg2.contains("ready"), "got: {msg2}");
}

#[test]
fn edge_multiple_decode_errors_in_stream() {
    let input = "bad line 1\nbad line 2\nbad line 3\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_err()));
}

#[test]
fn edge_mixed_valid_and_invalid_in_stream() {
    let hello = Envelope::hello(test_backend(), CapabilityManifest::new());
    let hello_line = JsonlCodec::encode(&hello).unwrap();
    let input = format!("{hello_line}bad json here\n{hello_line}");
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
}

#[test]
fn edge_pool_active_count() {
    let config = PoolConfig {
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(config);
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    assert_eq!(pool.active_count(), 3);
    pool.acquire(); // one becomes busy
    assert_eq!(pool.active_count(), 3); // busy + idle are both active
    pool.mark_failed("s2");
    assert_eq!(pool.active_count(), 2); // failed is not active
}
