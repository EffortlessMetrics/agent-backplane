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
//! Comprehensive tests for the CLI config parsing system.
//!
//! Covers: TOML parsing, default values, validation, config merging,
//! environment variable overrides, and backend configuration.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use abp_config::{
    apply_env_overrides, load_config, merge_configs, parse_toml, validate_config, BackendEntry,
    BackplaneConfig, ConfigError, ConfigWarning,
};

// ---------------------------------------------------------------------------
// Helper: RAII guard for ABP_* environment variables
// ---------------------------------------------------------------------------

struct EnvGuard {
    keys: Vec<&'static str>,
}

impl EnvGuard {
    fn new(pairs: &[(&'static str, &str)]) -> Self {
        let keys: Vec<&'static str> = pairs.iter().map(|(k, _)| *k).collect();
        for (k, v) in pairs {
            // SAFETY: env vars are process-global; these tests accept the race.
            unsafe { std::env::set_var(k, v) };
        }
        Self { keys }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for k in &self.keys {
            unsafe { std::env::remove_var(k) };
        }
    }
}

// =========================================================================
// 1. TOML Parsing (15 tests)
// =========================================================================

#[test]
fn toml_parse_empty_string() {
    let cfg = parse_toml("").unwrap();
    assert_eq!(cfg.default_backend, None);
    assert!(cfg.backends.is_empty());
}

#[test]
fn toml_parse_minimal_default_backend() {
    let cfg = parse_toml(r#"default_backend = "mock""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn toml_parse_all_top_level_fields() {
    let toml = r#"
        default_backend = "openai"
        workspace_dir = "/tmp/ws"
        log_level = "debug"
        receipts_dir = "/var/receipts"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/ws"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/var/receipts"));
}

#[test]
fn toml_parse_single_mock_backend() {
    let toml = r#"
        [backends.mock]
        type = "mock"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 1);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
}

#[test]
fn toml_parse_single_sidecar_backend() {
    let toml = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["node"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_parse_multiple_backends() {
    let toml = r#"
        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["openai.js"]

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["anthropic.py"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(cfg.backends.contains_key("mock"));
    assert!(cfg.backends.contains_key("openai"));
    assert!(cfg.backends.contains_key("anthropic"));
}

#[test]
fn toml_parse_sidecar_with_all_fields() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["--experimental", "host.js"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["--experimental", "host.js"]);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_parse_sidecar_with_empty_args() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "python3"
        args = []
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_parse_sidecar_with_many_args() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["--flag1", "--flag2", "a", "b", "c"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert_eq!(args.len(), 5),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_parse_with_comments() {
    let toml = r#"
        # This is a comment
        default_backend = "mock"
        # Another comment
        log_level = "warn"

        [backends.mock]
        type = "mock"  # inline comment
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
}

#[test]
fn toml_parse_preserves_string_values_with_spaces() {
    let toml = r#"
        default_backend = "my custom backend"
        workspace_dir = "/path/with spaces/workspace"
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("my custom backend"));
    assert_eq!(
        cfg.workspace_dir.as_deref(),
        Some("/path/with spaces/workspace")
    );
}

#[test]
fn toml_parse_backends_section_with_special_name_chars() {
    let toml = r#"
        [backends."sidecar:node"]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
    "#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("sidecar:node"));
}

#[test]
fn toml_parse_invalid_syntax_returns_error() {
    let bad = "this is [not valid = toml";
    let err = parse_toml(bad).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_wrong_type_for_field_returns_error() {
    let toml = r#"log_level = 42"#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn toml_parse_unknown_backend_type_returns_error() {
    let toml = r#"
        [backends.bad]
        type = "nonexistent"
        command = "foo"
    "#;
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// =========================================================================
// 2. Default Values (10 tests)
// =========================================================================

#[test]
fn defaults_log_level_is_info() {
    let cfg = BackplaneConfig::default();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn defaults_no_default_backend() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.default_backend.is_none());
}

#[test]
fn defaults_no_workspace_dir() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.workspace_dir.is_none());
}

#[test]
fn defaults_no_receipts_dir() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.receipts_dir.is_none());
}

#[test]
fn defaults_empty_backends_map() {
    let cfg = BackplaneConfig::default();
    assert!(cfg.backends.is_empty());
}

#[test]
fn defaults_empty_toml_yields_none_for_optional_fields() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
    assert!(cfg.receipts_dir.is_none());
    // log_level from empty TOML is None (no #[serde(default)] with value).
    // The Default impl sets it to Some("info"), but TOML parse does not.
    assert!(cfg.log_level.is_none());
}

#[test]
fn defaults_sidecar_timeout_is_none_when_omitted() {
    let toml = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => {
            assert_eq!(*timeout_secs, None);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn defaults_sidecar_args_empty_when_omitted() {
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
fn defaults_load_none_returns_default_config() {
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn defaults_config_passes_validation() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).expect("default config should be valid");
    // Default config is missing optional fields, so there should be warnings.
    assert!(!warnings.is_empty());
}

// =========================================================================
// 3. Validation (15 tests)
// =========================================================================

#[test]
fn validate_valid_mock_only_passes() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("mock".into(), BackendEntry::Mock {});
    validate_config(&cfg).expect("mock-only config should pass");
}

#[test]
fn validate_valid_sidecar_passes() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["host.js".into()],
            timeout_secs: Some(300),
        },
    );
    validate_config(&cfg).expect("valid sidecar should pass");
}

#[test]
fn validate_catches_invalid_log_level() {
    let cfg = BackplaneConfig {
        log_level: Some("verbose".into()),
        ..Default::default()
    };
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_catches_empty_sidecar_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: String::new(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons
                .iter()
                .any(|r| r.contains("command must not be empty")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_catches_whitespace_only_command() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "ws".into(),
        BackendEntry::Sidecar {
            command: "   ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_catches_zero_timeout() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_catches_timeout_exceeding_max() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn validate_accepts_timeout_at_max() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    // Should not error; max timeout is 86400 which is within range.
    validate_config(&cfg).expect("timeout at max should pass");
}

#[test]
fn validate_accepts_timeout_at_one() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&cfg).expect("timeout=1 should pass");
}

#[test]
fn validate_catches_empty_backend_name() {
    let mut cfg = BackplaneConfig::default();
    cfg.backends.insert("".into(), BackendEntry::Mock {});
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_catches_multiple_errors() {
    let mut cfg = BackplaneConfig {
        log_level: Some("bad_level".into()),
        ..Default::default()
    };
    cfg.backends.insert(
        "bad1".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    cfg.backends.insert(
        "bad2".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let err = validate_config(&cfg).unwrap_err();
    match err {
        ConfigError::ValidationError { reasons } => {
            // Should have at least 3 errors: bad log level, empty command, zero timeout.
            assert!(
                reasons.len() >= 3,
                "expected >=3 errors, got {}: {reasons:?}",
                reasons.len()
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn validate_warns_missing_default_backend() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn validate_warns_missing_receipts_dir() {
    let cfg = BackplaneConfig::default();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn validate_large_timeout_produces_warning() {
    let mut cfg = BackplaneConfig {
        default_backend: Some("sc".into()),
        receipts_dir: Some("/tmp".into()),
        ..Default::default()
    };
    cfg.backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7200),
        },
    );
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. })));
}

#[test]
fn validate_all_log_levels_accepted() {
    for level in &["error", "warn", "info", "debug", "trace"] {
        let cfg = BackplaneConfig {
            log_level: Some((*level).into()),
            ..Default::default()
        };
        validate_config(&cfg).unwrap_or_else(|e| {
            panic!("log_level '{level}' should be valid, got: {e:?}");
        });
    }
}

// =========================================================================
// 4. Config Merging (10 tests)
// =========================================================================

#[test]
fn merge_overlay_overrides_default_backend() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        log_level: None,
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
}

#[test]
fn merge_overlay_overrides_log_level() {
    let base = BackplaneConfig {
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        log_level: Some("trace".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.log_level.as_deref(), Some("trace"));
}

#[test]
fn merge_preserves_base_when_overlay_fields_none() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/work".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/receipts".into()),
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
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
    assert_eq!(merged.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/receipts"));
    assert!(merged.backends.contains_key("m"));
}

#[test]
fn merge_combines_disjoint_backends() {
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
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn merge_overlay_backend_wins_on_collision() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python3".into(),
                args: vec!["old.py".into()],
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
                args: vec!["new.js".into()],
                timeout_secs: Some(120),
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["new.js"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_workspace_dir_overlay_wins() {
    let base = BackplaneConfig {
        workspace_dir: Some("/old".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        workspace_dir: Some("/new".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.workspace_dir.as_deref(), Some("/new"));
}

#[test]
fn merge_receipts_dir_overlay_wins() {
    let base = BackplaneConfig {
        receipts_dir: Some("/old/receipts".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        receipts_dir: Some("/new/receipts".into()),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.receipts_dir.as_deref(), Some("/new/receipts"));
}

#[test]
fn merge_both_empty_gives_empty() {
    let base = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(merged.default_backend.is_none());
    assert!(merged.log_level.is_none());
    assert!(merged.backends.is_empty());
}

#[test]
fn merge_multiple_sequential_overlays() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay1 = BackplaneConfig {
        log_level: Some("debug".into()),
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay2 = BackplaneConfig {
        default_backend: Some("prod".into()),
        log_level: None,
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let step1 = merge_configs(base, overlay1);
    let step2 = merge_configs(step1, overlay2);
    assert_eq!(step2.default_backend.as_deref(), Some("prod"));
    assert_eq!(step2.log_level.as_deref(), Some("debug"));
    assert!(step2.backends.contains_key("a"));
    assert!(step2.backends.contains_key("b"));
}

#[test]
fn merge_overlay_replaces_mock_with_sidecar() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("sc".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = merge_configs(base, overlay);
    assert!(matches!(
        merged.backends["sc"],
        BackendEntry::Sidecar { .. }
    ));
}

// =========================================================================
// 5. Backend Configuration (15 tests)
// =========================================================================

#[test]
fn backend_mock_roundtrip_toml() {
    let toml_str = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(toml_str).unwrap();
    let serialized = toml::to_string(&cfg).unwrap();
    let cfg2: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg.backends["m"], cfg2.backends["m"]);
}

#[test]
fn backend_sidecar_roundtrip_toml() {
    let toml_str = r#"
        [backends.node]
        type = "sidecar"
        command = "node"
        args = ["--flag", "host.js"]
        timeout_secs = 300
    "#;
    let cfg = parse_toml(toml_str).unwrap();
    let serialized = toml::to_string(&cfg).unwrap();
    let cfg2: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg.backends["node"], cfg2.backends["node"]);
}

#[test]
fn backend_sidecar_without_timeout_roundtrip() {
    let cfg = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["host.js".into()],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let cfg2: BackplaneConfig = toml::from_str(&serialized).unwrap();
    match &cfg2.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert!(timeout_secs.is_none()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_multiple_types_coexist() {
    let toml_str = r#"
        [backends.test]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["openai.js"]

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["anthropic.py"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(toml_str).unwrap();
    assert!(matches!(cfg.backends["test"], BackendEntry::Mock {}));
    assert!(matches!(
        cfg.backends["openai"],
        BackendEntry::Sidecar { .. }
    ));
    assert!(matches!(
        cfg.backends["anthropic"],
        BackendEntry::Sidecar { .. }
    ));
}

#[test]
fn backend_names_sorted_in_btreemap() {
    let toml_str = r#"
        [backends.zebra]
        type = "mock"

        [backends.alpha]
        type = "mock"

        [backends.middle]
        type = "mock"
    "#;
    let cfg = parse_toml(toml_str).unwrap();
    let names: Vec<&String> = cfg.backends.keys().collect();
    assert_eq!(names, &["alpha", "middle", "zebra"]);
}

#[test]
fn backend_sidecar_args_with_special_characters() {
    let toml_str = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["--config=/etc/app.conf", "--name=hello world", "path/to/file.js"]
    "#;
    let cfg = parse_toml(toml_str).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => {
            assert_eq!(args[0], "--config=/etc/app.conf");
            assert_eq!(args[1], "--name=hello world");
            assert_eq!(args[2], "path/to/file.js");
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_command_with_absolute_path() {
    let toml_str = r#"
        [backends.sc]
        type = "sidecar"
        command = "/usr/local/bin/node"
        args = ["host.js"]
    "#;
    let cfg = parse_toml(toml_str).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => {
            assert_eq!(command, "/usr/local/bin/node");
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_mock_json_roundtrip() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn backend_sidecar_json_roundtrip() {
    let entry = BackendEntry::Sidecar {
        command: "python3".into(),
        args: vec!["host.py".into(), "--debug".into()],
        timeout_secs: Some(600),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: BackendEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn backend_serde_tag_is_type() {
    let entry = BackendEntry::Mock {};
    let json = serde_json::to_string(&entry).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["type"], "mock");
}

#[test]
fn backend_full_config_toml_roundtrip() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::from([
            ("mock".into(), BackendEntry::Mock {}),
            (
                "sc".into(),
                BackendEntry::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(300),
                },
            ),
        ]),
        ..Default::default()
    };
    let serialized = toml::to_string(&cfg).unwrap();
    let deserialized: BackplaneConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(cfg, deserialized);
}

#[test]
fn backend_example_config_file_parses() {
    let content = include_str!("../backplane.example.toml");
    let cfg: BackplaneConfig = toml::from_str(content).expect("parse example config");
    assert!(!cfg.backends.is_empty());
    assert!(cfg.backends.contains_key("mock"));
}

#[test]
fn backend_config_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backplane.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        r#"
default_backend = "sc"
log_level = "warn"
receipts_dir = "/data/receipts"

[backends.mock]
type = "mock"

[backends.sc]
type = "sidecar"
command = "node"
args = ["host.js"]
timeout_secs = 120
"#
    )
    .unwrap();

    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("sc"));
    assert_eq!(cfg.log_level.as_deref(), Some("warn"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/data/receipts"));
    assert_eq!(cfg.backends.len(), 2);
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
            assert_eq!(*timeout_secs, Some(120));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn backend_load_missing_file_gives_error() {
    let err = load_config(Some(Path::new("/nonexistent/path/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

// =========================================================================
// Extra: CLI config module (abp_cli::config) tests
// =========================================================================

#[test]
fn cli_config_parse_mock_backend() {
    let toml_str = r#"
        [backends.mock]
        type = "mock"
    "#;
    let cfg: abp_cli::config::BackplaneConfig = toml::from_str(toml_str).unwrap();
    assert!(cfg.backends.contains_key("mock"));
}

#[test]
fn cli_config_parse_sidecar_backend() {
    let toml_str = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
        timeout_secs = 300
    "#;
    let cfg: abp_cli::config::BackplaneConfig = toml::from_str(toml_str).unwrap();
    match &cfg.backends["sc"] {
        abp_cli::config::BackendConfig::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn cli_config_validate_catches_empty_command() {
    let cfg = abp_cli::config::BackplaneConfig {
        backends: std::collections::HashMap::from([(
            "bad".into(),
            abp_cli::config::BackendConfig::Sidecar {
                command: "  ".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let errs = abp_cli::config::validate_config(&cfg).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, abp_cli::config::ConfigError::InvalidBackend { .. })));
}

#[test]
fn cli_config_validate_catches_zero_timeout() {
    let cfg = abp_cli::config::BackplaneConfig {
        backends: std::collections::HashMap::from([(
            "sc".into(),
            abp_cli::config::BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        )]),
        ..Default::default()
    };
    let errs = abp_cli::config::validate_config(&cfg).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, abp_cli::config::ConfigError::InvalidTimeout { .. })));
}

#[test]
fn cli_config_validate_valid_passes() {
    let cfg = abp_cli::config::BackplaneConfig {
        backends: std::collections::HashMap::from([
            ("mock".into(), abp_cli::config::BackendConfig::Mock {}),
            (
                "sc".into(),
                abp_cli::config::BackendConfig::Sidecar {
                    command: "node".into(),
                    args: vec!["host.js".into()],
                    timeout_secs: Some(300),
                },
            ),
        ]),
        ..Default::default()
    };
    abp_cli::config::validate_config(&cfg).unwrap();
}

#[test]
fn cli_config_merge_overlays() {
    let base = abp_cli::config::BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        ..Default::default()
    };
    let overlay = abp_cli::config::BackplaneConfig {
        default_backend: Some("openai".into()),
        ..Default::default()
    };
    let merged = abp_cli::config::merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("openai"));
    assert_eq!(merged.log_level.as_deref(), Some("info"));
}

#[test]
fn cli_config_error_display() {
    let e = abp_cli::config::ConfigError::InvalidBackend {
        name: "x".into(),
        reason: "bad".into(),
    };
    assert_eq!(e.to_string(), "invalid backend 'x': bad");

    let e = abp_cli::config::ConfigError::InvalidTimeout { value: 0 };
    assert!(e.to_string().contains("invalid timeout"));

    let e = abp_cli::config::ConfigError::MissingRequiredField {
        field: "name".into(),
    };
    assert!(e.to_string().contains("missing required field"));
}

// =========================================================================
// Extra: ConfigError / ConfigWarning Display coverage
// =========================================================================

#[test]
fn config_error_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/missing.toml".into(),
    };
    let s = e.to_string();
    assert!(s.contains("/missing.toml"));
    assert!(s.contains("not found"));
}

#[test]
fn config_error_parse_error_display() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    let s = e.to_string();
    assert!(s.contains("unexpected token"));
}

#[test]
fn config_error_validation_error_display() {
    let e = ConfigError::ValidationError {
        reasons: vec!["bad field".into(), "timeout issue".into()],
    };
    let s = e.to_string();
    assert!(s.contains("bad field"));
}

#[test]
fn config_error_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflicting backends".into(),
    };
    let s = e.to_string();
    assert!(s.contains("conflicting backends"));
}

#[test]
fn config_warning_deprecated_field_display() {
    let w = ConfigWarning::DeprecatedField {
        field: "old_field".into(),
        suggestion: Some("new_field".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old_field"));
    assert!(s.contains("new_field"));
}

#[test]
fn config_warning_deprecated_field_no_suggestion_display() {
    let w = ConfigWarning::DeprecatedField {
        field: "legacy".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("legacy"));
}

#[test]
fn config_warning_missing_optional_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "receipts_dir".into(),
        hint: "will not persist".into(),
    };
    let s = w.to_string();
    assert!(s.contains("receipts_dir"));
    assert!(s.contains("will not persist"));
}

#[test]
fn config_warning_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "slow".into(),
        secs: 9999,
    };
    let s = w.to_string();
    assert!(s.contains("slow"));
    assert!(s.contains("9999"));
}

// =========================================================================
// Extra: Env override tests
// =========================================================================

#[test]
fn env_override_default_backend() {
    let _guard = EnvGuard::new(&[("ABP_DEFAULT_BACKEND", "env_backend")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("env_backend"));
}

#[test]
fn env_override_log_level() {
    let _guard = EnvGuard::new(&[("ABP_LOG_LEVEL", "trace")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
fn env_override_receipts_dir() {
    let _guard = EnvGuard::new(&[("ABP_RECEIPTS_DIR", "/env/receipts")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
}

#[test]
fn env_override_workspace_dir() {
    let _guard = EnvGuard::new(&[("ABP_WORKSPACE_DIR", "/env/workspace")]);
    let mut cfg = BackplaneConfig::default();
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/workspace"));
}
