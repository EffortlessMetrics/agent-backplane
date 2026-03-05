#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive tests for the host spawning layer (abp-host).

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, Outcome, PolicyProfile, Receipt,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_host::health::{HealthCheck, HealthMonitor, HealthReport, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState, LifecycleTransition};
use abp_host::pool::{PoolConfig, PoolEntry, PoolEntryState, PoolStats, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{
    compute_delay, is_retryable, RetryAttempt, RetryConfig, RetryMetadata, RetryOutcome,
};
use abp_host::{HostError, SidecarClient, SidecarHello, SidecarSpec};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

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

/// Returns the path to the mock sidecar Python script inside crates/abp-host/tests/.
fn mock_script_path() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("crates")
        .join("abp-host")
        .join("tests")
        .join("mock_sidecar.py")
        .to_string_lossy()
        .into_owned()
}

/// Check whether `python3` (or `python` on Windows) is available.
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

fn make_spec_with_mode(py: &str, mode: &str) -> SidecarSpec {
    let mut spec = SidecarSpec::new(py);
    spec.args = vec![mock_script_path(), mode.into()];
    spec
}

// ===========================================================================
// 1. SidecarSpec construction
// ===========================================================================

#[test]
fn spec_new_sets_command() {
    let spec = SidecarSpec::new("my-cmd");
    assert_eq!(spec.command, "my-cmd");
}

#[test]
fn spec_new_empty_args() {
    let spec = SidecarSpec::new("x");
    assert!(spec.args.is_empty());
}

#[test]
fn spec_new_empty_env() {
    let spec = SidecarSpec::new("x");
    assert!(spec.env.is_empty());
}

#[test]
fn spec_new_no_cwd() {
    let spec = SidecarSpec::new("x");
    assert!(spec.cwd.is_none());
}

#[test]
fn spec_command_from_string() {
    let cmd = String::from("hello");
    let spec = SidecarSpec::new(cmd);
    assert_eq!(spec.command, "hello");
}

#[test]
fn spec_command_from_str_ref() {
    let spec = SidecarSpec::new("world");
    assert_eq!(spec.command, "world");
}

#[test]
fn spec_args_settable() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["index.js".into(), "--port".into(), "3000".into()];
    assert_eq!(spec.args.len(), 3);
    assert_eq!(spec.args[2], "3000");
}

#[test]
fn spec_env_settable() {
    let mut spec = SidecarSpec::new("node");
    spec.env.insert("KEY".into(), "VALUE".into());
    assert_eq!(spec.env.get("KEY").unwrap(), "VALUE");
}

#[test]
fn spec_cwd_settable() {
    let mut spec = SidecarSpec::new("node");
    spec.cwd = Some("/tmp/work".into());
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn spec_clone() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["a.js".into()];
    spec.env.insert("K".into(), "V".into());
    spec.cwd = Some("/work".into());
    let cloned = spec.clone();
    assert_eq!(cloned.command, spec.command);
    assert_eq!(cloned.args, spec.args);
    assert_eq!(cloned.env, spec.env);
    assert_eq!(cloned.cwd, spec.cwd);
}

#[test]
fn spec_debug_impl() {
    let spec = SidecarSpec::new("test");
    let dbg = format!("{:?}", spec);
    assert!(dbg.contains("SidecarSpec"));
    assert!(dbg.contains("test"));
}

#[test]
fn spec_serde_roundtrip() {
    let mut spec = SidecarSpec::new("python3");
    spec.args = vec!["main.py".into()];
    spec.env.insert("FOO".into(), "bar".into());
    spec.cwd = Some("/tmp".into());
    let json = serde_json::to_string(&spec).unwrap();
    let deserialized: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.command, "python3");
    assert_eq!(deserialized.args, vec!["main.py"]);
    assert_eq!(deserialized.env.get("FOO").unwrap(), "bar");
    assert_eq!(deserialized.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn spec_serde_minimal() {
    let spec = SidecarSpec::new("cmd");
    let json = serde_json::to_string(&spec).unwrap();
    let back: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.command, "cmd");
    assert!(back.args.is_empty());
    assert!(back.env.is_empty());
    assert!(back.cwd.is_none());
}

#[test]
fn spec_multiple_env_vars() {
    let mut spec = SidecarSpec::new("n");
    spec.env.insert("A".into(), "1".into());
    spec.env.insert("B".into(), "2".into());
    spec.env.insert("C".into(), "3".into());
    assert_eq!(spec.env.len(), 3);
}

#[test]
fn spec_env_btreemap_ordered() {
    let mut spec = SidecarSpec::new("n");
    spec.env.insert("Z".into(), "last".into());
    spec.env.insert("A".into(), "first".into());
    let keys: Vec<&String> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A", "Z"]);
}

// ===========================================================================
// 2. HostError display and variants
// ===========================================================================

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"));
    let msg = err.to_string();
    assert!(msg.contains("spawn"));
    assert!(msg.contains("nope"));
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(err.to_string().contains("stdout"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
    assert!(err.to_string().contains("stdin"));
}

#[test]
fn host_error_protocol_display() {
    let proto_err = abp_protocol::ProtocolError::Violation("bad".into());
    let err = HostError::Protocol(proto_err);
    let msg = err.to_string();
    assert!(msg.contains("protocol"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("bad hello".into());
    let msg = err.to_string();
    assert!(msg.contains("violation"));
    assert!(msg.contains("bad hello"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("boom".into());
    let msg = err.to_string();
    assert!(msg.contains("fatal"));
    assert!(msg.contains("boom"));
}

#[test]
fn host_error_exited_with_code() {
    let err = HostError::Exited { code: Some(1) };
    let msg = err.to_string();
    assert!(msg.contains("exited"));
    assert!(msg.contains("1"));
}

#[test]
fn host_error_exited_no_code() {
    let err = HostError::Exited { code: None };
    let msg = err.to_string();
    assert!(msg.contains("exited"));
    assert!(msg.contains("None"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(139),
        stderr: "segfault".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"));
    assert!(msg.contains("139"));
    assert!(msg.contains("segfault"));
}

#[test]
fn host_error_sidecar_crashed_no_code() {
    let err = HostError::SidecarCrashed {
        exit_code: None,
        stderr: "unknown".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("crashed"));
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
fn host_error_debug_impl() {
    let err = HostError::Fatal("test".into());
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("Fatal"));
}

// ===========================================================================
// 3. ProcessConfig
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
fn process_config_with_working_dir() {
    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/tmp")),
        ..Default::default()
    };
    assert_eq!(cfg.working_dir.unwrap(), PathBuf::from("/tmp"));
}

#[test]
fn process_config_with_env_vars() {
    let mut vars = BTreeMap::new();
    vars.insert("RUST_LOG".into(), "debug".into());
    let cfg = ProcessConfig {
        env_vars: vars,
        ..Default::default()
    };
    assert_eq!(cfg.env_vars.get("RUST_LOG").unwrap(), "debug");
}

#[test]
fn process_config_with_timeout() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    };
    assert_eq!(cfg.timeout.unwrap(), Duration::from_secs(60));
}

#[test]
fn process_config_inherit_env_default_true() {
    let cfg = ProcessConfig::default();
    assert!(cfg.inherit_env);
}

#[test]
fn process_config_inherit_env_false() {
    let cfg = ProcessConfig {
        inherit_env: false,
        ..Default::default()
    };
    assert!(!cfg.inherit_env);
}

#[test]
fn process_config_serde_roundtrip() {
    let mut vars = BTreeMap::new();
    vars.insert("A".into(), "B".into());
    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/work")),
        env_vars: vars,
        timeout: Some(Duration::from_millis(5000)),
        inherit_env: false,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.working_dir.unwrap(), PathBuf::from("/work"));
    assert_eq!(back.env_vars.get("A").unwrap(), "B");
    assert_eq!(back.timeout.unwrap(), Duration::from_millis(5000));
    assert!(!back.inherit_env);
}

#[test]
fn process_config_serde_no_timeout() {
    let cfg = ProcessConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("timeout"));
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert!(back.timeout.is_none());
}

#[test]
fn process_config_clone() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_secs(10)),
        ..Default::default()
    };
    let cloned = cfg.clone();
    assert_eq!(cloned.timeout, cfg.timeout);
}

// ===========================================================================
// 4. ProcessStatus
// ===========================================================================

#[test]
fn process_status_not_started() {
    let s = ProcessStatus::NotStarted;
    assert_eq!(s, ProcessStatus::NotStarted);
}

#[test]
fn process_status_running() {
    let s = ProcessStatus::Running { pid: 1234 };
    assert!(matches!(s, ProcessStatus::Running { pid: 1234 }));
}

#[test]
fn process_status_exited() {
    let s = ProcessStatus::Exited { code: 0 };
    assert!(matches!(s, ProcessStatus::Exited { code: 0 }));
}

#[test]
fn process_status_killed() {
    let s = ProcessStatus::Killed;
    assert_eq!(s, ProcessStatus::Killed);
}

#[test]
fn process_status_timed_out() {
    let s = ProcessStatus::TimedOut;
    assert_eq!(s, ProcessStatus::TimedOut);
}

#[test]
fn process_status_serde_roundtrip_not_started() {
    let s = ProcessStatus::NotStarted;
    let json = serde_json::to_string(&s).unwrap();
    let back: ProcessStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ProcessStatus::NotStarted);
}

#[test]
fn process_status_serde_roundtrip_running() {
    let s = ProcessStatus::Running { pid: 42 };
    let json = serde_json::to_string(&s).unwrap();
    let back: ProcessStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ProcessStatus::Running { pid: 42 });
}

#[test]
fn process_status_serde_roundtrip_exited() {
    let s = ProcessStatus::Exited { code: 127 };
    let json = serde_json::to_string(&s).unwrap();
    let back: ProcessStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ProcessStatus::Exited { code: 127 });
}

#[test]
fn process_status_serde_roundtrip_killed() {
    let s = ProcessStatus::Killed;
    let json = serde_json::to_string(&s).unwrap();
    let back: ProcessStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ProcessStatus::Killed);
}

#[test]
fn process_status_serde_roundtrip_timed_out() {
    let s = ProcessStatus::TimedOut;
    let json = serde_json::to_string(&s).unwrap();
    let back: ProcessStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ProcessStatus::TimedOut);
}

// ===========================================================================
// 5. ProcessInfo
// ===========================================================================

#[test]
fn process_info_new() {
    let spec = SidecarSpec::new("node");
    let cfg = ProcessConfig::default();
    let info = ProcessInfo::new(spec, cfg);
    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
}

#[test]
fn process_info_is_running_false_initially() {
    let info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    assert!(!info.is_running());
}

#[test]
fn process_info_is_running_true() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 1 };
    assert!(info.is_running());
}

#[test]
fn process_info_is_terminated_false_initially() {
    let info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_is_terminated_exited() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_killed() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_timed_out() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_running_false() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 99 };
    assert!(!info.is_terminated());
}

#[test]
fn process_info_spec_preserved() {
    let mut spec = SidecarSpec::new("python3");
    spec.args = vec!["main.py".into()];
    let info = ProcessInfo::new(spec, ProcessConfig::default());
    assert_eq!(info.spec.command, "python3");
    assert_eq!(info.spec.args, vec!["main.py"]);
}

#[test]
fn process_info_config_preserved() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_secs(30)),
        ..Default::default()
    };
    let info = ProcessInfo::new(SidecarSpec::new("n"), cfg);
    assert_eq!(info.config.timeout, Some(Duration::from_secs(30)));
}

#[test]
fn process_info_started_at_settable() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.started_at = Some(Utc::now());
    assert!(info.started_at.is_some());
}

#[test]
fn process_info_ended_at_settable() {
    let mut info = ProcessInfo::new(SidecarSpec::new("n"), ProcessConfig::default());
    info.ended_at = Some(Utc::now());
    assert!(info.ended_at.is_some());
}

// ===========================================================================
// 6. LifecycleManager
// ===========================================================================

#[test]
fn lifecycle_initial_state() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_default() {
    let mgr = LifecycleManager::default();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_uninitialized_to_starting() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Starting);
}

#[test]
fn lifecycle_starting_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_ready_to_running() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Running);
}

#[test]
fn lifecycle_running_to_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_running_to_stopping() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Stopping);
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

#[test]
fn lifecycle_any_to_failed() {
    for initial in &[
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Stopping,
    ] {
        let mut mgr = LifecycleManager::new();
        if *initial != LifecycleState::Uninitialized {
            // Get to the initial state
            if *initial == LifecycleState::Starting {
                mgr.transition(LifecycleState::Starting, None).unwrap();
            }
            if *initial == LifecycleState::Stopping {
                mgr.transition(LifecycleState::Starting, None).unwrap();
                mgr.transition(LifecycleState::Ready, None).unwrap();
                mgr.transition(LifecycleState::Stopping, None).unwrap();
            }
        }
        mgr.transition(LifecycleState::Failed, Some("error".into()))
            .unwrap();
        assert_eq!(*mgr.state(), LifecycleState::Failed);
    }
}

#[test]
fn lifecycle_invalid_uninitialized_to_running() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Running, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        LifecycleError::InvalidTransition { from, to } => {
            assert_eq!(from, LifecycleState::Uninitialized);
            assert_eq!(to, LifecycleState::Running);
        }
        other => panic!("expected InvalidTransition, got {:?}", other),
    }
}

#[test]
fn lifecycle_already_in_state() {
    let mgr = LifecycleManager::new();
    // Can't transition to the state we're already in
    let mut mgr = mgr;
    let result = mgr.transition(LifecycleState::Uninitialized, None);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        LifecycleError::AlreadyInState(LifecycleState::Uninitialized)
    ));
}

#[test]
fn lifecycle_history_recorded() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("booting".into()))
        .unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let hist = mgr.history();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].from, LifecycleState::Uninitialized);
    assert_eq!(hist[0].to, LifecycleState::Starting);
    assert_eq!(hist[0].reason.as_deref(), Some("booting"));
    assert_eq!(hist[1].from, LifecycleState::Starting);
    assert_eq!(hist[1].to, LifecycleState::Ready);
    assert!(hist[1].reason.is_none());
}

#[test]
fn lifecycle_uptime_none_initially() {
    let mgr = LifecycleManager::new();
    assert!(mgr.uptime().is_none());
}

#[test]
fn lifecycle_uptime_some_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let up = mgr.uptime();
    assert!(up.is_some());
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
fn lifecycle_state_serde_roundtrip() {
    for state in &[
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
        LifecycleState::Failed,
    ] {
        let json = serde_json::to_string(state).unwrap();
        let back: LifecycleState = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, state);
    }
}

#[test]
fn lifecycle_error_display_invalid_transition() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Uninitialized,
        to: LifecycleState::Running,
    };
    let msg = err.to_string();
    assert!(msg.contains("invalid"));
    assert!(msg.contains("uninitialized"));
    assert!(msg.contains("running"));
}

#[test]
fn lifecycle_error_display_already_in_state() {
    let err = LifecycleError::AlreadyInState(LifecycleState::Ready);
    let msg = err.to_string();
    assert!(msg.contains("already"));
    assert!(msg.contains("ready"));
}

#[test]
fn lifecycle_can_transition_checks() {
    let mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Ready));
    assert!(!mgr.can_transition(&LifecycleState::Running));
    assert!(!mgr.can_transition(&LifecycleState::Stopping));
    assert!(!mgr.can_transition(&LifecycleState::Stopped));
    // Failed is always allowed
    assert!(mgr.can_transition(&LifecycleState::Failed));
}

// ===========================================================================
// 7. SidecarConfig and Registry
// ===========================================================================

#[test]
fn sidecar_config_new() {
    let cfg = SidecarConfig::new("test", "node");
    assert_eq!(cfg.name, "test");
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
fn sidecar_config_validate_both_empty() {
    let cfg = SidecarConfig::new("", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_to_spec() {
    let mut cfg = SidecarConfig::new("test", "python3");
    cfg.args = vec!["main.py".into()];
    cfg.env.insert("KEY".into(), "VAL".into());
    cfg.working_dir = Some(PathBuf::from("/work"));
    let spec = cfg.to_spec();
    assert_eq!(spec.command, "python3");
    assert_eq!(spec.args, vec!["main.py"]);
    assert_eq!(spec.env.get("KEY").unwrap(), "VAL");
    assert_eq!(spec.cwd.as_deref(), Some("/work"));
}

#[test]
fn sidecar_config_to_spec_no_cwd() {
    let cfg = SidecarConfig::new("test", "node");
    let spec = cfg.to_spec();
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_config_serde_roundtrip() {
    let mut cfg = SidecarConfig::new("my-sidecar", "bash");
    cfg.args = vec!["-c".into(), "echo hello".into()];
    cfg.env.insert("X".into(), "Y".into());
    cfg.working_dir = Some(PathBuf::from("/opt"));
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "my-sidecar");
    assert_eq!(back.command, "bash");
    assert_eq!(back.args.len(), 2);
}

#[test]
fn registry_empty() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    let cfg = SidecarConfig::new("node", "node");
    reg.register(cfg).unwrap();
    assert!(reg.get("node").is_some());
    assert_eq!(reg.get("node").unwrap().command, "node");
}

#[test]
fn registry_register_duplicate_fails() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    let result = reg.register(SidecarConfig::new("node", "python"));
    assert!(result.is_err());
}

#[test]
fn registry_register_invalid_config_fails() {
    let mut reg = SidecarRegistry::default();
    let result = reg.register(SidecarConfig::new("", "node"));
    assert!(result.is_err());
}

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("zeta", "z")).unwrap();
    reg.register(SidecarConfig::new("alpha", "a")).unwrap();
    reg.register(SidecarConfig::new("beta", "b")).unwrap();
    let names = reg.list();
    assert_eq!(names, vec!["alpha", "beta", "zeta"]);
}

#[test]
fn registry_remove() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    assert!(reg.remove("node"));
    assert!(reg.get("node").is_none());
}

#[test]
fn registry_remove_nonexistent() {
    let mut reg = SidecarRegistry::default();
    assert!(!reg.remove("ghost"));
}

#[test]
fn registry_get_nonexistent() {
    let reg = SidecarRegistry::default();
    assert!(reg.get("nothing").is_none());
}

#[test]
fn registry_from_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let node_dir = dir.path().join("mynode");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// js").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.get("mynode").is_some());
    let cfg = reg.get("mynode").unwrap();
    assert_eq!(cfg.command, "node");
}

#[test]
fn registry_from_config_dir_python() {
    let dir = tempfile::tempdir().unwrap();
    let py_dir = dir.path().join("mypython");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "# py").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.get("mypython").is_some());
    let cfg = reg.get("mypython").unwrap();
    assert_eq!(cfg.command, "python");
}

#[test]
fn registry_from_config_dir_bash() {
    let dir = tempfile::tempdir().unwrap();
    let sh_dir = dir.path().join("mysh");
    std::fs::create_dir(&sh_dir).unwrap();
    std::fs::write(sh_dir.join("host.sh"), "#!/bin/bash").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.get("mysh").is_some());
    let cfg = reg.get("mysh").unwrap();
    assert_eq!(cfg.command, "bash");
}

#[test]
fn registry_from_config_dir_ignores_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("not_a_dir.txt"), "hi").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_from_config_dir_empty() {
    let dir = tempfile::tempdir().unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_from_config_dir_nonexistent() {
    let result = SidecarRegistry::from_config_dir(Path::new("/nonexistent_dir_xyz"));
    assert!(result.is_err());
}

#[test]
fn registry_discover_from_dir_alias() {
    let dir = tempfile::tempdir().unwrap();
    let reg = SidecarRegistry::discover_from_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

// ===========================================================================
// 8. RetryConfig
// ===========================================================================

#[test]
fn retry_config_default() {
    let cfg = RetryConfig::default();
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.base_delay, Duration::from_millis(100));
    assert_eq!(cfg.max_delay, Duration::from_secs(10));
    assert_eq!(cfg.overall_timeout, Duration::from_secs(60));
    assert!((cfg.jitter_factor - 0.5).abs() < f64::EPSILON);
}

#[test]
fn retry_config_serde_roundtrip() {
    let cfg = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(5),
        overall_timeout: Duration::from_secs(30),
        jitter_factor: 0.3,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_retries, 5);
    assert_eq!(back.base_delay, Duration::from_millis(200));
    assert_eq!(back.max_delay, Duration::from_secs(5));
    assert_eq!(back.overall_timeout, Duration::from_secs(30));
}

#[test]
fn retry_config_zero_retries() {
    let cfg = RetryConfig {
        max_retries: 0,
        ..Default::default()
    };
    assert_eq!(cfg.max_retries, 0);
}

// ===========================================================================
// 9. compute_delay
// ===========================================================================

#[test]
fn compute_delay_zero_jitter() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        ..Default::default()
    };
    // attempt 0: 100 * 2^0 = 100ms
    let d = compute_delay(&cfg, 0);
    assert_eq!(d, Duration::from_millis(100));
    // attempt 1: 100 * 2^1 = 200ms
    let d = compute_delay(&cfg, 1);
    assert_eq!(d, Duration::from_millis(200));
    // attempt 2: 100 * 2^2 = 400ms
    let d = compute_delay(&cfg, 2);
    assert_eq!(d, Duration::from_millis(400));
}

#[test]
fn compute_delay_capped_at_max() {
    let cfg = RetryConfig {
        jitter_factor: 0.0,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(500),
        ..Default::default()
    };
    // attempt 3: 100 * 8 = 800, capped to 500
    let d = compute_delay(&cfg, 3);
    assert_eq!(d, Duration::from_millis(500));
}

#[test]
fn compute_delay_with_jitter_bounded() {
    let cfg = RetryConfig {
        jitter_factor: 0.5,
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(10),
        ..Default::default()
    };
    // attempt 0: base = 1000ms, jitter up to 500ms subtracted
    // so result should be in [500, 1000]
    let d = compute_delay(&cfg, 0);
    assert!(d >= Duration::from_millis(500));
    assert!(d <= Duration::from_millis(1000));
}

#[test]
fn compute_delay_full_jitter() {
    let cfg = RetryConfig {
        jitter_factor: 1.0,
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(10),
        ..Default::default()
    };
    let d = compute_delay(&cfg, 0);
    assert!(d <= Duration::from_millis(1000));
}

// ===========================================================================
// 10. is_retryable
// ===========================================================================

#[test]
fn is_retryable_spawn() {
    let err = HostError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "n"));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_stdout() {
    let err = HostError::Stdout(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "p"));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_exited() {
    let err = HostError::Exited { code: Some(1) };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_crashed() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(1),
        stderr: "x".into(),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_timeout() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(1),
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
    let proto = abp_protocol::ProtocolError::Violation("x".into());
    let err = HostError::Protocol(proto);
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_stdin() {
    let err = HostError::Stdin(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "p"));
    assert!(!is_retryable(&err));
}

// ===========================================================================
// 11. RetryMetadata
// ===========================================================================

#[test]
fn retry_metadata_default() {
    let meta = RetryMetadata::default();
    assert_eq!(meta.total_attempts, 0);
    assert!(meta.failed_attempts.is_empty());
    assert_eq!(meta.total_duration, Duration::ZERO);
}

#[test]
fn retry_metadata_to_receipt_no_failures() {
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

#[test]
fn retry_metadata_to_receipt_with_failures() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![
            RetryAttempt {
                attempt: 0,
                error: "fail1".into(),
                delay: Duration::from_millis(100),
            },
            RetryAttempt {
                attempt: 1,
                error: "fail2".into(),
                delay: Duration::from_millis(200),
            },
        ],
        total_duration: Duration::from_millis(500),
    };
    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    assert!(map.contains_key("retry_failed_attempts"));
    let attempts = map["retry_failed_attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
}

#[test]
fn retry_metadata_serde_roundtrip() {
    let meta = RetryMetadata {
        total_attempts: 2,
        failed_attempts: vec![RetryAttempt {
            attempt: 0,
            error: "oops".into(),
            delay: Duration::from_millis(100),
        }],
        total_duration: Duration::from_millis(300),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: RetryMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_attempts, 2);
    assert_eq!(back.failed_attempts.len(), 1);
    assert_eq!(back.failed_attempts[0].error, "oops");
}

// ===========================================================================
// 12. HealthMonitor
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
    mon.record_check("s1", HealthStatus::Healthy, Some(Duration::from_millis(5)));
    assert_eq!(mon.total_checks(), 1);
    assert!(mon.all_healthy());
}

#[test]
fn health_monitor_record_unhealthy() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    assert!(!mon.all_healthy());
    assert_eq!(mon.unhealthy_sidecars().len(), 1);
}

#[test]
fn health_monitor_degraded() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "s1",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    assert!(!mon.all_healthy());
}

#[test]
fn health_monitor_consecutive_failures() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    mon.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    let check = mon.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 2);
}

#[test]
fn health_monitor_consecutive_failures_reset_on_healthy() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "s1",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );
    mon.record_check("s1", HealthStatus::Healthy, None);
    let check = mon.get_status("s1").unwrap();
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn health_monitor_get_status_nonexistent() {
    let mon = HealthMonitor::new();
    assert!(mon.get_status("ghost").is_none());
}

#[test]
fn health_monitor_uptime_percentage_no_history() {
    let mon = HealthMonitor::new();
    assert!((mon.uptime_percentage("s1") - 0.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_uptime_percentage_all_healthy() {
    let mut mon = HealthMonitor::new();
    for _ in 0..10 {
        mon.record_check("s1", HealthStatus::Healthy, None);
    }
    assert!((mon.uptime_percentage("s1") - 100.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_uptime_percentage_half() {
    let mut mon = HealthMonitor::new();
    for _ in 0..5 {
        mon.record_check("s1", HealthStatus::Healthy, None);
    }
    for _ in 0..5 {
        mon.record_check("s1", HealthStatus::Unhealthy { reason: "x".into() }, None);
    }
    assert!((mon.uptime_percentage("s1") - 50.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_generate_report_empty() {
    let mon = HealthMonitor::new();
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unknown));
    assert!(report.checks.is_empty());
}

#[test]
fn health_monitor_generate_report_all_healthy() {
    let mut mon = HealthMonitor::new();
    mon.record_check("s1", HealthStatus::Healthy, None);
    mon.record_check("s2", HealthStatus::Healthy, None);
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Healthy));
    assert_eq!(report.checks.len(), 2);
}

#[test]
fn health_monitor_generate_report_one_unhealthy() {
    let mut mon = HealthMonitor::new();
    mon.record_check("s1", HealthStatus::Healthy, None);
    mon.record_check(
        "s2",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        None,
    );
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn health_monitor_generate_report_one_degraded() {
    let mut mon = HealthMonitor::new();
    mon.record_check("s1", HealthStatus::Healthy, None);
    mon.record_check(
        "s2",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_status_serde_roundtrip() {
    let statuses = vec![
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        HealthStatus::Unknown,
    ];
    for s in statuses {
        let json = serde_json::to_string(&s).unwrap();
        let back: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}

// ===========================================================================
// 13. Pool
// ===========================================================================

#[test]
fn pool_config_default() {
    let cfg = PoolConfig::default();
    assert_eq!(cfg.min_size, 1);
    assert_eq!(cfg.max_size, 4);
    assert_eq!(cfg.idle_timeout, Duration::from_secs(300));
    assert_eq!(cfg.health_check_interval, Duration::from_secs(30));
}

#[test]
fn pool_config_serde_roundtrip() {
    let cfg = PoolConfig {
        min_size: 2,
        max_size: 8,
        idle_timeout: Duration::from_secs(600),
        health_check_interval: Duration::from_secs(15),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: PoolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.min_size, 2);
    assert_eq!(back.max_size, 8);
}

#[test]
fn pool_new_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn pool_add() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    assert_eq!(pool.total_count(), 1);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_add_respects_max_size() {
    let pool = SidecarPool::new(PoolConfig {
        max_size: 2,
        ..Default::default()
    });
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3"));
}

#[test]
fn pool_acquire() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    assert_eq!(entry.id, "s1");
    assert_eq!(entry.state, PoolEntryState::Busy);
}

#[test]
fn pool_acquire_empty_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_acquire_all_busy_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire().unwrap();
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_release() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let entry = pool.acquire().unwrap();
    assert_eq!(pool.idle_count(), 0);
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
fn pool_drain() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.drain("s1");
    let stats = pool.stats();
    assert_eq!(stats.draining, 1);
}

#[test]
fn pool_remove() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    let removed = pool.remove("s1");
    assert!(removed.is_some());
    assert_eq!(pool.total_count(), 0);
}

#[test]
fn pool_remove_nonexistent() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.remove("ghost").is_none());
}

#[test]
fn pool_stats() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire(); // s1 → busy
    pool.mark_failed("s3");
    let stats = pool.stats();
    assert_eq!(stats.total, 3);
    assert_eq!(stats.idle, 1);
    assert_eq!(stats.busy, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.draining, 0);
}

#[test]
fn pool_stats_utilization_empty() {
    let stats = PoolStats {
        total: 0,
        idle: 0,
        busy: 0,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn pool_stats_utilization_half() {
    let stats = PoolStats {
        total: 4,
        idle: 2,
        busy: 2,
        draining: 0,
        failed: 0,
    };
    assert!((stats.utilization() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn pool_config_ref() {
    let cfg = PoolConfig {
        min_size: 3,
        ..Default::default()
    };
    let pool = SidecarPool::new(cfg);
    assert_eq!(pool.config().min_size, 3);
}

// ===========================================================================
// 14. JSONL handshake validation (Envelope tests)
// ===========================================================================

#[test]
fn envelope_hello_encode_decode() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&hello).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn envelope_hello_contains_contract_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("abp/v0.1"));
}

#[test]
fn envelope_run_encode_decode() {
    let wo = test_work_order();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let encoded = JsonlCodec::encode(&run).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Run { .. }));
}

#[test]
fn envelope_fatal_encode_decode() {
    let fatal = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "bad thing".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Envelope::Fatal { error, .. } => assert_eq!(error, "bad thing"),
        _ => panic!("expected Fatal"),
    }
}

#[test]
fn envelope_decode_invalid_json() {
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
}

#[test]
fn envelope_decode_unknown_type() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_type","data":1}"#);
    assert!(result.is_err());
}

#[test]
fn envelope_hello_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    let hello = Envelope::hello(
        BackendIdentity {
            id: "cap-test".into(),
            backend_version: None,
            adapter_version: None,
        },
        caps,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("tool_read"));
    assert!(json.contains("streaming"));
}

// ===========================================================================
// 15. SidecarHello serialization
// ===========================================================================

#[test]
fn sidecar_hello_serde_roundtrip() {
    let hello = SidecarHello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        capabilities: {
            let mut m = CapabilityManifest::new();
            m.insert(Capability::ToolRead, SupportLevel::Native);
            m
        },
    };
    let json = serde_json::to_string(&hello).unwrap();
    let back: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(back.contract_version, "abp/v0.1");
    assert_eq!(back.backend.id, "mock");
    assert!(back.capabilities.contains_key(&Capability::ToolRead));
}

#[test]
fn sidecar_hello_debug() {
    let hello = SidecarHello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };
    let dbg = format!("{:?}", hello);
    assert!(dbg.contains("SidecarHello"));
}

// ===========================================================================
// 16. Spawn integration tests (require Python)
// ===========================================================================

#[tokio::test]
async fn spawn_receives_hello() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => {
            eprintln!("SKIP: python not found");
            return;
        }
    };
    let spec = make_spec_with_mode(&py, "default");
    let client = SidecarClient::spawn(spec).await.expect("spawn succeeds");
    assert_eq!(client.hello.backend.id, "mock-test");
    assert_eq!(client.hello.backend.backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn spawn_binary_not_found() {
    let spec = SidecarSpec::new("nonexistent_binary_xyz_12345");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Spawn(_)));
}

#[tokio::test]
async fn spawn_exit_nonzero_no_hello() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "exit_nonzero");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn spawn_no_hello_protocol_error() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "no_hello");
    let result = SidecarClient::spawn(spec).await;
    assert!(result.is_err());
    // Should be a protocol error (unexpected message)
    match result.unwrap_err() {
        HostError::Protocol(_) => {}
        other => panic!("expected Protocol error, got: {:?}", other),
    }
}

#[tokio::test]
async fn run_default_flow() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "default");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let wo = test_work_order();
    let run_id = Uuid::new_v4().to_string();
    let run = client.run(run_id, wo).await.unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert!(!events.is_empty());
    let receipt = run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.backend.id, "mock-test");
}

#[tokio::test]
async fn run_multi_events() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "multi_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client.run("run-2".into(), test_work_order()).await.unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn run_fatal_error() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "fatal");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client.run("run-f".into(), test_work_order()).await.unwrap();
    let _events: Vec<_> = run.events.collect().await;
    let result = run.receipt.await.unwrap();
    assert!(result.is_err());
    match result.unwrap_err() {
        HostError::Fatal(msg) => assert!(msg.contains("something went wrong")),
        other => panic!("expected Fatal, got: {:?}", other),
    }
}

#[tokio::test]
async fn run_bad_json_midstream() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "bad_json_midstream");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-bj".into(), test_work_order())
        .await
        .unwrap();
    let _events: Vec<_> = run.events.collect().await;
    let result = run.receipt.await.unwrap();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Protocol(_)));
}

#[tokio::test]
async fn run_env_var_passing() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "echo_env".into()];
    spec.env
        .insert("ABP_TEST_VAR".into(), "hello_from_test".into());
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-env".into(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    let has_env = events.iter().any(|e| {
        if let AgentEventKind::RunStarted { message } = &e.kind {
            message.contains("hello_from_test")
        } else {
            false
        }
    });
    assert!(has_env, "Expected env var in events: {:?}", events);
}

#[tokio::test]
async fn run_working_directory() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let tmpdir = tempfile::tempdir().unwrap();
    let mut spec = SidecarSpec::new(&py);
    spec.args = vec![mock_script_path(), "echo_cwd".into()];
    spec.cwd = Some(tmpdir.path().to_string_lossy().into_owned());
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-cwd".into(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    let has_cwd = events.iter().any(|e| {
        if let AgentEventKind::RunStarted { message } = &e.kind {
            message.contains("cwd=")
        } else {
            false
        }
    });
    assert!(has_cwd, "Expected cwd in events: {:?}", events);
}

#[tokio::test]
async fn run_wrong_ref_id_events_dropped() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "wrong_ref_id");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-wr".into(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    // The wrong-ref-id event should be dropped, so we get 2 events with correct ref
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn run_empty_lines_ignored() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "empty_lines");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert_eq!(events.len(), 2);
    let receipt = run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_unicode_content() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "unicode_content");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert_eq!(events.len(), 2);
    let receipt = run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_large_payload() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "large_payload");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-large".into(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert_eq!(events.len(), 1);
    if let AgentEventKind::AssistantMessage { text } = &events[0].kind {
        assert_eq!(text.len(), 100_000);
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn run_graceful_exit() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "graceful_exit");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run(Uuid::new_v4().to_string(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert_eq!(events.len(), 2);
    let receipt = run.receipt.await.unwrap().unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn run_tool_call_events() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "tool_call_events");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-tools".into(), test_work_order())
        .await
        .unwrap();
    let events: Vec<_> = run.events.collect().await;
    assert_eq!(events.len(), 4); // run_started, tool_call, tool_result, run_completed
    assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file")));
    assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::ToolResult { tool_name, .. } if tool_name == "read_file")));
}

#[tokio::test]
async fn run_drop_midstream() {
    let py = match python_cmd() {
        Some(cmd) => cmd,
        None => return,
    };
    let spec = make_spec_with_mode(&py, "drop_midstream");
    let client = SidecarClient::spawn(spec).await.unwrap();
    let run = client
        .run("run-drop".into(), test_work_order())
        .await
        .unwrap();
    let _events: Vec<_> = run.events.collect().await;
    // Receipt should indicate an error (process exited unexpectedly)
    let result = run.receipt.await;
    // Either receipt_tx was sent with an error, or the channel was dropped
    match result {
        Ok(Ok(_)) => panic!("expected error or dropped channel"),
        _ => {} // error or channel dropped — both acceptable
    }
}

// ===========================================================================
// 17. Retry async integration
// ===========================================================================

#[tokio::test]
async fn retry_async_succeeds_first_try() {
    let cfg = RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(100),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let mut call_count = 0u32;
    let result = abp_host::retry::retry_async(
        &cfg,
        || {
            call_count += 1;
            async { Ok::<_, HostError>(42) }
        },
        is_retryable,
    )
    .await;
    let outcome = result.unwrap();
    assert_eq!(outcome.value, 42);
    assert_eq!(outcome.metadata.total_attempts, 1);
    assert!(outcome.metadata.failed_attempts.is_empty());
}

#[tokio::test]
async fn retry_async_non_retryable_fails_immediately() {
    let cfg = RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(100),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let result = abp_host::retry::retry_async::<i32, _, _>(
        &cfg,
        || async { Err::<i32, _>(HostError::Violation("bad".into())) },
        is_retryable,
    )
    .await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Violation(_)));
}

#[tokio::test]
async fn retry_async_retries_on_spawn_error() {
    let cfg = RetryConfig {
        max_retries: 2,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(50),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };
    let attempt = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let attempt_clone = attempt.clone();
    let result = abp_host::retry::retry_async(
        &cfg,
        move || {
            let n = attempt_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err(HostError::Spawn(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "not found",
                    )))
                } else {
                    Ok(99)
                }
            }
        },
        is_retryable,
    )
    .await;
    let outcome = result.unwrap();
    assert_eq!(outcome.value, 99);
    assert_eq!(outcome.metadata.total_attempts, 3);
    assert_eq!(outcome.metadata.failed_attempts.len(), 2);
}
