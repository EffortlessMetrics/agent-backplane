// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the process management utilities.

use abp_host::process::{ProcessConfig, ProcessInfo, ProcessStatus};
use abp_host::SidecarSpec;
use chrono::Utc;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

// ── ProcessConfig defaults ──────────────────────────────────────────

#[test]
fn process_config_defaults() {
    let cfg = ProcessConfig::default();
    assert!(cfg.working_dir.is_none());
    assert!(cfg.env_vars.is_empty());
    assert!(cfg.timeout.is_none());
    assert!(cfg.inherit_env);
}

#[test]
fn process_config_custom_env() {
    let mut env = BTreeMap::new();
    env.insert("FOO".into(), "bar".into());
    env.insert("BAZ".into(), "qux".into());

    let cfg = ProcessConfig {
        env_vars: env.clone(),
        ..Default::default()
    };

    assert_eq!(cfg.env_vars.len(), 2);
    assert_eq!(cfg.env_vars["FOO"], "bar");
    assert_eq!(cfg.env_vars["BAZ"], "qux");
}

#[test]
fn process_config_inherit_env_false() {
    let cfg = ProcessConfig {
        inherit_env: false,
        ..Default::default()
    };
    assert!(!cfg.inherit_env);
}

// ── ProcessConfig serde ─────────────────────────────────────────────

#[test]
fn process_config_serde_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("KEY".into(), "VALUE".into());

    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/tmp/work")),
        env_vars: env,
        timeout: Some(Duration::from_secs(30)),
        inherit_env: false,
    };

    let json = serde_json::to_string(&cfg).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(back.working_dir, cfg.working_dir);
    assert_eq!(back.env_vars, cfg.env_vars);
    assert_eq!(back.timeout, cfg.timeout);
    assert_eq!(back.inherit_env, cfg.inherit_env);
}

#[test]
fn process_config_serde_default_timeout_omitted() {
    let cfg = ProcessConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("timeout"));
}

// ── ProcessStatus ───────────────────────────────────────────────────

#[test]
fn process_status_not_started() {
    let s = ProcessStatus::NotStarted;
    assert_eq!(s, ProcessStatus::NotStarted);
}

#[test]
fn process_status_transitions() {
    let statuses = [
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 1234 },
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];

    // All variants are distinct.
    for (i, a) in statuses.iter().enumerate() {
        for (j, b) in statuses.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

#[test]
fn process_status_serde_roundtrip() {
    let cases = vec![
        ProcessStatus::NotStarted,
        ProcessStatus::Running { pid: 42 },
        ProcessStatus::Exited { code: -1 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ];

    for status in &cases {
        let json = serde_json::to_string(status).unwrap();
        let back: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, status);
    }
}

// ── ProcessInfo lifecycle ───────────────────────────────────────────

#[test]
fn process_info_new_is_not_started() {
    let spec = SidecarSpec::new("echo");
    let info = ProcessInfo::new(spec, ProcessConfig::default());

    assert_eq!(info.status, ProcessStatus::NotStarted);
    assert!(info.started_at.is_none());
    assert!(info.ended_at.is_none());
    assert!(!info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_running_state() {
    let spec = SidecarSpec::new("node");
    let mut info = ProcessInfo::new(spec, ProcessConfig::default());

    info.status = ProcessStatus::Running { pid: 9999 };
    info.started_at = Some(Utc::now());

    assert!(info.is_running());
    assert!(!info.is_terminated());
}

#[test]
fn process_info_terminated_states() {
    let spec = SidecarSpec::new("python");

    for terminal in [
        ProcessStatus::Exited { code: 0 },
        ProcessStatus::Killed,
        ProcessStatus::TimedOut,
    ] {
        let mut info = ProcessInfo::new(spec.clone(), ProcessConfig::default());
        info.status = terminal;
        info.ended_at = Some(Utc::now());

        assert!(!info.is_running());
        assert!(info.is_terminated());
    }
}

#[test]
fn process_info_serde_roundtrip() {
    let mut spec = SidecarSpec::new("node");
    spec.args = vec!["host.js".into()];

    let mut env = BTreeMap::new();
    env.insert("TOKEN".into(), "secret".into());

    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/workspace")),
        env_vars: env,
        timeout: Some(Duration::from_secs(60)),
        inherit_env: true,
    };

    let now = Utc::now();
    let info = ProcessInfo {
        spec,
        config: cfg,
        status: ProcessStatus::Running { pid: 5678 },
        started_at: Some(now),
        ended_at: None,
    };

    let json = serde_json::to_string_pretty(&info).unwrap();
    let back: ProcessInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(back.status, info.status);
    assert_eq!(back.spec.command, "node");
    assert_eq!(back.config.timeout, Some(Duration::from_secs(60)));
    assert!(back.started_at.is_some());
    assert!(back.ended_at.is_none());
}

// ── Timeout configuration ───────────────────────────────────────────

#[test]
fn timeout_serialises_as_millis() {
    let cfg = ProcessConfig {
        timeout: Some(Duration::from_millis(1500)),
        ..Default::default()
    };

    let json = serde_json::to_string(&cfg).unwrap();
    // Duration of 1500ms should serialize as the integer 1500.
    assert!(json.contains("1500"), "expected millis value in json: {json}");

    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.timeout, Some(Duration::from_millis(1500)));
}

// ── Working directory ───────────────────────────────────────────────

#[test]
fn working_dir_preserves_path() {
    let cfg = ProcessConfig {
        working_dir: Some(PathBuf::from("/some/deep/path")),
        ..Default::default()
    };

    let json = serde_json::to_string(&cfg).unwrap();
    let back: ProcessConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.working_dir.as_deref(),
        Some(std::path::Path::new("/some/deep/path"))
    );
}
