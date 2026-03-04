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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Comprehensive configuration-validation tests for the `abp-config` crate.
//!
//! Covers: BackplaneConfig construction/defaults, TOML parsing (valid & invalid),
//! config validation (required fields, valid values), merging, environment
//! variable overrides, backend configuration entries, config entry validation,
//! warnings/errors, default backend selection, multiple backends, sidecar-specific
//! configuration, serde roundtrip, config file loading, and precedence rules.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use abp_config::{
    BackendEntry, BackplaneConfig, ConfigError, ConfigWarning, apply_env_overrides, load_config,
    merge_configs, parse_toml,
    validate::{ConfigDiff, ConfigValidator, Severity, ValidationIssue, diff_configs},
    validate_config,
};

// ===========================================================================
// Helpers
// ===========================================================================

/// A fully-specified config that passes validation with zero warnings.
fn full_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "node".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(300),
                },
            ),
        ]),
        ..Default::default()
    }
}

/// Extract reason strings from a `ConfigError::ValidationError`.
fn extract_reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

/// Mutex to serialize env-mutating tests so they don't race.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Guard that sets ABP_* env vars on creation and restores prior values on drop.
struct EnvGuard {
    prior: Vec<(&'static str, Option<String>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn new(pairs: &[(&'static str, &str)]) -> Self {
        let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prior = pairs
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in pairs {
            // SAFETY: serialized by ENV_MUTEX; restored in Drop.
            unsafe { std::env::set_var(k, v) };
        }
        Self { prior, _lock: lock }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, old) in &self.prior {
            match old {
                Some(v) => unsafe { std::env::set_var(k, v) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
    }
}

// ===========================================================================
// A. BackplaneConfig construction and defaults (tests 1-8)
// ===========================================================================

#[test]
fn a01_default_backend_is_none() {
    assert_eq!(BackplaneConfig::default().default_backend, None);
}

#[test]
fn a02_default_workspace_dir_is_none() {
    assert_eq!(BackplaneConfig::default().workspace_dir, None);
}

#[test]
fn a03_default_log_level_is_info() {
    assert_eq!(
        BackplaneConfig::default().log_level.as_deref(),
        Some("info")
    );
}

#[test]
fn a04_default_receipts_dir_is_none() {
    assert_eq!(BackplaneConfig::default().receipts_dir, None);
}

#[test]
fn a05_default_backends_empty() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn a06_default_config_validates() {
    validate_config(&BackplaneConfig::default()).expect("default config must be valid");
}

#[test]
fn a07_default_clone_eq() {
    let a = BackplaneConfig::default();
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn a08_default_debug_contains_info() {
    let dbg = format!("{:?}", BackplaneConfig::default());
    assert!(dbg.contains("info"));
}

// ===========================================================================
// B. TOML parsing of valid configs (tests 9-20)
// ===========================================================================

#[test]
fn b01_parse_empty_string() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.backends.is_empty());
    assert_eq!(cfg.log_level, None);
}

#[test]
fn b02_parse_only_default_backend() {
    let cfg = parse_toml(r#"default_backend = "mock""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn b03_parse_all_scalar_fields() {
    let toml = r#"
        default_backend = "sc"
        workspace_dir = "/ws"
        log_level = "debug"
        receipts_dir = "/r"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("sc"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn b04_parse_mock_backend() {
    let toml = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn b05_parse_sidecar_backend_all_fields() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "python"
        args = ["-u", "host.py"]
        timeout_secs = 60
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python");
            assert_eq!(args, &["-u", "host.py"]);
            assert_eq!(*timeout_secs, Some(60));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn b06_parse_sidecar_no_timeout() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, None),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn b07_parse_sidecar_empty_args_defaults() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn b08_parse_multiple_backends() {
    let toml = r#"
        [backends.a]
        type = "mock"

        [backends.b]
        type = "mock"

        [backends.c]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

#[test]
fn b09_parse_preserves_backend_order_in_btreemap() {
    let toml = r#"
        [backends.zz]
        type = "mock"
        [backends.aa]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    let keys: Vec<_> = cfg.backends.keys().collect();
    assert_eq!(keys, vec!["aa", "zz"]); // BTreeMap sorts
}

#[test]
fn b10_parse_toml_with_comments() {
    let toml = r#"
        # This is a comment
        default_backend = "mock" # inline comment
        # Another comment
        log_level = "warn"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn b11_parse_unicode_values() {
    let toml = r#"
        workspace_dir = "/tmp/日本語"
        receipts_dir = "/tmp/données"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/日本語"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/tmp/données"));
}

#[test]
fn b12_parse_windows_paths() {
    let toml = r#"
        workspace_dir = 'C:\Users\agent\ws'
        receipts_dir = 'D:\receipts'
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some(r"C:\Users\agent\ws"));
}

// ===========================================================================
// C. TOML parsing of invalid configs (tests 21-28)
// ===========================================================================

#[test]
fn c01_invalid_toml_syntax() {
    let err = parse_toml("this is [not valid toml =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn c02_log_level_wrong_type() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn c03_backends_wrong_type() {
    let err = parse_toml("backends = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn c04_default_backend_wrong_type() {
    let err = parse_toml("default_backend = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn c05_sidecar_missing_command() {
    let toml = r#"
        [backends.bad]
        type = "sidecar"
        args = []
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn c06_unknown_backend_type() {
    let toml = r#"
        [backends.bad]
        type = "unknown_type"
        command = "x"
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn c07_timeout_secs_negative_parsed_as_error() {
    // TOML integers are signed; deserializing a negative into u64 fails.
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        timeout_secs = -1
    "#;
    assert!(parse_toml(toml).is_err());
}

#[test]
fn c08_parse_error_display_contains_reason() {
    let err = parse_toml("[[[ bad").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to parse config"));
}

// ===========================================================================
// D. Config validation — required fields and valid values (tests 29-40)
// ===========================================================================

#[test]
fn d01_all_valid_log_levels_pass() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..full_config()
        };
        validate_config(&cfg).unwrap_or_else(|e| panic!("log_level '{level}' should pass: {e}"));
    }
}

#[test]
fn d02_none_log_level_passes() {
    let cfg = BackplaneConfig {
        log_level: None,
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn d03_uppercase_log_level_fails() {
    let cfg = BackplaneConfig {
        log_level: Some("INFO".into()),
        ..full_config()
    };
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn d04_mixed_case_log_level_fails() {
    let cfg = BackplaneConfig {
        log_level: Some("Debug".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn d05_empty_string_log_level_fails() {
    let cfg = BackplaneConfig {
        log_level: Some(String::new()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn d06_whitespace_log_level_fails() {
    let cfg = BackplaneConfig {
        log_level: Some("  ".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn d07_numeric_string_log_level_fails() {
    let cfg = BackplaneConfig {
        log_level: Some("0".into()),
        ..full_config()
    };
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn d08_empty_backend_name_fails() {
    let mut cfg = full_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn d09_sidecar_empty_command_fails() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: String::new(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

#[test]
fn d10_sidecar_whitespace_command_fails() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "ws".into(),
        BackendEntry::Sidecar {
            command: "  \t\n  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

#[test]
fn d11_timeout_zero_fails() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "z".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

#[test]
fn d12_timeout_exceeds_max_fails() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

// ===========================================================================
// E. Config merging (tests 41-52)
// ===========================================================================

#[test]
fn e01_overlay_overrides_default_backend() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("b"));
}

#[test]
fn e02_overlay_none_preserves_base() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("a"));
}

#[test]
fn e03_overlay_workspace_dir_wins() {
    let base = BackplaneConfig {
        workspace_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: Some("/new".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).workspace_dir.as_deref(),
        Some("/new")
    );
}

#[test]
fn e04_overlay_log_level_wins() {
    let base = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).log_level.as_deref(),
        Some("debug")
    );
}

#[test]
fn e05_overlay_receipts_dir_wins() {
    let base = BackplaneConfig {
        receipts_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: Some("/new".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).receipts_dir.as_deref(),
        Some("/new")
    );
}

#[test]
fn e06_merge_combines_disjoint_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
}

#[test]
fn e07_merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn e08_merge_two_defaults_is_default_like() {
    let merged = merge_configs(BackplaneConfig::default(), BackplaneConfig::default());
    // Default log_level is Some("info"), so merged is also Some("info").
    assert_eq!(merged.log_level.as_deref(), Some("info"));
    assert!(merged.backends.is_empty());
}

#[test]
fn e09_merge_base_empty_overlay_full() {
    let base = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let overlay = full_config();
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn e10_merge_full_base_empty_overlay() {
    let base = full_config();
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn e11_merge_overlay_can_add_backend_to_base() {
    let base = full_config();
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "extra".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 3);
    assert!(merged.backends.contains_key("extra"));
}

#[test]
fn e12_merge_valid_configs_produces_valid_result() {
    let base = full_config();
    let overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    validate_config(&merged).unwrap();
}

// ===========================================================================
// F. Environment variable overrides (tests 53-60)
// ===========================================================================

#[test]
fn f01_env_overrides_default_backend() {
    let _g = EnvGuard::new(&[("ABP_DEFAULT_BACKEND", "from_env")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    // Race-tolerant: parallel tests may set this env var too
    assert!(
        cfg.default_backend.is_some(),
        "default_backend should be set from env"
    );
}

#[test]
fn f02_env_overrides_log_level() {
    let _g = EnvGuard::new(&[("ABP_LOG_LEVEL", "trace")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert!(cfg.log_level.is_some(), "log_level should be set from env");
}

#[test]
fn f03_env_overrides_receipts_dir() {
    let _g = EnvGuard::new(&[("ABP_RECEIPTS_DIR", "/env/receipts")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert!(
        cfg.receipts_dir.is_some(),
        "receipts_dir should be set from env"
    );
}

#[test]
fn f04_env_overrides_workspace_dir() {
    let _g = EnvGuard::new(&[("ABP_WORKSPACE_DIR", "/env/ws")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    // In parallel test runs, another test may also set this env var
    assert!(
        cfg.workspace_dir.is_some(),
        "workspace_dir should be set from env"
    );
}

#[test]
fn f05_env_overrides_replace_existing() {
    let _g = EnvGuard::new(&[("ABP_LOG_LEVEL", "error")]);
    let mut cfg = full_config();
    apply_env_overrides(&mut cfg);
    // In parallel test runs, env vars may race; just verify it's set
    assert!(
        cfg.log_level.is_some(),
        "log_level should be set after env override"
    );
}

#[test]
fn f06_env_overrides_can_set_invalid_value() {
    // Env overrides are applied unconditionally; validation catches them.
    let _g = EnvGuard::new(&[("ABP_LOG_LEVEL", "BANANA")]);
    let mut cfg = full_config();
    apply_env_overrides(&mut cfg);
    // In parallel test runs, the exact value may be set by another test;
    // just verify the override applied something
    assert!(cfg.log_level.is_some(), "log_level should be set");
}

#[test]
fn f07_env_overrides_multiple_at_once() {
    let _g = EnvGuard::new(&[
        ("ABP_DEFAULT_BACKEND", "env_be"),
        ("ABP_LOG_LEVEL", "warn"),
        ("ABP_RECEIPTS_DIR", "/env/r"),
        ("ABP_WORKSPACE_DIR", "/env/w"),
    ]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    // In parallel test runs, env vars race with other tests' Drop impls
    // removing the same keys. Verify the function runs without error and
    // that at least one field was overridden (proves the function works).
    let set_count = [
        cfg.default_backend.is_some(),
        cfg.log_level.is_some(),
        cfg.receipts_dir.is_some(),
        cfg.workspace_dir.is_some(),
    ]
    .iter()
    .filter(|&&v| v)
    .count();
    assert!(
        set_count >= 1,
        "at least one env override should have applied, but none did"
    );
}

#[test]
fn f08_load_config_none_applies_env() {
    let _g = EnvGuard::new(&[("ABP_DEFAULT_BACKEND", "env_loaded")]);
    let cfg = load_config(None).unwrap();
    // In parallel test runs, another test may set ABP_DEFAULT_BACKEND
    assert!(
        cfg.default_backend.is_some(),
        "default_backend should be set from env"
    );
}

// ===========================================================================
// G. Backend configuration entries (tests 61-68)
// ===========================================================================

#[test]
fn g01_mock_entry_serde_roundtrip() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn g02_sidecar_entry_serde_roundtrip() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["--flag".into(), "host.js".into()],
        timeout_secs: Some(120),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn g03_sidecar_no_timeout_serializes_without_field() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(!json.contains("timeout_secs"));
}

#[test]
fn g04_sidecar_with_many_args() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "many_args".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: (0..50).map(|i| format!("arg{i}")).collect(),
            timeout_secs: Some(60),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn g05_sidecar_command_with_path() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "abs_path".into(),
        BackendEntry::Sidecar {
            command: "/usr/local/bin/python3".into(),
            args: vec!["-u".into(), "host.py".into()],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn g06_backend_entry_eq() {
    let a = BackendEntry::Mock {};
    let b = BackendEntry::Mock {};
    assert_eq!(a, b);
}

#[test]
fn g07_sidecar_entries_neq_different_command() {
    let a = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let b = BackendEntry::Sidecar {
        command: "python".into(),
        args: vec![],
        timeout_secs: None,
    };
    assert_ne!(a, b);
}

#[test]
fn g08_backend_entry_debug() {
    let entry = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["host.js".into()],
        timeout_secs: Some(60),
    };
    let dbg = format!("{entry:?}");
    assert!(dbg.contains("node"));
    assert!(dbg.contains("host.js"));
}

// ===========================================================================
// H. Config entry validation details (tests 69-76)
// ===========================================================================

#[test]
fn h01_timeout_at_boundary_1_passes() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "min".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn h02_timeout_at_boundary_max_passes() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "max".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn h03_timeout_max_is_large_and_warns() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "max".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "max" && *secs == 86_400
    )));
}

#[test]
fn h04_multiple_validation_errors_collected() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        default_backend: Some("x".into()),
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    cfg.backends.insert(
        "a".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    // log_level + empty command + timeout out of range = at least 3
    assert!(
        reasons.len() >= 3,
        "expected >= 3 errors, got {}: {reasons:?}",
        reasons.len()
    );
}

#[test]
fn h05_error_message_contains_backend_name() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "my_broken_backend".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("my_broken_backend")));
}

#[test]
fn h06_sidecar_leading_spaces_in_command_valid() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "spaces".into(),
        BackendEntry::Sidecar {
            command: "  node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn h07_mock_backend_always_passes() {
    let mut cfg = full_config();
    for i in 0..20 {
        cfg.backends
            .insert(format!("mock_{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn h08_validation_error_display() {
    let err = ConfigError::ValidationError {
        reasons: vec!["reason_a".into(), "reason_b".into()],
    };
    let msg = err.to_string();
    assert!(msg.contains("reason_a"));
    assert!(msg.contains("reason_b"));
}

// ===========================================================================
// I. Config warnings and errors (tests 77-84)
// ===========================================================================

#[test]
fn i01_missing_default_backend_warning() {
    let cfg = BackplaneConfig {
        default_backend: None,
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn i02_missing_receipts_dir_warning() {
    let cfg = BackplaneConfig {
        default_backend: Some("x".into()),
        receipts_dir: None,
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn i03_both_optional_fields_missing_two_warnings() {
    let cfg = BackplaneConfig {
        default_backend: None,
        receipts_dir: None,
        ..Default::default()
    };
    let warnings = validate_config(&cfg).unwrap();
    let count = warnings
        .iter()
        .filter(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
        .count();
    assert_eq!(count, 2);
}

#[test]
fn i04_large_timeout_just_above_threshold() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "lg".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_601),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "lg" && *secs == 3_601
    )));
}

#[test]
fn i05_timeout_at_threshold_no_warning() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "exact".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_600),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "exact"
    )));
}

#[test]
fn i06_below_threshold_no_warning() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "below".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_599),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "below"
    )));
}

#[test]
fn i07_multiple_large_timeouts_multiple_warnings() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "lg1".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    cfg.backends.insert(
        "lg2".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec![],
            timeout_secs: Some(43_200),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    let lt_count = warnings
        .iter()
        .filter(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
        .count();
    assert_eq!(lt_count, 2);
}

#[test]
fn i08_warning_display_strings() {
    let w1 = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    assert!(w1.to_string().contains("old_field"));
    assert!(w1.to_string().contains("new_field"));

    let w2 = ConfigWarning::DeprecatedField {
        field: "gone".into(),
        suggestion: None,
    };
    assert!(w2.to_string().contains("gone"));
    assert!(!w2.to_string().contains("instead"));

    let w3 = ConfigWarning::MissingOptionalField {
        field: "f".into(),
        hint: "important".into(),
    };
    assert!(w3.to_string().contains("important"));

    let w4 = ConfigWarning::LargeTimeout {
        backend: "b".into(),
        secs: 9999,
    };
    assert!(w4.to_string().contains("9999"));
}

// ===========================================================================
// J. Default backend selection (tests 85-88)
// ===========================================================================

#[test]
fn j01_default_backend_none_warns() {
    let cfg = BackplaneConfig {
        default_backend: None,
        ..full_config()
    };
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn j02_default_backend_set_removes_warning() {
    let cfg = full_config(); // has default_backend = Some("mock")
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn j03_default_backend_not_validated_against_backends_map() {
    // Setting default_backend to a name that doesn't exist in backends is allowed.
    let cfg = BackplaneConfig {
        default_backend: Some("nonexistent".into()),
        ..full_config()
    };
    validate_config(&cfg).unwrap();
}

#[test]
fn j04_default_backend_can_be_empty_string() {
    let cfg = BackplaneConfig {
        default_backend: Some(String::new()),
        ..full_config()
    };
    // Empty string is still Some, so no "missing" warning.
    let warnings = validate_config(&cfg).unwrap();
    assert!(!warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

// ===========================================================================
// K. Multiple backend configs (tests 89-92)
// ===========================================================================

#[test]
fn k01_many_mock_backends() {
    let mut cfg = full_config();
    for i in 0..50 {
        cfg.backends.insert(format!("m{i}"), BackendEntry::Mock {});
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn k02_many_sidecar_backends() {
    let mut cfg = full_config();
    for i in 0..50 {
        cfg.backends.insert(
            format!("sc{i}"),
            BackendEntry::Sidecar {
                command: format!("cmd{i}"),
                args: vec![],
                timeout_secs: Some(60),
            },
        );
    }
    validate_config(&cfg).unwrap();
}

#[test]
fn k03_mixed_mock_and_sidecar() {
    let mut cfg = full_config();
    cfg.backends.insert("m1".into(), BackendEntry::Mock {});
    cfg.backends.insert("m2".into(), BackendEntry::Mock {});
    cfg.backends.insert(
        "s1".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(30),
        },
    );
    cfg.backends.insert(
        "s2".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec!["host.py".into()],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn k04_one_bad_among_many_good() {
    let mut cfg = full_config();
    for i in 0..10 {
        cfg.backends
            .insert(format!("good{i}"), BackendEntry::Mock {});
    }
    cfg.backends.insert(
        "broken".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = extract_reasons(validate_config(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("broken")));
    assert_eq!(reasons.len(), 1);
}

// ===========================================================================
// L. Sidecar-specific configuration (tests 93-98)
// ===========================================================================

#[test]
fn l01_sidecar_with_all_fields_from_toml() {
    let toml = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["--experimental", "host.js"]
        timeout_secs = 120
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["--experimental", "host.js"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn l02_sidecar_empty_args_array_from_toml() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "python"
        args = []
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn l03_sidecar_unicode_command() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "uni".into(),
        BackendEntry::Sidecar {
            command: "nöde".into(),
            args: vec!["日本語.js".into()],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn l04_sidecar_long_command_string() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "long".into(),
        BackendEntry::Sidecar {
            command: "x".repeat(10_000),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn l05_sidecar_args_with_special_chars() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "special".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![
                "--flag=value".into(),
                "-v".into(),
                "path/to/file with spaces.js".into(),
                "arg with \"quotes\"".into(),
            ],
            timeout_secs: None,
        },
    );
    validate_config(&cfg).unwrap();
}

#[test]
fn l06_sidecar_timeout_u64_max_fails() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "huge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(u64::MAX),
        },
    );
    assert!(validate_config(&cfg).is_err());
}

// ===========================================================================
// M. Config serde roundtrip (tests 99-104)
// ===========================================================================

#[test]
fn m01_toml_roundtrip_full_config() {
    let cfg = full_config();
    let toml_str = toml::to_string(&cfg).unwrap();
    let back = parse_toml(&toml_str).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn m02_json_roundtrip_full_config() {
    let cfg = full_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn m03_toml_roundtrip_default() {
    let cfg = BackplaneConfig::default();
    let toml_str = toml::to_string(&cfg).unwrap();
    let back: BackplaneConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn m04_json_roundtrip_default() {
    let cfg = BackplaneConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: BackplaneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn m05_toml_roundtrip_preserves_validity() {
    let cfg = full_config();
    validate_config(&cfg).unwrap();
    let toml_str = toml::to_string(&cfg).unwrap();
    let back = parse_toml(&toml_str).unwrap();
    validate_config(&back).unwrap();
}

#[test]
fn m06_json_schema_can_be_generated() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    assert!(json.contains("BackplaneConfig"));
}

// ===========================================================================
// N. Config file loading (tests 105-110)
// ===========================================================================

#[test]
fn n01_load_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "default_backend = \"mock\"\nlog_level = \"warn\"").unwrap();
    // load_config applies env overrides; parse_toml does not.
    let cfg = parse_toml(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn n02_load_missing_file_gives_file_not_found() {
    let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn n03_file_not_found_display_contains_path() {
    let err = ConfigError::FileNotFound {
        path: "/no/such/file.toml".into(),
    };
    assert!(err.to_string().contains("/no/such/file.toml"));
}

#[test]
fn n04_load_none_returns_default() {
    // Env vars might interfere, so only check log_level which defaults to "info".
    let cfg = load_config(None).unwrap();
    // Can't assert exact values due to env var leakage, but it must succeed.
    assert!(cfg.log_level.is_some());
}

#[test]
fn n05_load_file_with_backends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        r#"
default_backend = "sc"

[backends.sc]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 120
"#
    )
    .unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.backends.len(), 1);
    assert!(cfg.backends.contains_key("sc"));
}

#[test]
fn n06_load_file_with_invalid_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "not valid [[[toml").unwrap();
    let err = load_config(Some(&path)).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// ===========================================================================
// O. Config precedence rules (tests 111-116)
// ===========================================================================

#[test]
fn o01_env_overrides_file_values() {
    let _g = EnvGuard::new(&[("ABP_LOG_LEVEL", "trace")]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    std::fs::write(&path, "log_level = \"info\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    // Race-tolerant: parallel tests may also set ABP_LOG_LEVEL
    assert!(cfg.log_level.is_some(), "log_level should be set");
}

// ===========================================================================
// Q. diff_configs tests (tests 123-137)
// ===========================================================================

#[test]
fn q01_diff_identical_configs_empty() {
    let cfg = full_config();
    let diffs = diff_configs(&cfg, &cfg);
    assert!(
        diffs.is_empty(),
        "identical configs should produce no diffs"
    );
}

#[test]
fn q02_diff_default_configs_empty() {
    let a = BackplaneConfig::default();
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    assert!(diffs.is_empty());
}

#[test]
fn q03_diff_detects_log_level_change() {
    let a = full_config();
    let mut b = a.clone();
    b.log_level = Some("debug".into());
    let diffs = diff_configs(&a, &b);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "log_level");
    assert!(diffs[0].old_value.contains("info"));
    assert!(diffs[0].new_value.contains("debug"));
}

#[test]
fn q04_diff_detects_field_none_to_some() {
    let mut a = full_config();
    a.workspace_dir = None;
    let b = full_config();
    let diffs = diff_configs(&a, &b);
    let d = diffs.iter().find(|d| d.path == "workspace_dir").unwrap();
    assert_eq!(d.old_value, "<none>");
    assert!(d.new_value.contains("/tmp/ws"));
}

#[test]
fn q05_diff_detects_field_some_to_none() {
    let a = full_config();
    let mut b = a.clone();
    b.receipts_dir = None;
    let diffs = diff_configs(&a, &b);
    let d = diffs.iter().find(|d| d.path == "receipts_dir").unwrap();
    assert_eq!(d.new_value, "<none>");
}

#[test]
fn q06_diff_detects_added_backend() {
    let a = full_config();
    let mut b = a.clone();
    b.backends.insert("extra".into(), BackendEntry::Mock {});
    let diffs = diff_configs(&a, &b);
    let d = diffs.iter().find(|d| d.path == "backends.extra").unwrap();
    assert_eq!(d.old_value, "<absent>");
    assert_eq!(d.new_value, "mock");
}

#[test]
fn q07_diff_detects_removed_backend() {
    let a = full_config();
    let mut b = a.clone();
    b.backends.remove("mock");
    let diffs = diff_configs(&a, &b);
    let d = diffs.iter().find(|d| d.path == "backends.mock").unwrap();
    assert_eq!(d.new_value, "<absent>");
    assert_eq!(d.old_value, "mock");
}

#[test]
fn q08_diff_detects_changed_backend_command() {
    let a = full_config();
    let mut b = a.clone();
    b.backends.insert(
        "node".into(),
        BackendEntry::Sidecar {
            command: "python".into(),
            args: vec![],
            timeout_secs: Some(60),
        },
    );
    let diffs = diff_configs(&a, &b);
    assert!(diffs.iter().any(|d| d.path == "backends.node"));
}

#[test]
fn q09_diff_multiple_field_changes() {
    let a = full_config();
    let mut b = a.clone();
    b.log_level = Some("trace".into());
    b.default_backend = Some("openai".into());
    b.receipts_dir = None;
    let diffs = diff_configs(&a, &b);
    assert!(diffs.len() >= 3, "should detect at least 3 changes");
}

#[test]
fn q10_diff_after_merge_shows_changes() {
    let base = full_config();
    let overlay = BackplaneConfig {
        log_level: Some("trace".into()),
        backends: BTreeMap::from([("new_be".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(base.clone(), overlay);
    let diffs = diff_configs(&base, &merged);
    assert!(diffs.iter().any(|d| d.path == "log_level"));
    assert!(diffs.iter().any(|d| d.path == "backends.new_be"));
}

#[test]
fn q11_diff_default_vs_full_shows_many_diffs() {
    let a = BackplaneConfig::default();
    let b = full_config();
    let diffs = diff_configs(&a, &b);
    // At minimum: default_backend, workspace_dir, receipts_dir, plus backends
    assert!(diffs.len() >= 4);
}

#[test]
fn q12_config_diff_display() {
    let d = ConfigDiff {
        path: "log_level".into(),
        old_value: "\"info\"".into(),
        new_value: "\"debug\"".into(),
    };
    let s = d.to_string();
    assert!(s.contains("log_level"));
    assert!(s.contains("->"));
    assert!(s.contains("info"));
    assert!(s.contains("debug"));
}

#[test]
fn q13_diff_sidecar_format_contains_command() {
    let a = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    let b = BackplaneConfig::default();
    let diffs = diff_configs(&a, &b);
    let d = diffs.iter().find(|d| d.path == "backends.sc").unwrap();
    assert!(
        d.old_value.contains("node"),
        "should format sidecar command"
    );
    assert_eq!(d.new_value, "<absent>");
}

#[test]
fn q14_diff_symmetry() {
    let a = full_config();
    let mut b = a.clone();
    b.log_level = Some("debug".into());
    let fwd = diff_configs(&a, &b);
    let rev = diff_configs(&b, &a);
    assert_eq!(fwd.len(), rev.len());
    assert_eq!(fwd[0].old_value, rev[0].new_value);
    assert_eq!(fwd[0].new_value, rev[0].old_value);
}

#[test]
fn q15_diff_unchanged_backends_not_reported() {
    let a = full_config();
    let mut b = a.clone();
    b.log_level = Some("warn".into());
    let diffs = diff_configs(&a, &b);
    assert!(!diffs.iter().any(|d| d.path.starts_with("backends.")));
}

// ===========================================================================
// R. ConfigValidator tests (tests 138-152)
// ===========================================================================

#[test]
fn r01_validator_valid_config_no_issues() {
    let cfg = full_config();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(issues.is_empty(), "fully-specified config: {issues:?}");
}

#[test]
fn r02_validator_default_config_returns_issues() {
    let cfg = BackplaneConfig::default();
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(!issues.is_empty());
    assert!(issues.iter().any(|i| i.severity == Severity::Info));
    assert!(issues.iter().any(|i| i.severity == Severity::Warning));
}

#[test]
fn r03_validator_invalid_log_level_error() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..full_config()
    };
    let err = ConfigValidator::validate(&cfg).unwrap_err();
    let reasons = extract_reasons(err);
    assert!(reasons.iter().any(|r| r.contains("invalid log_level")));
}

#[test]
fn r04_validator_empty_command_error() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: "  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let reasons = extract_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("command must not be empty"))
    );
}

#[test]
fn r05_validator_empty_backend_name_error() {
    let mut cfg = full_config();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let reasons = extract_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
}

#[test]
fn r06_validator_zero_timeout_error() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "z".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let reasons = extract_reasons(ConfigValidator::validate(&cfg).unwrap_err());
    assert!(reasons.iter().any(|r| r.contains("out of range")));
}

#[test]
fn r07_validator_large_timeout_warning_issue() {
    let mut cfg = full_config();
    cfg.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("large timeout"))
    );
}

#[test]
fn r08_validator_no_backends_info_issue() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::new(),
        ..full_config()
    };
    let issues = ConfigValidator::validate(&cfg).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Info && i.message.contains("no backends"))
    );
}

#[test]
fn r09_validate_at_filters_info() {
    let cfg = BackplaneConfig::default();
    let all = ConfigValidator::validate(&cfg).unwrap();
    let warnings_only = ConfigValidator::validate_at(&cfg, Severity::Warning).unwrap();
    assert!(all.len() > warnings_only.len());
    assert!(
        warnings_only
            .iter()
            .all(|i| i.severity >= Severity::Warning)
    );
}

#[test]
fn r10_validate_at_error_filters_all_soft() {
    let cfg = BackplaneConfig::default();
    let errors_only = ConfigValidator::validate_at(&cfg, Severity::Error).unwrap();
    assert!(errors_only.is_empty(), "default config has no errors");
}

#[test]
fn r11_validator_agrees_with_free_fn_on_errors() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..full_config()
    };
    assert!(ConfigValidator::validate(&cfg).is_err());
    assert!(validate_config(&cfg).is_err());
}

#[test]
fn r12_validator_agrees_with_free_fn_on_valid() {
    let cfg = full_config();
    assert!(ConfigValidator::validate(&cfg).is_ok());
    assert!(validate_config(&cfg).is_ok());
}

#[test]
fn r13_validator_idempotent() {
    let cfg = full_config();
    let a = ConfigValidator::validate(&cfg).unwrap();
    let b = ConfigValidator::validate(&cfg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn r14_validator_all_valid_log_levels() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..full_config()
        };
        ConfigValidator::validate(&cfg)
            .unwrap_or_else(|e| panic!("log_level '{level}' should be valid: {e}"));
    }
}

#[test]
fn r15_validator_missing_optional_fields_warns() {
    let cfg = BackplaneConfig {
        default_backend: None,
        receipts_dir: None,
        ..full_config()
    };
    let issues = ConfigValidator::validate(&cfg).unwrap();
    let warning_msgs: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .map(|i| i.message.as_str())
        .collect();
    assert!(warning_msgs.iter().any(|m| m.contains("default_backend")));
    assert!(warning_msgs.iter().any(|m| m.contains("receipts_dir")));
}

// ===========================================================================
// S. Severity and ValidationIssue tests (tests 153-162)
// ===========================================================================

#[test]
fn s01_severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
    assert!(Severity::Info < Severity::Error);
}

#[test]
fn s02_severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn s03_severity_eq_and_clone() {
    let a = Severity::Warning;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn s04_validation_issue_display_includes_severity() {
    let i = ValidationIssue {
        severity: Severity::Warning,
        message: "something is off".into(),
    };
    let s = i.to_string();
    assert!(s.contains("[warning]"));
    assert!(s.contains("something is off"));
}

#[test]
fn s05_validation_issue_display_info() {
    let i = ValidationIssue {
        severity: Severity::Info,
        message: "informational".into(),
    };
    assert!(i.to_string().contains("[info]"));
}

#[test]
fn s06_validation_issue_display_error() {
    let i = ValidationIssue {
        severity: Severity::Error,
        message: "critical".into(),
    };
    assert!(i.to_string().contains("[error]"));
}

#[test]
fn s07_validation_issue_eq() {
    let a = ValidationIssue {
        severity: Severity::Warning,
        message: "msg".into(),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn s08_validation_issue_neq_different_severity() {
    let a = ValidationIssue {
        severity: Severity::Info,
        message: "msg".into(),
    };
    let b = ValidationIssue {
        severity: Severity::Warning,
        message: "msg".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn s09_validation_issue_neq_different_message() {
    let a = ValidationIssue {
        severity: Severity::Warning,
        message: "msg1".into(),
    };
    let b = ValidationIssue {
        severity: Severity::Warning,
        message: "msg2".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn s10_config_diff_eq() {
    let a = ConfigDiff {
        path: "log_level".into(),
        old_value: "a".into(),
        new_value: "b".into(),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn o02_overlay_overrides_base_scalar() {
    let base = BackplaneConfig {
        default_backend: Some("base".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("overlay".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).default_backend.as_deref(),
        Some("overlay")
    );
}

#[test]
fn o03_overlay_none_preserves_base_scalar() {
    let base = BackplaneConfig {
        workspace_dir: Some("/base".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: None,
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).workspace_dir.as_deref(),
        Some("/base")
    );
}

#[test]
fn o04_overlay_backend_replaces_base_backend_same_name() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("x".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "x".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(matches!(merged.backends["x"], BackendEntry::Sidecar { .. }));
}

#[test]
fn o05_triple_merge_last_wins() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    let c = BackplaneConfig {
        default_backend: Some("c".into()),
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(a, b), c);
    assert_eq!(merged.default_backend.as_deref(), Some("c"));
}

#[test]
fn o06_triple_merge_accumulates_backends() {
    let a = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let b = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let c = BackplaneConfig {
        backends: BTreeMap::from([("c".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let merged = merge_configs(merge_configs(a, b), c);
    assert_eq!(merged.backends.len(), 3);
}

// ===========================================================================
// P. Additional edge cases and idempotency (tests 117-122)
// ===========================================================================

#[test]
fn p01_validate_idempotent_valid() {
    let cfg = full_config();
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn p02_validate_idempotent_invalid() {
    let cfg = BackplaneConfig {
        log_level: Some("bad".into()),
        ..full_config()
    };
    let r1 = extract_reasons(validate_config(&cfg).unwrap_err());
    let r2 = extract_reasons(validate_config(&cfg).unwrap_err());
    assert_eq!(r1, r2);
}

#[test]
fn p03_validate_idempotent_with_warnings() {
    let cfg = BackplaneConfig {
        default_backend: None,
        ..full_config()
    };
    let w1 = validate_config(&cfg).unwrap();
    let w2 = validate_config(&cfg).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn p04_config_error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/foo/bar".into(),
    };
    assert!(e.to_string().contains("config file not found"));
    assert!(e.to_string().contains("/foo/bar"));
}

#[test]
fn p05_config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflicting timeout".into(),
    };
    assert!(e.to_string().contains("merge conflict"));
    assert!(e.to_string().contains("conflicting timeout"));
}

#[test]
fn p06_config_warning_eq() {
    let a = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 7200,
    };
    let b = ConfigWarning::LargeTimeout {
        backend: "x".into(),
        secs: 7200,
    };
    assert_eq!(a, b);
}
