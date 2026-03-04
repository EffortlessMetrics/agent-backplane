#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-host crate covering:
//! - SidecarSpec construction & serialization
//! - SidecarHello construction & serialization
//! - HostError variants & Display
//! - ProcessConfig, ProcessStatus, ProcessInfo
//! - HealthStatus, HealthCheck, HealthMonitor, HealthReport
//! - LifecycleState, LifecycleManager, LifecycleError
//! - SidecarPool, PoolConfig, PoolStats
//! - SidecarRegistry, SidecarConfig
//! - RetryConfig, RetryMetadata, RetryAttempt, compute_delay, is_retryable
//! - JSONL handshake protocol (encode/decode)
//! - Edge cases & error handling

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RunMetadata, RuntimeConfig, UsageNormalized, VerificationReport, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use std::time::Duration;
use uuid::Uuid;

use abp_host::health::{HealthMonitor, HealthReport, HealthStatus};
use abp_host::lifecycle::{LifecycleError, LifecycleManager, LifecycleState};
use abp_host::pool::{PoolConfig, PoolEntryState, PoolStats, SidecarPool};
use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::registry::{SidecarConfig, SidecarRegistry};
use abp_host::retry::{RetryAttempt, RetryConfig, RetryMetadata, compute_delay, is_retryable};
use abp_host::{HostError, SidecarHello, SidecarSpec};

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    }
}

fn test_receipt(run_id: Uuid, wo_id: Uuid) -> Receipt {
    let now = Utc::now();
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

fn make_event(ref_id: &str, kind: AgentEventKind) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        },
    }
}

fn make_hello_json(backend_id: &str) -> String {
    format!(
        r#"{{"t":"hello","contract_version":"{}","backend":{{"id":"{}","backend_version":null,"adapter_version":null}},"capabilities":{{}}}}"#,
        CONTRACT_VERSION, backend_id
    )
}

// ═══════════════════════════════════════════════════════════════════════
// 1. SidecarSpec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_spec_new_defaults() {
    let spec = SidecarSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_spec_new_from_string() {
    let cmd = String::from("python3");
    let spec = SidecarSpec::new(cmd);
    assert_eq!(spec.command, "python3");
}

#[test]
fn sidecar_spec_with_args_and_env() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["host.js".into(), "--debug".into()];
    spec.env.insert("NODE_ENV".into(), "production".into());
    spec.cwd = Some("/tmp/work".into());

    assert_eq!(spec.args.len(), 2);
    assert_eq!(spec.env.get("NODE_ENV").unwrap(), "production");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn sidecar_spec_serialize_roundtrip() {
    let mut spec = SidecarSpec::new("bash");
    spec.args = vec!["-c".into(), "echo hello".into()];
    spec.env.insert("FOO".into(), "bar".into());
    spec.cwd = Some("/home/user".into());

    let json = serde_json::to_string(&spec).unwrap();
    let deserialized: SidecarSpec = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.command, "bash");
    assert_eq!(deserialized.args, vec!["-c", "echo hello"]);
    assert_eq!(deserialized.env.get("FOO").unwrap(), "bar");
    assert_eq!(deserialized.cwd, Some("/home/user".into()));
}

#[test]
fn sidecar_spec_clone() {
    let spec = SidecarSpec::new("node");
    let cloned = spec.clone();
    assert_eq!(cloned.command, spec.command);
}

#[test]
fn sidecar_spec_debug_format() {
    let spec = SidecarSpec::new("test");
    let debug = format!("{:?}", spec);
    assert!(debug.contains("test"));
}

#[test]
fn sidecar_spec_empty_command() {
    let spec = SidecarSpec::new("");
    assert_eq!(spec.command, "");
}

#[test]
fn sidecar_spec_deterministic_env_ordering() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("Z_VAR".into(), "z".into());
    spec.env.insert("A_VAR".into(), "a".into());
    spec.env.insert("M_VAR".into(), "m".into());

    let keys: Vec<&String> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A_VAR", "M_VAR", "Z_VAR"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. SidecarHello
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_hello_construction() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
    };
    assert_eq!(hello.contract_version, CONTRACT_VERSION);
    assert_eq!(hello.backend.id, "test-backend");
    assert!(hello.capabilities.is_empty());
}

#[test]
fn sidecar_hello_serialize_roundtrip() {
    let hello = SidecarHello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
    };

    let json = serde_json::to_string(&hello).unwrap();
    let deserialized: SidecarHello = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.contract_version, "abp/v0.1");
    assert_eq!(deserialized.backend.id, "mock");
}

#[test]
fn sidecar_hello_with_capabilities() {
    use abp_core::{Capability, SupportLevel};
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);

    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: caps,
    };
    assert_eq!(hello.capabilities.len(), 2);
}

#[test]
fn sidecar_hello_clone() {
    let hello = SidecarHello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
    };
    let cloned = hello.clone();
    assert_eq!(cloned.contract_version, hello.contract_version);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. HostError variants & Display
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn host_error_spawn_display() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    let msg = format!("{}", err);
    assert!(msg.contains("spawn"));
    assert!(msg.contains("not found"));
}

#[test]
fn host_error_stdout_display() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe broken",
    ));
    assert!(format!("{}", err).contains("stdout"));
}

#[test]
fn host_error_stdin_display() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "pipe broken",
    ));
    assert!(format!("{}", err).contains("stdin"));
}

#[test]
fn host_error_protocol_display() {
    let proto_err = ProtocolError::Violation("bad frame".into());
    let err = HostError::Protocol(proto_err);
    assert!(format!("{}", err).contains("protocol"));
}

#[test]
fn host_error_violation_display() {
    let err = HostError::Violation("unexpected hello".into());
    let msg = format!("{}", err);
    assert!(msg.contains("protocol violation"));
    assert!(msg.contains("unexpected hello"));
}

#[test]
fn host_error_fatal_display() {
    let err = HostError::Fatal("out of memory".into());
    assert!(format!("{}", err).contains("out of memory"));
}

#[test]
fn host_error_exited_display() {
    let err = HostError::Exited { code: Some(1) };
    let msg = format!("{}", err);
    assert!(msg.contains("exited"));
    assert!(msg.contains("1"));
}

#[test]
fn host_error_exited_none_code() {
    let err = HostError::Exited { code: None };
    assert!(format!("{}", err).contains("None"));
}

#[test]
fn host_error_sidecar_crashed_display() {
    let err = HostError::SidecarCrashed {
        exit_code: Some(139),
        stderr: "segfault".into(),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("crashed"));
    assert!(msg.contains("139"));
    assert!(msg.contains("segfault"));
}

#[test]
fn host_error_timeout_display() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("timed out"));
    assert!(msg.contains("30"));
}

#[test]
fn host_error_debug_format() {
    let err = HostError::Fatal("test".into());
    let debug = format!("{:?}", err);
    assert!(debug.contains("Fatal"));
}

#[test]
fn host_error_protocol_from_conversion() {
    let proto_err = ProtocolError::Violation("test".into());
    let host_err: HostError = proto_err.into();
    assert!(matches!(host_err, HostError::Protocol(_)));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. ProcessConfig
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_config_default() {
    let cfg = ProcessConfig::default();
    assert!(cfg.working_dir.is_none());
    assert!(cfg.env_vars.is_empty());
    assert!(cfg.timeout.is_none());
    assert!(cfg.inherit_env);
}

#[test]
fn process_config_serialize_roundtrip() {
    let mut cfg = ProcessConfig::default();
    cfg.working_dir = Some("/tmp".into());
    cfg.timeout = Some(Duration::from_millis(5000));
    cfg.inherit_env = false;
    cfg.env_vars.insert("KEY".into(), "VAL".into());

    let json = serde_json::to_string(&cfg).unwrap();
    let deserialized: ProcessConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.working_dir.unwrap().to_str().unwrap(), "/tmp");
    assert_eq!(deserialized.timeout.unwrap(), Duration::from_millis(5000));
    assert!(!deserialized.inherit_env);
    assert_eq!(deserialized.env_vars.get("KEY").unwrap(), "VAL");
}

#[test]
fn process_config_timeout_none_skipped_in_json() {
    let cfg = ProcessConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("timeout"));
}

#[test]
fn process_config_timeout_present_in_json() {
    let mut cfg = ProcessConfig::default();
    cfg.timeout = Some(Duration::from_secs(10));
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("timeout"));
    assert!(json.contains("10000")); // 10s = 10000ms
}

// ═══════════════════════════════════════════════════════════════════════
// 5. ProcessStatus
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_status_not_started_serialize() {
    let status = ProcessStatus::NotStarted;
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("not_started"));
}

#[test]
fn process_status_running_serialize() {
    let status = ProcessStatus::Running { pid: 12345 };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("running"));
    assert!(json.contains("12345"));
}

#[test]
fn process_status_exited_serialize() {
    let status = ProcessStatus::Exited { code: 0 };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("exited"));
}

#[test]
fn process_status_killed_serialize() {
    let json = serde_json::to_string(&ProcessStatus::Killed).unwrap();
    assert!(json.contains("killed"));
}

#[test]
fn process_status_timed_out_serialize() {
    let json = serde_json::to_string(&ProcessStatus::TimedOut).unwrap();
    assert!(json.contains("timed_out"));
}

#[test]
fn process_status_roundtrip_all_variants() {
    let variants = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 1 },
        ProcessStatus::Exited { code: 42 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let parsed: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, v);
    }
}

#[test]
fn process_status_equality() {
    assert_eq!(ProcessStatus::NotStarted, ProcessStatus::NotStarted);
    assert_ne!(
        ProcessStatus::Running { pid: 1 },
        ProcessStatus::Running { pid: 2 }
    );
    assert_eq!(
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Exited { code: 0 }
    );
    assert_ne!(ProcessStatus::Killed, ProcessStatus::TimedOut);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. ProcessInfo
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn process_info_new_defaults() {
    let spec = SidecarSpec::new("test");
    let config = ProcessConfig::default();
    let info = ProcessInfo::new(spec, config);

    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
    assert!(!info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_is_running() {
    let mut info = ProcessInfo::new(SidecarSpec::new("test"), ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 100 };
    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_is_terminated_exited() {
    let mut info = ProcessInfo::new(SidecarSpec::new("test"), ProcessConfig::default());
    info.status = ProcessStatus::Exited { code: 0 };
    assert!(info.is_terminated());
    assert!(!info.is_running());
}

#[test]
fn process_info_is_terminated_killed() {
    let mut info = ProcessInfo::new(SidecarSpec::new("test"), ProcessConfig::default());
    info.status = ProcessStatus::Killed;
    assert!(info.is_terminated());
}

#[test]
fn process_info_is_terminated_timed_out() {
    let mut info = ProcessInfo::new(SidecarSpec::new("test"), ProcessConfig::default());
    info.status = ProcessStatus::TimedOut;
    assert!(info.is_terminated());
}

#[test]
fn process_info_not_started_is_not_terminated() {
    let info = ProcessInfo::new(SidecarSpec::new("test"), ProcessConfig::default());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_serialize_roundtrip() {
    let mut info = ProcessInfo::new(SidecarSpec::new("node"), ProcessConfig::default());
    info.status = ProcessStatus::Running { pid: 999 };
    info.started_at = Some(Utc::now());

    let json = serde_json::to_string(&info).unwrap();
    let deserialized: ProcessInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.spec.command, "node");
    assert!(matches!(
        deserialized.status,
        ProcessStatus::Running { pid: 999 }
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. HealthStatus
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_status_healthy_serialize() {
    let json = serde_json::to_string(&HealthStatus::Healthy).unwrap();
    assert!(json.contains("healthy"));
}

#[test]
fn health_status_degraded_serialize() {
    let status = HealthStatus::Degraded {
        reason: "high latency".into(),
    };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("degraded"));
    assert!(json.contains("high latency"));
}

#[test]
fn health_status_unhealthy_serialize() {
    let status = HealthStatus::Unhealthy {
        reason: "connection refused".into(),
    };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("unhealthy"));
}

#[test]
fn health_status_unknown_serialize() {
    let json = serde_json::to_string(&HealthStatus::Unknown).unwrap();
    assert!(json.contains("unknown"));
}

#[test]
fn health_status_equality() {
    assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
    assert_eq!(HealthStatus::Unknown, HealthStatus::Unknown);
    assert_ne!(HealthStatus::Healthy, HealthStatus::Unknown);
}

#[test]
fn health_status_roundtrip_all_variants() {
    let variants = vec![
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        HealthStatus::Unknown,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let parsed: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, v);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. HealthMonitor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_monitor_new_is_empty() {
    let monitor = HealthMonitor::new();
    assert_eq!(monitor.total_checks(), 0);
    assert!(!monitor.all_healthy());
}

#[test]
fn health_monitor_default_is_empty() {
    let monitor = HealthMonitor::default();
    assert_eq!(monitor.total_checks(), 0);
}

#[test]
fn health_monitor_record_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "sidecar-a",
        HealthStatus::Healthy,
        Some(Duration::from_millis(5)),
    );
    assert_eq!(monitor.total_checks(), 1);
    assert!(monitor.all_healthy());

    let check = monitor.get_status("sidecar-a").unwrap();
    assert_eq!(check.name, "sidecar-a");
    assert_eq!(check.consecutive_failures, 0);
    assert!(check.response_time.is_some());
}

#[test]
fn health_monitor_record_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check(
        "bad-sidecar",
        HealthStatus::Unhealthy {
            reason: "crash".into(),
        },
        None,
    );
    assert!(!monitor.all_healthy());

    let check = monitor.get_status("bad-sidecar").unwrap();
    assert_eq!(check.consecutive_failures, 1);
}

#[test]
fn health_monitor_consecutive_failures_increment() {
    let mut monitor = HealthMonitor::new();
    for _ in 0..5 {
        monitor.record_check(
            "flaky",
            HealthStatus::Unhealthy {
                reason: "fail".into(),
            },
            None,
        );
    }
    let check = monitor.get_status("flaky").unwrap();
    assert_eq!(check.consecutive_failures, 5);
}

#[test]
fn health_monitor_consecutive_failures_reset_on_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s", HealthStatus::Unhealthy { reason: "x".into() }, None);
    monitor.record_check("s", HealthStatus::Unhealthy { reason: "x".into() }, None);
    assert_eq!(monitor.get_status("s").unwrap().consecutive_failures, 2);

    monitor.record_check("s", HealthStatus::Healthy, None);
    assert_eq!(monitor.get_status("s").unwrap().consecutive_failures, 0);
}

#[test]
fn health_monitor_get_status_unknown_returns_none() {
    let monitor = HealthMonitor::new();
    assert!(monitor.get_status("nonexistent").is_none());
}

#[test]
fn health_monitor_unhealthy_sidecars() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("ok", HealthStatus::Healthy, None);
    monitor.record_check("bad1", HealthStatus::Unhealthy { reason: "a".into() }, None);
    monitor.record_check("bad2", HealthStatus::Unhealthy { reason: "b".into() }, None);
    monitor.record_check(
        "degraded",
        HealthStatus::Degraded { reason: "c".into() },
        None,
    );

    let unhealthy = monitor.unhealthy_sidecars();
    assert_eq!(unhealthy.len(), 2);
}

#[test]
fn health_monitor_uptime_percentage_no_history() {
    let monitor = HealthMonitor::new();
    assert_eq!(monitor.uptime_percentage("nonexistent"), 0.0);
}

#[test]
fn health_monitor_uptime_percentage_all_healthy() {
    let mut monitor = HealthMonitor::new();
    for _ in 0..10 {
        monitor.record_check("s", HealthStatus::Healthy, None);
    }
    assert!((monitor.uptime_percentage("s") - 100.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_uptime_percentage_mixed() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s", HealthStatus::Healthy, None);
    monitor.record_check("s", HealthStatus::Unhealthy { reason: "x".into() }, None);
    monitor.record_check("s", HealthStatus::Healthy, None);
    monitor.record_check("s", HealthStatus::Healthy, None);
    // 3 healthy out of 4 = 75%
    assert!((monitor.uptime_percentage("s") - 75.0).abs() < f64::EPSILON);
}

#[test]
fn health_monitor_generate_report_empty() {
    let monitor = HealthMonitor::new();
    let report = monitor.generate_report();
    assert!(report.checks.is_empty());
    assert_eq!(report.overall, HealthStatus::Unknown);
}

#[test]
fn health_monitor_generate_report_all_healthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("a", HealthStatus::Healthy, None);
    monitor.record_check("b", HealthStatus::Healthy, None);
    let report = monitor.generate_report();
    assert_eq!(report.overall, HealthStatus::Healthy);
    assert_eq!(report.checks.len(), 2);
}

#[test]
fn health_monitor_report_unhealthy_overrides() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("a", HealthStatus::Healthy, None);
    monitor.record_check(
        "b",
        HealthStatus::Unhealthy {
            reason: "bad".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn health_monitor_report_degraded_when_no_unhealthy() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("a", HealthStatus::Healthy, None);
    monitor.record_check(
        "b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    let report = monitor.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));
}

#[test]
fn health_monitor_report_unknown_propagates() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("a", HealthStatus::Healthy, None);
    monitor.record_check("b", HealthStatus::Unknown, None);
    let report = monitor.generate_report();
    assert_eq!(report.overall, HealthStatus::Unknown);
}

#[test]
fn health_report_serialize_roundtrip() {
    let mut monitor = HealthMonitor::new();
    monitor.record_check("s", HealthStatus::Healthy, Some(Duration::from_millis(42)));
    let report = monitor.generate_report();

    let json = serde_json::to_string(&report).unwrap();
    let deserialized: HealthReport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.checks.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. LifecycleState & LifecycleManager
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lifecycle_state_display() {
    assert_eq!(LifecycleState::Uninitialized.to_string(), "uninitialized");
    assert_eq!(LifecycleState::Starting.to_string(), "starting");
    assert_eq!(LifecycleState::Ready.to_string(), "ready");
    assert_eq!(LifecycleState::Running.to_string(), "running");
    assert_eq!(LifecycleState::Stopping.to_string(), "stopping");
    assert_eq!(LifecycleState::Stopped.to_string(), "stopped");
    assert_eq!(LifecycleState::Failed.to_string(), "failed");
}

#[test]
fn lifecycle_state_serialize_roundtrip() {
    let states = vec![
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
        LifecycleState::Stopped,
        LifecycleState::Failed,
    ];
    for state in &states {
        let json = serde_json::to_string(state).unwrap();
        let parsed: LifecycleState = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, state);
    }
}

#[test]
fn lifecycle_manager_new_starts_uninitialized() {
    let mgr = LifecycleManager::new();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
    assert!(mgr.history().is_empty());
    assert!(mgr.uptime().is_none());
}

#[test]
fn lifecycle_manager_default_starts_uninitialized() {
    let mgr = LifecycleManager::default();
    assert_eq!(*mgr.state(), LifecycleState::Uninitialized);
}

#[test]
fn lifecycle_manager_valid_full_path() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();

    assert_eq!(*mgr.state(), LifecycleState::Stopped);
    assert_eq!(mgr.history().len(), 5);
}

#[test]
fn lifecycle_manager_ready_to_running_and_back() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Running, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    assert_eq!(*mgr.state(), LifecycleState::Ready);
}

#[test]
fn lifecycle_manager_failed_from_any_state() {
    let states = vec![
        LifecycleState::Uninitialized,
        LifecycleState::Starting,
        LifecycleState::Ready,
        LifecycleState::Running,
        LifecycleState::Stopping,
    ];
    for state in states {
        let mut mgr = LifecycleManager::new();
        // Walk to the target state
        match state {
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
        assert!(mgr.transition(LifecycleState::Failed, None).is_ok());
        assert_eq!(*mgr.state(), LifecycleState::Failed);
    }
}

#[test]
fn lifecycle_manager_invalid_transition_error() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Running, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        LifecycleError::InvalidTransition { from, to } => {
            assert_eq!(from, LifecycleState::Uninitialized);
            assert_eq!(to, LifecycleState::Running);
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn lifecycle_manager_already_in_state_error() {
    let mut mgr = LifecycleManager::new();
    let result = mgr.transition(LifecycleState::Uninitialized, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        LifecycleError::AlreadyInState(s) => {
            assert_eq!(s, LifecycleState::Uninitialized);
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

#[test]
fn lifecycle_error_display() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Ready,
        to: LifecycleState::Uninitialized,
    };
    let msg = format!("{}", err);
    assert!(msg.contains("invalid lifecycle transition"));
    assert!(msg.contains("ready"));
    assert!(msg.contains("uninitialized"));

    let err2 = LifecycleError::AlreadyInState(LifecycleState::Running);
    assert!(format!("{}", err2).contains("already in state"));
}

#[test]
fn lifecycle_manager_transition_with_reason() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, Some("boot initiated".into()))
        .unwrap();
    let entry = &mgr.history()[0];
    assert_eq!(entry.from, LifecycleState::Uninitialized);
    assert_eq!(entry.to, LifecycleState::Starting);
    assert_eq!(entry.reason.as_deref(), Some("boot initiated"));
    assert!(!entry.timestamp.is_empty());
}

#[test]
fn lifecycle_manager_uptime_after_ready() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    let uptime = mgr.uptime();
    assert!(uptime.is_some());
}

#[test]
fn lifecycle_manager_can_transition_checks() {
    let mgr = LifecycleManager::new();
    assert!(mgr.can_transition(&LifecycleState::Starting));
    assert!(!mgr.can_transition(&LifecycleState::Ready));
    assert!(!mgr.can_transition(&LifecycleState::Running));
    assert!(mgr.can_transition(&LifecycleState::Failed)); // always allowed
}

#[test]
fn lifecycle_manager_stopped_to_ready_invalid() {
    let mut mgr = LifecycleManager::new();
    mgr.transition(LifecycleState::Starting, None).unwrap();
    mgr.transition(LifecycleState::Ready, None).unwrap();
    mgr.transition(LifecycleState::Stopping, None).unwrap();
    mgr.transition(LifecycleState::Stopped, None).unwrap();

    let result = mgr.transition(LifecycleState::Ready, None);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. PoolConfig & SidecarPool
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pool_config_default() {
    let cfg = PoolConfig::default();
    assert_eq!(cfg.min_size, 1);
    assert_eq!(cfg.max_size, 4);
    assert_eq!(cfg.idle_timeout, Duration::from_secs(300));
    assert_eq!(cfg.health_check_interval, Duration::from_secs(30));
}

#[test]
fn pool_config_serialize_roundtrip() {
    let cfg = PoolConfig {
        min_size: 2,
        max_size: 8,
        idle_timeout: Duration::from_secs(60),
        health_check_interval: Duration::from_secs(15),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let deserialized: PoolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.min_size, 2);
    assert_eq!(deserialized.max_size, 8);
    assert_eq!(deserialized.idle_timeout, Duration::from_secs(60));
}

#[test]
fn pool_new_empty() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn pool_add_entry() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.add("s1"));
    assert_eq!(pool.total_count(), 1);
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_add_respects_max_size() {
    let cfg = PoolConfig {
        max_size: 2,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(cfg);
    assert!(pool.add("s1"));
    assert!(pool.add("s2"));
    assert!(!pool.add("s3"));
    assert_eq!(pool.total_count(), 2);
}

#[test]
fn pool_acquire_and_release() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");

    let entry = pool.acquire().unwrap();
    assert_eq!(entry.id, "s1");
    assert_eq!(entry.state, PoolEntryState::Busy);
    assert_eq!(pool.idle_count(), 0);

    pool.release("s1");
    assert_eq!(pool.idle_count(), 1);
}

#[test]
fn pool_acquire_returns_none_when_no_idle() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.acquire().unwrap(); // now busy
    assert!(pool.acquire().is_none());
}

#[test]
fn pool_acquire_empty_pool_returns_none() {
    let pool = SidecarPool::new(PoolConfig::default());
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
fn pool_remove_nonexistent() {
    let pool = SidecarPool::new(PoolConfig::default());
    assert!(pool.remove("nonexistent").is_none());
}

#[test]
fn pool_stats() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire(); // s1 → busy
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
fn pool_stats_utilization() {
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
fn pool_stats_utilization_empty() {
    let stats = PoolStats {
        total: 0,
        idle: 0,
        busy: 0,
        draining: 0,
        failed: 0,
    };
    assert_eq!(stats.utilization(), 0.0);
}

#[test]
fn pool_stats_serialize_roundtrip() {
    let stats = PoolStats {
        total: 5,
        idle: 2,
        busy: 1,
        draining: 1,
        failed: 1,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let deserialized: PoolStats = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, stats);
}

#[test]
fn pool_active_count() {
    let pool = SidecarPool::new(PoolConfig::default());
    pool.add("s1");
    pool.add("s2");
    pool.add("s3");
    pool.acquire(); // one busy
    pool.mark_failed("s3");

    // active = idle + busy, excludes failed/draining
    assert_eq!(pool.active_count(), 2);
}

#[test]
fn pool_config_accessor() {
    let cfg = PoolConfig {
        min_size: 3,
        max_size: 10,
        ..PoolConfig::default()
    };
    let pool = SidecarPool::new(cfg);
    assert_eq!(pool.config().min_size, 3);
    assert_eq!(pool.config().max_size, 10);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. SidecarRegistry & SidecarConfig
// ═══════════════════════════════════════════════════════════════════════

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
    let cfg = SidecarConfig::new("test", "bash");
    assert!(cfg.validate().is_ok());
}

#[test]
fn sidecar_config_validate_empty_name() {
    let cfg = SidecarConfig::new("", "bash");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_validate_empty_command() {
    let cfg = SidecarConfig::new("test", "");
    assert!(cfg.validate().is_err());
}

#[test]
fn sidecar_config_to_spec() {
    let mut cfg = SidecarConfig::new("test", "node");
    cfg.args = vec!["host.js".into()];
    cfg.env.insert("FOO".into(), "bar".into());
    cfg.working_dir = Some("/tmp".into());

    let spec = cfg.to_spec();
    assert_eq!(spec.command, "node");
    assert_eq!(spec.args, vec!["host.js"]);
    assert_eq!(spec.env.get("FOO").unwrap(), "bar");
    assert_eq!(spec.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn sidecar_config_to_spec_no_cwd() {
    let cfg = SidecarConfig::new("test", "python");
    let spec = cfg.to_spec();
    assert!(spec.cwd.is_none());
}

#[test]
fn sidecar_config_serialize_roundtrip() {
    let mut cfg = SidecarConfig::new("sidecar", "node");
    cfg.args = vec!["main.js".into()];
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: SidecarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "sidecar");
    assert_eq!(parsed.command, "node");
}

#[test]
fn registry_default_empty() {
    let reg = SidecarRegistry::default();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();

    let cfg = reg.get("node").unwrap();
    assert_eq!(cfg.command, "node");
}

#[test]
fn registry_register_duplicate_fails() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    let result = reg.register(SidecarConfig::new("node", "node"));
    assert!(result.is_err());
}

#[test]
fn registry_register_invalid_config_fails() {
    let mut reg = SidecarRegistry::default();
    let result = reg.register(SidecarConfig::new("", "bash"));
    assert!(result.is_err());
}

#[test]
fn registry_list_sorted() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("zulu", "z")).unwrap();
    reg.register(SidecarConfig::new("alpha", "a")).unwrap();
    reg.register(SidecarConfig::new("mike", "m")).unwrap();

    let names = reg.list();
    assert_eq!(names, vec!["alpha", "mike", "zulu"]);
}

#[test]
fn registry_remove() {
    let mut reg = SidecarRegistry::default();
    reg.register(SidecarConfig::new("node", "node")).unwrap();
    assert!(reg.remove("node"));
    assert!(reg.get("node").is_none());
    assert!(!reg.remove("node")); // already removed
}

#[test]
fn registry_get_nonexistent() {
    let reg = SidecarRegistry::default();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_from_config_dir_with_temp() {
    let dir = tempfile::tempdir().unwrap();

    // Create subdirectory with host.js
    let node_dir = dir.path().join("my-node-sidecar");
    std::fs::create_dir(&node_dir).unwrap();
    std::fs::write(node_dir.join("host.js"), "// sidecar").unwrap();

    // Create subdirectory with host.py
    let py_dir = dir.path().join("my-python-sidecar");
    std::fs::create_dir(&py_dir).unwrap();
    std::fs::write(py_dir.join("host.py"), "# sidecar").unwrap();

    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    let names = reg.list();
    assert!(names.contains(&"my-node-sidecar"));
    assert!(names.contains(&"my-python-sidecar"));

    let node_cfg = reg.get("my-node-sidecar").unwrap();
    assert_eq!(node_cfg.command, "node");

    let py_cfg = reg.get("my-python-sidecar").unwrap();
    assert_eq!(py_cfg.command, "python");
}

#[test]
fn registry_discover_from_dir_is_alias() {
    let dir = tempfile::tempdir().unwrap();
    let reg = SidecarRegistry::discover_from_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_from_config_dir_ignores_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("not-a-dir.txt"), "data").unwrap();
    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

#[test]
fn registry_from_config_dir_ignores_dirs_without_known_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("unknown-sidecar");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("main.rs"), "fn main() {}").unwrap();

    let reg = SidecarRegistry::from_config_dir(dir.path()).unwrap();
    assert!(reg.list().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. RetryConfig
// ═══════════════════════════════════════════════════════════════════════

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
fn retry_config_serialize_roundtrip() {
    let cfg = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(30),
        overall_timeout: Duration::from_secs(120),
        jitter_factor: 0.25,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let deserialized: RetryConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.max_retries, 5);
    assert_eq!(deserialized.base_delay, Duration::from_millis(200));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. compute_delay
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compute_delay_no_jitter_exponential() {
    let cfg = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        jitter_factor: 0.0,
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
fn compute_delay_respects_max_delay() {
    let cfg = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(500),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };

    let d10 = compute_delay(&cfg, 10);
    assert_eq!(d10, Duration::from_millis(500));
}

#[test]
fn compute_delay_with_jitter_bounded() {
    let cfg = RetryConfig {
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(60),
        jitter_factor: 1.0,
        ..RetryConfig::default()
    };

    for attempt in 0..5 {
        let delay = compute_delay(&cfg, attempt);
        let nominal = Duration::from_millis(1000 * 2u64.pow(attempt));
        // With full jitter, delay should be in [0, nominal]
        assert!(delay <= nominal);
    }
}

#[test]
fn compute_delay_zero_base() {
    let cfg = RetryConfig {
        base_delay: Duration::from_millis(0),
        jitter_factor: 0.0,
        ..RetryConfig::default()
    };
    let d = compute_delay(&cfg, 5);
    assert_eq!(d, Duration::from_millis(0));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. is_retryable
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn is_retryable_spawn_error() {
    let err = HostError::Spawn(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_stdout_error() {
    let err = HostError::Stdout(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken",
    ));
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
        exit_code: Some(139),
        stderr: "segfault".into(),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_retryable_timeout_error() {
    let err = HostError::Timeout {
        duration: Duration::from_secs(30),
    };
    assert!(is_retryable(&err));
}

#[test]
fn is_not_retryable_violation() {
    let err = HostError::Violation("bad message".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_fatal() {
    let err = HostError::Fatal("fatal error".into());
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_protocol() {
    let proto_err = ProtocolError::Violation("test".into());
    let err = HostError::Protocol(proto_err);
    assert!(!is_retryable(&err));
}

#[test]
fn is_not_retryable_stdin() {
    let err = HostError::Stdin(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "broken",
    ));
    assert!(!is_retryable(&err));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. RetryMetadata & RetryAttempt
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_metadata_default() {
    let meta = RetryMetadata::default();
    assert_eq!(meta.total_attempts, 0);
    assert!(meta.failed_attempts.is_empty());
    assert_eq!(meta.total_duration, Duration::from_millis(0));
}

#[test]
fn retry_metadata_to_receipt_metadata_empty() {
    let meta = RetryMetadata::default();
    let map = meta.to_receipt_metadata();
    assert!(map.contains_key("retry_total_attempts"));
    assert!(map.contains_key("retry_total_duration_ms"));
    assert!(!map.contains_key("retry_failed_attempts"));
}

#[test]
fn retry_metadata_to_receipt_metadata_with_failures() {
    let meta = RetryMetadata {
        total_attempts: 3,
        failed_attempts: vec![
            RetryAttempt {
                attempt: 0,
                error: "spawn failed".into(),
                delay: Duration::from_millis(100),
            },
            RetryAttempt {
                attempt: 1,
                error: "timeout".into(),
                delay: Duration::from_millis(200),
            },
        ],
        total_duration: Duration::from_millis(500),
    };

    let map = meta.to_receipt_metadata();
    assert_eq!(map["retry_total_attempts"], serde_json::json!(3));
    assert_eq!(map["retry_total_duration_ms"], serde_json::json!(500));
    assert!(map.contains_key("retry_failed_attempts"));

    let attempts = map["retry_failed_attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0]["attempt"], 0);
    assert_eq!(attempts[0]["error"], "spawn failed");
    assert_eq!(attempts[0]["delay_ms"], 100);
}

#[test]
fn retry_metadata_serialize_roundtrip() {
    let meta = RetryMetadata {
        total_attempts: 2,
        failed_attempts: vec![RetryAttempt {
            attempt: 0,
            error: "err".into(),
            delay: Duration::from_millis(50),
        }],
        total_duration: Duration::from_millis(150),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let deserialized: RetryMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_attempts, 2);
    assert_eq!(deserialized.failed_attempts.len(), 1);
}

#[test]
fn retry_attempt_serialize_roundtrip() {
    let attempt = RetryAttempt {
        attempt: 3,
        error: "connection refused".into(),
        delay: Duration::from_millis(800),
    };
    let json = serde_json::to_string(&attempt).unwrap();
    let deserialized: RetryAttempt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.attempt, 3);
    assert_eq!(deserialized.error, "connection refused");
    assert_eq!(deserialized.delay, Duration::from_millis(800));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. JSONL Handshake Protocol — Encoding/Decoding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encode_decode_hello_envelope() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(backend.id, "test-backend");
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn encode_decode_run_envelope() {
    let env = Envelope::Run {
        id: "run-001".into(),
        work_order: test_work_order(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Run { id, work_order } => {
            assert_eq!(id, "run-001");
            assert_eq!(work_order.task, "test task");
        }
        other => panic!("expected Run, got {:?}", other),
    }
}

#[test]
fn encode_decode_event_envelope() {
    let env = make_event(
        "run-001",
        AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-001");
            match event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text, "hello world");
                }
                other => panic!("expected AssistantMessage, got {:?}", other),
            }
        }
        other => panic!("expected Event, got {:?}", other),
    }
}

#[test]
fn encode_decode_final_envelope() {
    let run_id = Uuid::new_v4();
    let wo_id = Uuid::nil();
    let env = Envelope::Final {
        ref_id: "run-001".into(),
        receipt: test_receipt(run_id, wo_id),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-001");
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
        other => panic!("expected Final, got {:?}", other),
    }
}

#[test]
fn encode_decode_fatal_envelope() {
    let env = Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "critical failure".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, error, .. } => {
            assert_eq!(ref_id.as_deref(), Some("run-001"));
            assert_eq!(error, "critical failure");
        }
        other => panic!("expected Fatal, got {:?}", other),
    }
}

#[test]
fn encode_decode_fatal_no_ref_id() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "global failure".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => {
            assert!(ref_id.is_none());
        }
        other => panic!("expected Fatal, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. JSONL Handshake — raw JSON parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_hello_from_raw_json() {
    let line = make_hello_json("my-backend");
    let env = JsonlCodec::decode(&line).unwrap();
    match env {
        Envelope::Hello {
            backend,
            contract_version,
            ..
        } => {
            assert_eq!(backend.id, "my-backend");
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn decode_invalid_json_returns_error() {
    let result = JsonlCodec::decode("not valid json");
    assert!(result.is_err());
}

#[test]
fn decode_empty_object_returns_error() {
    let result = JsonlCodec::decode("{}");
    assert!(result.is_err());
}

#[test]
fn decode_missing_tag_field_returns_error() {
    let result = JsonlCodec::decode(r#"{"type":"hello"}"#);
    assert!(result.is_err()); // tag field is "t", not "type"
}

#[test]
fn decode_unknown_tag_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_envelope_type"}"#);
    assert!(result.is_err());
}

#[test]
fn encoded_line_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn encoded_line_contains_tag_field() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    assert!(encoded.contains(r#""t":"hello""#));
}

#[test]
fn decode_stream_multiple_lines() {
    let hello = make_hello_json("test");
    let fatal = format!(r#"{{"t":"fatal","ref_id":null,"error":"oops"}}"#);
    let input = format!("{}\n{}\n", hello, fatal);

    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], Envelope::Hello { .. }));
    assert!(matches!(envelopes[1], Envelope::Fatal { .. }));
}

#[test]
fn decode_stream_skips_blank_lines() {
    let hello = make_hello_json("test");
    let input = format!("\n\n{}\n\n", hello);

    let reader = std::io::BufReader::new(input.as_bytes());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(envelopes.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Capability Negotiation during Handshake
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hello_with_capabilities_roundtrip() {
    use abp_core::{Capability, SupportLevel};

    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Emulated);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );

    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: caps.clone(),
        mode: ExecutionMode::Mapped,
    };

    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 3);
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolUse),
                Some(SupportLevel::Emulated)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolBash),
                Some(SupportLevel::Restricted { .. })
            ));
        }
        other => panic!("expected Hello, got {:?}", other),
    }
}

#[test]
fn hello_empty_capabilities_is_valid() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert!(capabilities.is_empty());
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_passthrough_mode() {
    let env = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: test_backend(),
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Passthrough,
    };
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Event Streaming — Various AgentEventKind variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_run_started_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::RunStarted {
            message: "starting".into(),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::RunStarted { message } => assert_eq!(message, "starting"),
            other => panic!("unexpected kind: {:?}", other),
        },
        other => panic!("expected Event, got {:?}", other),
    }
}

#[test]
fn event_run_completed_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::RunCompleted { message } => assert_eq!(message, "done"),
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_assistant_delta_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::AssistantDelta {
            text: "partial".into(),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "partial"),
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_call_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"cmd": "ls"}),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_use_id.as_deref(), Some("tu-1"));
                assert_eq!(input["cmd"], "ls");
            }
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_tool_result_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu-1".into()),
            output: serde_json::json!({"stdout": "ok"}),
            is_error: false,
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert!(!is_error);
            }
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_file_changed_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "added function");
            }
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_command_executed_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all passed".into()),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::CommandExecuted {
                command, exit_code, ..
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(exit_code, Some(0));
            }
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_warning_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::Warning {
            message: "deprecated API".into(),
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "deprecated API"),
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

#[test]
fn event_error_roundtrip() {
    let env = make_event(
        "r1",
        AgentEventKind::Error {
            message: "something broke".into(),
            error_code: None,
        },
    );
    let encoded = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
    match decoded {
        Envelope::Event { event, .. } => match event.kind {
            AgentEventKind::Error { message, .. } => assert_eq!(message, "something broke"),
            other => panic!("unexpected kind: {:?}", other),
        },
        _ => panic!("expected Event"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Edge Cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sidecar_spec_unicode_command() {
    let spec = SidecarSpec::new("ünïcödé");
    assert_eq!(spec.command, "ünïcödé");
}

#[test]
fn sidecar_spec_unicode_env_roundtrip() {
    let mut spec = SidecarSpec::new("cmd");
    spec.env.insert("GRÜSS".into(), "wörld".into());
    let json = serde_json::to_string(&spec).unwrap();
    let parsed: SidecarSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.env.get("GRÜSS").unwrap(), "wörld");
}

#[test]
fn host_error_long_violation_message() {
    let long_msg = "x".repeat(10_000);
    let err = HostError::Violation(long_msg.clone());
    let display = format!("{}", err);
    assert!(display.contains(&long_msg));
}

#[test]
fn host_error_empty_fatal_message() {
    let err = HostError::Fatal(String::new());
    let display = format!("{}", err);
    assert!(display.contains("fatal"));
}

#[test]
fn pool_entry_state_idle_is_not_busy() {
    assert_ne!(PoolEntryState::Idle, PoolEntryState::Busy);
    assert_ne!(PoolEntryState::Idle, PoolEntryState::Draining);
    assert_ne!(PoolEntryState::Idle, PoolEntryState::Failed);
}

#[test]
fn lifecycle_error_is_std_error() {
    let err = LifecycleError::InvalidTransition {
        from: LifecycleState::Stopped,
        to: LifecycleState::Running,
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn process_config_large_env_vars() {
    let mut cfg = ProcessConfig::default();
    for i in 0..100 {
        cfg.env_vars
            .insert(format!("VAR_{}", i), format!("val_{}", i));
    }
    assert_eq!(cfg.env_vars.len(), 100);

    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.env_vars.len(), 100);
}

#[test]
fn registry_multiple_register_remove_cycles() {
    let mut reg = SidecarRegistry::default();

    for i in 0..10 {
        let name = format!("sidecar-{}", i);
        reg.register(SidecarConfig::new(&name, "test")).unwrap();
    }
    assert_eq!(reg.list().len(), 10);

    for i in 0..5 {
        reg.remove(&format!("sidecar-{}", i));
    }
    assert_eq!(reg.list().len(), 5);
}

#[test]
fn pool_concurrent_add_is_thread_safe() {
    use std::sync::Arc;
    use std::thread;

    let pool = Arc::new(SidecarPool::new(PoolConfig {
        max_size: 100,
        ..PoolConfig::default()
    }));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let pool = Arc::clone(&pool);
            thread::spawn(move || {
                for j in 0..10 {
                    pool.add(format!("s-{}-{}", i, j));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(pool.total_count(), 100);
}

#[test]
fn retry_config_zero_retries() {
    let cfg = RetryConfig {
        max_retries: 0,
        ..RetryConfig::default()
    };
    assert_eq!(cfg.max_retries, 0);
}

#[test]
fn retry_config_jitter_factor_clamping_in_compute_delay() {
    // jitter_factor > 1.0 should be clamped
    let cfg = RetryConfig {
        base_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(60),
        jitter_factor: 5.0,
        ..RetryConfig::default()
    };
    let delay = compute_delay(&cfg, 0);
    // With clamped jitter_factor=1.0, delay ∈ [0, 1000ms]
    assert!(delay <= Duration::from_millis(1000));
}

#[test]
fn retry_config_negative_jitter_clamped_to_zero() {
    let cfg = RetryConfig {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(60),
        jitter_factor: -1.0,
        ..RetryConfig::default()
    };
    // Clamped to 0, so no jitter applied — delay = base_delay
    let delay = compute_delay(&cfg, 0);
    assert_eq!(delay, Duration::from_millis(100));
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Async retry tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn retry_async_succeeds_first_try() {
    use abp_host::retry::retry_async;

    let cfg = RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };

    let outcome = retry_async(&cfg, || async { Ok::<_, HostError>(42) }, is_retryable)
        .await
        .unwrap();

    assert_eq!(outcome.value, 42);
    assert_eq!(outcome.metadata.total_attempts, 1);
    assert!(outcome.metadata.failed_attempts.is_empty());
}

#[tokio::test]
async fn retry_async_fails_then_succeeds() {
    use abp_host::retry::retry_async;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let cfg = RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };

    let attempt = Arc::new(AtomicU32::new(0));
    let attempt_clone = Arc::clone(&attempt);

    let outcome = retry_async(
        &cfg,
        move || {
            let attempt = Arc::clone(&attempt_clone);
            async move {
                let n = attempt.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(HostError::Exited { code: Some(1) })
                } else {
                    Ok("success")
                }
            }
        },
        is_retryable,
    )
    .await
    .unwrap();

    assert_eq!(outcome.value, "success");
    assert_eq!(outcome.metadata.total_attempts, 3);
    assert_eq!(outcome.metadata.failed_attempts.len(), 2);
}

#[tokio::test]
async fn retry_async_non_retryable_stops_immediately() {
    use abp_host::retry::retry_async;

    let cfg = RetryConfig {
        max_retries: 5,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };

    let result = retry_async(
        &cfg,
        || async { Err::<i32, _>(HostError::Violation("bad protocol".into())) },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Violation(_)));
}

#[tokio::test]
async fn retry_async_exhausts_max_retries() {
    use abp_host::retry::retry_async;

    let cfg = RetryConfig {
        max_retries: 2,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        overall_timeout: Duration::from_secs(5),
        jitter_factor: 0.0,
    };

    let result = retry_async(
        &cfg,
        || async { Err::<i32, _>(HostError::Exited { code: Some(1) }) },
        is_retryable,
    )
    .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HostError::Exited { .. }));
}
