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
//! Deep comprehensive tests for `abp-config`.
//!
//! Covers: TOML parsing, defaults/overrides, env var resolution, validation,
//! nested backend sections, merge behaviour, error messages, serialization
//! round-trips, optional/required fields, sidecar config, and edge cases.

use abp_config::*;
use serial_test::serial;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ===========================================================================
// Helpers
// ===========================================================================

/// Fully-populated config that passes validation with zero warnings.
fn full_cfg() -> BackplaneConfig {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "sc".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["host.js".into()],
            timeout_secs: Some(120),
        },
    );
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
        backends,
        ..Default::default()
    }
}

/// Extract reasons from a ValidationError or panic.
fn reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

/// Write a temp TOML file and return its path (inside `dir`).
fn write_toml(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    p
}

// ===========================================================================
// 1. TOML parsing – basic
// ===========================================================================

#[test]
fn parse_empty_string_gives_defaults() {
    let cfg = parse_toml("").unwrap();
    assert_eq!(cfg.default_backend, None);
    assert!(cfg.backends.is_empty());
    assert_eq!(cfg.log_level, None);
}

#[test]
fn parse_minimal_toml() {
    let cfg = parse_toml(r#"default_backend = "m""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("m"));
}

#[test]
fn parse_all_top_level_fields() {
    let t = r#"
        default_backend = "openai"
        workspace_dir   = "/work"
        log_level       = "debug"
        receipts_dir    = "/recv"
    "#;
    let cfg = parse_toml(t).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/recv"));
}

#[test]
fn parse_mock_backend() {
    let t = r#"
        [backends.m]
        type = "mock"
    "#;
    let cfg = parse_toml(t).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn parse_sidecar_backend_full() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "python3"
        args = ["host.py", "--verbose"]
        timeout_secs = 600
    "#;
    let cfg = parse_toml(t).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert_eq!(args, &["host.py", "--verbose"]);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_sidecar_without_optional_fields() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(t).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            args, timeout_secs, ..
        } => {
            assert!(args.is_empty());
            assert_eq!(*timeout_secs, None);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_multiple_backends() {
    let t = r#"
        [backends.mock1]
        type = "mock"
        [backends.mock2]
        type = "mock"
        [backends.sc1]
        type = "sidecar"
        command = "node"
        [backends.sc2]
        type = "sidecar"
        command = "python"
        args = ["h.py"]
        timeout_secs = 30
    "#;
    let cfg = parse_toml(t).unwrap();
    assert_eq!(cfg.backends.len(), 4);
}

// ===========================================================================
// 2. TOML parsing – error cases
// ===========================================================================

#[test]
fn parse_garbage_gives_parse_error() {
    let err = parse_toml("{{{{not toml}}}}").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_default_backend() {
    let err = parse_toml("default_backend = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_log_level() {
    let err = parse_toml("log_level = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_wrong_type_for_backends() {
    let err = parse_toml("backends = 99").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_backend_missing_type_discriminator() {
    let t = r#"
        [backends.bad]
        command = "node"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn parse_backend_unknown_type_discriminator() {
    let t = r#"
        [backends.bad]
        type = "openai_native"
        command = "node"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn parse_sidecar_missing_command_field() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        args = ["x"]
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn parse_sidecar_wrong_type_for_command() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        command = 123
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn parse_sidecar_wrong_type_for_args() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        command = "node"
        args = "not_an_array"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn parse_sidecar_wrong_type_for_timeout() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        command = "node"
        timeout_secs = "thirty"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn parse_error_display_contains_detail() {
    let err = parse_toml("{{bad}}").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to parse config"));
}

// ===========================================================================
// 3. Config defaults
// ===========================================================================

#[test]
fn default_config_log_level_is_info() {
    assert_eq!(
        BackplaneConfig::default().log_level.as_deref(),
        Some("info")
    );
}

#[test]
fn default_config_no_default_backend() {
    assert!(BackplaneConfig::default().default_backend.is_none());
}

#[test]
fn default_config_no_workspace_dir() {
    assert!(BackplaneConfig::default().workspace_dir.is_none());
}

#[test]
fn default_config_no_receipts_dir() {
    assert!(BackplaneConfig::default().receipts_dir.is_none());
}

#[test]
fn default_config_empty_backends() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn default_config_is_valid() {
    let w = validate_config(&BackplaneConfig::default()).unwrap();
    // Has advisory warnings (missing optional fields) but no hard errors.
    assert!(!w.is_empty());
}

// ===========================================================================
// 4. Validation – valid configs
// ===========================================================================

#[test]
fn full_config_zero_warnings() {
    let w = validate_config(&full_cfg()).unwrap();
    assert!(w.is_empty(), "unexpected warnings: {w:?}");
}

#[test]
fn valid_log_level_error() {
    let mut c = full_cfg();
    c.log_level = Some("error".into());
    validate_config(&c).unwrap();
}

#[test]
fn valid_log_level_warn() {
    let mut c = full_cfg();
    c.log_level = Some("warn".into());
    validate_config(&c).unwrap();
}

#[test]
fn valid_log_level_debug() {
    let mut c = full_cfg();
    c.log_level = Some("debug".into());
    validate_config(&c).unwrap();
}

#[test]
fn valid_log_level_trace() {
    let mut c = full_cfg();
    c.log_level = Some("trace".into());
    validate_config(&c).unwrap();
}

#[test]
fn valid_log_level_none() {
    let mut c = full_cfg();
    c.log_level = None;
    validate_config(&c).unwrap();
}

#[test]
fn sidecar_timeout_1s_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "x".into(),
            args: vec![],
            timeout_secs: Some(1),
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn sidecar_timeout_max_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "x".into(),
            args: vec![],
            timeout_secs: Some(86_400),
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn many_mock_backends_valid() {
    let mut c = full_cfg();
    for i in 0..50 {
        c.backends.insert(format!("m{i}"), BackendEntry::Mock {});
    }
    validate_config(&c).unwrap();
}

// ===========================================================================
// 5. Validation – hard errors
// ===========================================================================

#[test]
fn invalid_log_level_verbose() {
    let mut c = full_cfg();
    c.log_level = Some("verbose".into());
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("invalid log_level")));
}

#[test]
fn invalid_log_level_uppercase() {
    let mut c = full_cfg();
    c.log_level = Some("INFO".into());
    assert!(validate_config(&c).is_err());
}

#[test]
fn invalid_log_level_empty_string() {
    let mut c = full_cfg();
    c.log_level = Some(String::new());
    assert!(validate_config(&c).is_err());
}

#[test]
fn invalid_log_level_mixed_case() {
    let mut c = full_cfg();
    c.log_level = Some("Debug".into());
    assert!(validate_config(&c).is_err());
}

#[test]
fn invalid_log_level_with_whitespace() {
    let mut c = full_cfg();
    c.log_level = Some(" info ".into());
    assert!(validate_config(&c).is_err());
}

#[test]
fn empty_sidecar_command_error() {
    let mut c = full_cfg();
    c.backends.insert(
        "bad".into(),
        BackendEntry::Sidecar {
            command: String::new(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("command must not be empty")));
}

#[test]
fn whitespace_only_command_error() {
    let mut c = full_cfg();
    c.backends.insert(
        "ws".into(),
        BackendEntry::Sidecar {
            command: "   \t\n".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    assert!(validate_config(&c).is_err());
}

#[test]
fn zero_timeout_error() {
    let mut c = full_cfg();
    c.backends.insert(
        "z".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("out of range")));
}

#[test]
fn timeout_just_over_max_error() {
    let mut c = full_cfg();
    c.backends.insert(
        "over".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(86_401),
        },
    );
    assert!(validate_config(&c).is_err());
}

#[test]
fn timeout_u64_max_error() {
    let mut c = full_cfg();
    c.backends.insert(
        "huge".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(u64::MAX),
        },
    );
    assert!(validate_config(&c).is_err());
}

#[test]
fn empty_backend_name_error() {
    let mut c = full_cfg();
    c.backends.insert("".into(), BackendEntry::Mock {});
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("name must not be empty")));
}

#[test]
fn multiple_validation_errors_collected() {
    let mut c = BackplaneConfig {
        log_level: Some("BAD".into()),
        default_backend: Some("x".into()),
        receipts_dir: Some("/r".into()),
        ..Default::default()
    };
    c.backends.insert(
        "a".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: Some(0),
        },
    );
    c.backends.insert(
        "b".into(),
        BackendEntry::Sidecar {
            command: "  ".into(),
            args: vec![],
            timeout_secs: Some(999_999),
        },
    );
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.len() >= 5, "expected >=5 errors, got {}: {r:?}", r.len());
}

#[test]
fn error_message_references_backend_name() {
    let mut c = full_cfg();
    c.backends.insert(
        "my_broken_sc".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("my_broken_sc")));
}

#[test]
fn timeout_error_message_shows_value() {
    let mut c = full_cfg();
    c.backends.insert(
        "t".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(100_000),
        },
    );
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("100000")));
}

// ===========================================================================
// 6. Validation – advisory warnings
// ===========================================================================

#[test]
fn missing_default_backend_warning() {
    let mut c = full_cfg();
    c.default_backend = None;
    let w = validate_config(&c).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn missing_receipts_dir_warning() {
    let mut c = full_cfg();
    c.receipts_dir = None;
    let w = validate_config(&c).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn both_optional_missing_gives_two_warnings() {
    let mut c = full_cfg();
    c.default_backend = None;
    c.receipts_dir = None;
    let w = validate_config(&c).unwrap();
    let count = w
        .iter()
        .filter(|w| matches!(w, ConfigWarning::MissingOptionalField { .. }))
        .count();
    assert_eq!(count, 2);
}

#[test]
fn large_timeout_above_threshold_warns() {
    let mut c = full_cfg();
    c.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_601),
        },
    );
    let w = validate_config(&c).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, secs } if backend == "big" && *secs == 3_601
    )));
}

#[test]
fn timeout_exactly_at_threshold_no_warning() {
    let mut c = full_cfg();
    c.backends.insert(
        "exact".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_600),
        },
    );
    let w = validate_config(&c).unwrap();
    assert!(!w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "exact"
    )));
}

#[test]
fn timeout_below_threshold_no_warning() {
    let mut c = full_cfg();
    c.backends.insert(
        "below".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(3_599),
        },
    );
    let w = validate_config(&c).unwrap();
    assert!(!w.iter().any(|w| matches!(
        w,
        ConfigWarning::LargeTimeout { backend, .. } if backend == "below"
    )));
}

#[test]
fn multiple_large_timeouts_produce_multiple_warnings() {
    let mut c = full_cfg();
    c.backends.insert(
        "big1".into(),
        BackendEntry::Sidecar {
            command: "a".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    c.backends.insert(
        "big2".into(),
        BackendEntry::Sidecar {
            command: "b".into(),
            args: vec![],
            timeout_secs: Some(43_200),
        },
    );
    let w = validate_config(&c).unwrap();
    let lt = w
        .iter()
        .filter(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
        .count();
    assert_eq!(lt, 2);
}

// ===========================================================================
// 7. ConfigWarning Display
// ===========================================================================

#[test]
fn deprecated_field_display_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: Some("new".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old") && s.contains("new"));
}

#[test]
fn deprecated_field_display_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "x".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("x"));
    assert!(!s.contains("instead"));
}

#[test]
fn missing_optional_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "f".into(),
        hint: "h".into(),
    };
    assert!(w.to_string().contains('f') && w.to_string().contains('h'));
}

#[test]
fn large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 9999,
    };
    let s = w.to_string();
    assert!(s.contains("sc") && s.contains("9999"));
}

// ===========================================================================
// 8. ConfigError Display
// ===========================================================================

#[test]
fn file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/foo/bar.toml".into(),
    };
    assert!(e.to_string().contains("/foo/bar.toml"));
}

#[test]
fn parse_error_display() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    assert!(e.to_string().contains("unexpected token"));
}

#[test]
fn validation_error_display_all_reasons() {
    let e = ConfigError::ValidationError {
        reasons: vec!["one".into(), "two".into()],
    };
    let s = e.to_string();
    assert!(s.contains("one") && s.contains("two"));
}

#[test]
fn merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflict!".into(),
    };
    assert!(e.to_string().contains("conflict!"));
}

// ===========================================================================
// 9. Config merging
// ===========================================================================

#[test]
fn merge_overlay_wins_for_default_backend() {
    let base = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("b".into()),
        ..Default::default()
    };
    assert_eq!(
        merge_configs(base, overlay).default_backend.as_deref(),
        Some("b")
    );
}

#[test]
fn merge_overlay_wins_for_workspace_dir() {
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
fn merge_overlay_wins_for_log_level() {
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
fn merge_overlay_wins_for_receipts_dir() {
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
fn merge_base_preserved_when_overlay_none() {
    let base = BackplaneConfig {
        default_backend: Some("x".into()),
        workspace_dir: Some("/w".into()),
        log_level: Some("trace".into()),
        receipts_dir: Some("/r".into()),
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
    let m = merge_configs(base, overlay);
    assert_eq!(m.default_backend.as_deref(), Some("x"));
    assert_eq!(m.workspace_dir.as_deref(), Some("/w"));
    assert_eq!(m.log_level.as_deref(), Some("trace"));
    assert_eq!(m.receipts_dir.as_deref(), Some("/r"));
    assert!(m.backends.contains_key("m"));
}

#[test]
fn merge_combines_distinct_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert!(m.backends.contains_key("a"));
    assert!(m.backends.contains_key("b"));
}

#[test]
fn merge_overlay_backend_wins_on_collision() {
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
                args: vec!["h.js".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    match &merge_configs(base, overlay).backends["sc"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["h.js"]);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_three_layers() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let c = BackplaneConfig {
        receipts_dir: Some("/r".into()),
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let m = merge_configs(merge_configs(a, b), c);
    assert_eq!(m.default_backend.as_deref(), Some("a"));
    assert_eq!(m.log_level.as_deref(), Some("debug"));
    assert_eq!(m.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn merge_default_overlay_preserves_base_log_level() {
    // Default has log_level = Some("info"), which will override base.
    let base = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let m = merge_configs(base, BackplaneConfig::default());
    // overlay.log_level = Some("info"), so it wins.
    assert_eq!(m.log_level.as_deref(), Some("info"));
}

#[test]
fn merge_both_none_stays_none() {
    let a = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let b = a.clone();
    let m = merge_configs(a, b);
    assert!(m.default_backend.is_none());
    assert!(m.workspace_dir.is_none());
    assert!(m.log_level.is_none());
    assert!(m.receipts_dir.is_none());
}

#[test]
fn merge_overlay_replaces_mock_with_sidecar() {
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
    assert!(matches!(
        merge_configs(base, overlay).backends["x"],
        BackendEntry::Sidecar { .. }
    ));
}

#[test]
fn merge_overlay_fixes_bad_backend() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..full_cfg()
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
    validate_config(&merge_configs(base, overlay)).unwrap();
}

#[test]
fn merge_introduces_invalid_log_level() {
    let base = full_cfg();
    let overlay = BackplaneConfig {
        log_level: Some("banana".into()),
        ..Default::default()
    };
    assert!(validate_config(&merge_configs(base, overlay)).is_err());
}

// ===========================================================================
// 10. Serialization round-trips
// ===========================================================================

#[test]
fn toml_roundtrip_full_config() {
    let c = full_cfg();
    let s = toml::to_string(&c).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(c, d);
}

#[test]
fn toml_roundtrip_default_config() {
    let c = BackplaneConfig::default();
    let s = toml::to_string(&c).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(c, d);
}

#[test]
fn json_roundtrip_full_config() {
    let c = full_cfg();
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c, d);
}

#[test]
fn json_roundtrip_default_config() {
    let c = BackplaneConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c, d);
}

#[test]
fn toml_roundtrip_preserves_validation() {
    let c = full_cfg();
    validate_config(&c).unwrap();
    let s = toml::to_string(&c).unwrap();
    let d = parse_toml(&s).unwrap();
    validate_config(&d).unwrap();
}

#[test]
fn json_roundtrip_preserves_validation() {
    let c = full_cfg();
    validate_config(&c).unwrap();
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    validate_config(&d).unwrap();
}

#[test]
fn toml_serialization_skips_none_fields() {
    let c = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let s = toml::to_string(&c).unwrap();
    assert!(!s.contains("default_backend"));
    assert!(!s.contains("workspace_dir"));
    assert!(!s.contains("log_level"));
    assert!(!s.contains("receipts_dir"));
}

#[test]
fn json_serialization_skips_none_fields() {
    let c = BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let j = serde_json::to_string(&c).unwrap();
    assert!(!j.contains("default_backend"));
    assert!(!j.contains("workspace_dir"));
    assert!(!j.contains("log_level"));
    assert!(!j.contains("receipts_dir"));
}

#[test]
fn toml_roundtrip_sidecar_with_args() {
    let mut c = full_cfg();
    c.backends.insert(
        "complex".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec![
                "--host".into(),
                "0.0.0.0".into(),
                "--port".into(),
                "8080".into(),
            ],
            timeout_secs: Some(300),
        },
    );
    let s = toml::to_string(&c).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(c, d);
}

#[test]
fn json_schema_can_be_generated() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let j = serde_json::to_string_pretty(&schema).unwrap();
    assert!(j.contains("BackplaneConfig"));
}

// ===========================================================================
// 11. load_config – file I/O
// ===========================================================================

#[test]
fn load_none_returns_default_with_env() {
    let c = load_config(None).unwrap();
    // log_level may have been overridden by env, but is at least Some.
    assert!(c.log_level.is_some() || std::env::var("ABP_LOG_LEVEL").is_ok());
}

#[test]
fn load_missing_file_gives_file_not_found() {
    let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_from_file_with_backends() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(
        dir.path(),
        "bp.toml",
        r#"
        default_backend = "mock"
        log_level = "warn"
        [backends.mock]
        type = "mock"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["h.js"]
        timeout_secs = 60
    "#,
    );
    let c = load_config(Some(&p)).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
    assert_eq!(c.backends.len(), 2);
}

#[test]
fn load_empty_file_gives_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "empty.toml", "");
    let c = load_config(Some(&p)).unwrap();
    assert!(c.backends.is_empty());
}

#[test]
fn load_file_with_only_backends() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(
        dir.path(),
        "be.toml",
        r#"
        [backends.mock]
        type = "mock"
    "#,
    );
    let c = load_config(Some(&p)).unwrap();
    assert!(c.default_backend.is_none());
    assert_eq!(c.backends.len(), 1);
}

#[test]
fn load_invalid_toml_file_gives_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "bad.toml", "not = [valid toml");
    let err = load_config(Some(&p)).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// ===========================================================================
// 12. Environment variable overrides
// ===========================================================================
// Use unique env var names (via _320_ prefix) per test to avoid interference.

#[test]
#[serial]
fn env_override_default_backend() {
    let mut c = BackplaneConfig::default();
    // Manually set env, apply, unset.
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "from_env_320_1") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.default_backend.as_deref(), Some("from_env_320_1"));
}

#[test]
#[serial]
fn env_override_log_level() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace_320_2") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    assert_eq!(c.log_level.as_deref(), Some("trace_320_2"));
}

#[test]
#[serial]
fn env_override_receipts_dir() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_RECEIPTS_DIR", "/recv_320_3") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") }
    assert_eq!(c.receipts_dir.as_deref(), Some("/recv_320_3"));
}

#[test]
#[serial]
fn env_override_workspace_dir() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/ws_320_4") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") }
    assert_eq!(c.workspace_dir.as_deref(), Some("/ws_320_4"));
}

#[test]
#[serial]
fn env_override_replaces_existing_value() {
    let mut c = BackplaneConfig {
        default_backend: Some("old".into()),
        ..Default::default()
    };
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "new_320_5") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.default_backend.as_deref(), Some("new_320_5"));
}

#[test]
#[serial]
fn env_override_does_not_touch_unset_vars() {
    // Ensure none of the env vars are set.
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") }
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") }
    let mut c = BackplaneConfig {
        default_backend: Some("keep".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/r".into()),
        workspace_dir: Some("/w".into()),
        backends: BTreeMap::new(),
        ..Default::default()
    };
    apply_env_overrides(&mut c);
    assert_eq!(c.default_backend.as_deref(), Some("keep"));
    assert_eq!(c.log_level.as_deref(), Some("info"));
    assert_eq!(c.receipts_dir.as_deref(), Some("/r"));
    assert_eq!(c.workspace_dir.as_deref(), Some("/w"));
}

#[test]
#[serial]
fn env_override_empty_string_is_set() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    // Empty string is still Some("").
    assert_eq!(c.default_backend.as_deref(), Some(""));
}

#[test]
#[serial]
fn env_override_does_not_affect_backends() {
    let mut c = full_cfg();
    let before = c.backends.len();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "env_320_6") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.backends.len(), before);
}

// ===========================================================================
// 13. Config merge + env integration
// ===========================================================================

#[test]
#[serial]
fn file_plus_env_plus_defaults_merge() {
    // Simulate: defaults → file → env
    let file_cfg = BackplaneConfig {
        default_backend: Some("file_be".into()),
        log_level: Some("warn".into()),
        ..Default::default()
    };
    let merged = merge_configs(BackplaneConfig::default(), file_cfg);
    // Then env override on top
    let mut final_cfg = merged;
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "error") }
    apply_env_overrides(&mut final_cfg);
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    assert_eq!(final_cfg.default_backend.as_deref(), Some("file_be"));
    assert_eq!(final_cfg.log_level.as_deref(), Some("error"));
}

// ===========================================================================
// 14. Validation idempotency
// ===========================================================================

#[test]
fn validate_twice_same_result_valid() {
    let c = full_cfg();
    let w1 = validate_config(&c).unwrap();
    let w2 = validate_config(&c).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn validate_twice_same_result_invalid() {
    let c = BackplaneConfig {
        log_level: Some("nope".into()),
        ..full_cfg()
    };
    let r1 = reasons(validate_config(&c).unwrap_err());
    let r2 = reasons(validate_config(&c).unwrap_err());
    assert_eq!(r1, r2);
}

#[test]
fn validate_twice_same_warnings() {
    let mut c = full_cfg();
    c.default_backend = None;
    c.backends.insert(
        "big".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: Some(7_200),
        },
    );
    let w1 = validate_config(&c).unwrap();
    let w2 = validate_config(&c).unwrap();
    assert_eq!(w1, w2);
}

// ===========================================================================
// 15. Edge cases – strings and special values
// ===========================================================================

#[test]
fn very_long_backend_name_valid() {
    let mut c = full_cfg();
    c.backends.insert("a".repeat(10_000), BackendEntry::Mock {});
    validate_config(&c).unwrap();
}

#[test]
fn very_long_command_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "long".into(),
        BackendEntry::Sidecar {
            command: "x".repeat(100_000),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn unicode_in_backend_name_and_command() {
    let mut c = full_cfg();
    c.backends.insert(
        "日本語".into(),
        BackendEntry::Sidecar {
            command: "nöde".into(),
            args: vec!["—arg".into(), "ñ".into()],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn special_chars_in_paths() {
    let c = BackplaneConfig {
        workspace_dir: Some("/tmp/agent (copy)/ws!/@#$%".into()),
        receipts_dir: Some(r"C:\Users\日本語\receipts".into()),
        ..full_cfg()
    };
    validate_config(&c).unwrap();
}

#[test]
fn backend_name_with_dots_and_dashes() {
    let mut c = full_cfg();
    c.backends
        .insert("my-backend_v2.0".into(), BackendEntry::Mock {});
    validate_config(&c).unwrap();
}

#[test]
fn empty_args_vec_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "empty_args".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn args_with_empty_strings() {
    let mut c = full_cfg();
    c.backends.insert(
        "empty_arg_items".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["".into(), "".into()],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn many_args() {
    let mut c = full_cfg();
    c.backends.insert(
        "many_args".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: (0..100).map(|i| format!("--arg{i}")).collect(),
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

// ===========================================================================
// 16. Sidecar config parsing – example TOML format
// ===========================================================================

#[test]
fn parse_example_toml_format() {
    let t = r#"
        default_backend = "mock"
        log_level = "info"
        receipts_dir = "./data/receipts"

        [backends.mock]
        type = "mock"

        [backends.openai]
        type = "sidecar"
        command = "node"
        args = ["path/to/openai-sidecar.js"]
        timeout_secs = 300

        [backends.anthropic]
        type = "sidecar"
        command = "python3"
        args = ["path/to/anthropic-sidecar.py"]
        timeout_secs = 600
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
    assert_eq!(c.backends.len(), 3);
    assert!(matches!(c.backends["mock"], BackendEntry::Mock {}));
    match &c.backends["openai"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["path/to/openai-sidecar.js"]);
            assert_eq!(*timeout_secs, Some(300));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
    match &c.backends["anthropic"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert_eq!(args, &["path/to/anthropic-sidecar.py"]);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_sidecar_empty_args_array() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = []
    "#;
    let c = parse_toml(t).unwrap();
    match &c.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn parse_sidecar_many_args() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["--a", "--b", "--c", "--d", "--e"]
    "#;
    let c = parse_toml(t).unwrap();
    match &c.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert_eq!(args.len(), 5),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// ===========================================================================
// 17. TOML ignores unknown fields (serde default)
// ===========================================================================

#[test]
fn unknown_top_level_fields_ignored() {
    let t = r#"
        default_backend = "mock"
        unknown_field = "hello"
        another_one = 42
    "#;
    // toml::from_str with deny_unknown_fields would fail, but our struct
    // uses default serde which ignores unknown fields.
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
}

// ===========================================================================
// 18. BackendEntry discriminator tag is "type"
// ===========================================================================

#[test]
fn backend_entry_tagged_with_type() {
    let j = serde_json::to_string(&BackendEntry::Mock {}).unwrap();
    assert!(j.contains(r#""type":"mock""#));
}

#[test]
fn backend_entry_sidecar_tag() {
    let e = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let j = serde_json::to_string(&e).unwrap();
    assert!(j.contains(r#""type":"sidecar""#));
}

// ===========================================================================
// 19. PartialEq / Clone / Debug derive checks
// ===========================================================================

#[test]
fn config_clone_equals_original() {
    let c = full_cfg();
    let c2 = c.clone();
    assert_eq!(c, c2);
}

#[test]
fn config_debug_not_empty() {
    let c = full_cfg();
    let dbg = format!("{c:?}");
    assert!(!dbg.is_empty());
    assert!(dbg.contains("BackplaneConfig"));
}

#[test]
fn backend_entry_clone_equals() {
    let e = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["a".into()],
        timeout_secs: Some(60),
    };
    assert_eq!(e, e.clone());
}

#[test]
fn backend_entry_mock_clone_equals() {
    let e = BackendEntry::Mock {};
    assert_eq!(e, e.clone());
}

#[test]
fn config_ne_when_different() {
    let a = full_cfg();
    let mut b = full_cfg();
    b.log_level = Some("debug".into());
    assert_ne!(a, b);
}

// ===========================================================================
// 20. BTreeMap ordering – deterministic serialization
// ===========================================================================

#[test]
fn backends_serialize_in_alphabetical_order() {
    let mut c = full_cfg();
    c.backends.insert("zzz".into(), BackendEntry::Mock {});
    c.backends.insert("aaa".into(), BackendEntry::Mock {});
    let s = toml::to_string(&c).unwrap();
    let pos_aaa = s.find("[backends.aaa]").unwrap();
    let pos_zzz = s.find("[backends.zzz]").unwrap();
    assert!(pos_aaa < pos_zzz);
}

// ===========================================================================
// 21. Config with all optional fields set
// ===========================================================================

#[test]
fn all_optional_fields_set_parses_and_validates() {
    let t = r#"
        default_backend = "mock"
        workspace_dir = "/ws"
        log_level = "debug"
        receipts_dir = "/r"

        [backends.mock]
        type = "mock"

        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
        timeout_secs = 120
    "#;
    let c = parse_toml(t).unwrap();
    assert!(c.default_backend.is_some());
    assert!(c.workspace_dir.is_some());
    assert!(c.log_level.is_some());
    assert!(c.receipts_dir.is_some());
    let w = validate_config(&c).unwrap();
    assert!(w.is_empty());
}

// ===========================================================================
// 22. Config with minimal required fields (empty is valid structurally)
// ===========================================================================

#[test]
fn minimal_config_has_no_hard_errors() {
    let c = parse_toml("").unwrap();
    // No hard errors, but advisory warnings.
    let w = validate_config(&c).unwrap();
    assert!(!w.is_empty());
}

#[test]
fn minimal_config_warnings_are_advisory() {
    let c = parse_toml("").unwrap();
    let w = validate_config(&c).unwrap();
    for warning in &w {
        assert!(matches!(
            warning,
            ConfigWarning::MissingOptionalField { .. }
        ));
    }
}

// ===========================================================================
// 23. load_config applies env after file
// ===========================================================================

#[test]
#[serial]
fn load_config_env_overrides_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "bp.toml", r#"log_level = "debug""#);
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace") }
    let c = load_config(Some(&p)).unwrap();
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    assert_eq!(c.log_level.as_deref(), Some("trace"));
}

// ===========================================================================
// 24. ConfigWarning PartialEq / Eq / Clone
// ===========================================================================

#[test]
fn config_warning_clone_eq() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    assert_eq!(w, w.clone());
}

#[test]
fn config_warning_ne() {
    let a = ConfigWarning::LargeTimeout {
        backend: "a".into(),
        secs: 100,
    };
    let b = ConfigWarning::LargeTimeout {
        backend: "b".into(),
        secs: 200,
    };
    assert_ne!(a, b);
}

#[test]
fn config_warning_debug() {
    let w = ConfigWarning::DeprecatedField {
        field: "x".into(),
        suggestion: None,
    };
    let d = format!("{w:?}");
    assert!(d.contains("DeprecatedField"));
}

// ===========================================================================
// 25. Multiline TOML values
// ===========================================================================

#[test]
fn toml_multiline_string_in_workspace_dir() {
    let t = "workspace_dir = '''\n/tmp/\nmultiline\n'''";
    let c = parse_toml(t).unwrap();
    assert!(c.workspace_dir.unwrap().contains("multiline"));
}

// ===========================================================================
// 26. TOML with inline tables
// ===========================================================================

#[test]
fn inline_backend_table() {
    // TOML inline table syntax isn't typically used for tagged enums in TOML,
    // but the regular dotted format works.
    let t = r#"
        [backends]
        [backends.m]
        type = "mock"
    "#;
    let c = parse_toml(t).unwrap();
    assert!(matches!(c.backends["m"], BackendEntry::Mock {}));
}

// ===========================================================================
// 27. Concurrent-safe: each test creates its own config
// ===========================================================================

#[test]
fn independent_config_instances() {
    let c1 = full_cfg();
    let mut c2 = full_cfg();
    c2.log_level = Some("trace".into());
    assert_ne!(c1, c2);
    // c1 not affected.
    assert_eq!(c1.log_level.as_deref(), Some("info"));
}

// ===========================================================================
// 28. Backend-specific config sections
// ===========================================================================

#[test]
fn only_mock_backends_config() {
    let t = r#"
        [backends.m1]
        type = "mock"
        [backends.m2]
        type = "mock"
        [backends.m3]
        type = "mock"
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.backends.len(), 3);
    for v in c.backends.values() {
        assert!(matches!(v, BackendEntry::Mock {}));
    }
}

#[test]
fn only_sidecar_backends_config() {
    let t = r#"
        [backends.s1]
        type = "sidecar"
        command = "node"
        [backends.s2]
        type = "sidecar"
        command = "python"
        args = ["h.py"]
        timeout_secs = 60
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.backends.len(), 2);
    for v in c.backends.values() {
        assert!(matches!(v, BackendEntry::Sidecar { .. }));
    }
}

#[test]
fn mixed_backends_config() {
    let t = r#"
        [backends.mock]
        type = "mock"
        [backends.node_sc]
        type = "sidecar"
        command = "node"
        args = ["host.js"]
        [backends.py_sc]
        type = "sidecar"
        command = "python3"
        timeout_secs = 120
    "#;
    let c = parse_toml(t).unwrap();
    assert!(matches!(c.backends["mock"], BackendEntry::Mock {}));
    assert!(matches!(
        c.backends["node_sc"],
        BackendEntry::Sidecar { .. }
    ));
    assert!(matches!(c.backends["py_sc"], BackendEntry::Sidecar { .. }));
}

// ===========================================================================
// 29. Validation does not mutate config
// ===========================================================================

#[test]
fn validate_does_not_mutate() {
    let c = full_cfg();
    let before = c.clone();
    validate_config(&c).unwrap();
    assert_eq!(c, before);
}

#[test]
fn validate_invalid_does_not_mutate() {
    let c = BackplaneConfig {
        log_level: Some("bad".into()),
        ..full_cfg()
    };
    let before = c.clone();
    let _ = validate_config(&c);
    assert_eq!(c, before);
}

// ===========================================================================
// 30. Merge is not commutative
// ===========================================================================

#[test]
fn merge_is_not_commutative() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        log_level: None,
        ..Default::default()
    };
    let b = BackplaneConfig {
        default_backend: Some("b".into()),
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let ab = merge_configs(a.clone(), b.clone());
    let ba = merge_configs(b, a);
    // overlay wins, so ab.default_backend = "b", ba.default_backend = "a"
    assert_ne!(ab.default_backend, ba.default_backend);
}

// ===========================================================================
// 31. Merge with empty backends map
// ===========================================================================

#[test]
fn merge_empty_backends_preserved() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert!(m.backends.contains_key("m"));
}

// ===========================================================================
// 32. ConfigError is Debug
// ===========================================================================

#[test]
fn config_error_debug() {
    let e = ConfigError::FileNotFound { path: "/p".into() };
    let d = format!("{e:?}");
    assert!(d.contains("FileNotFound"));
}

#[test]
fn parse_error_debug() {
    let e = ConfigError::ParseError { reason: "x".into() };
    assert!(format!("{e:?}").contains("ParseError"));
}

#[test]
fn validation_error_debug() {
    let e = ConfigError::ValidationError {
        reasons: vec!["r".into()],
    };
    assert!(format!("{e:?}").contains("ValidationError"));
}

#[test]
fn merge_conflict_debug() {
    let e = ConfigError::MergeConflict { reason: "c".into() };
    assert!(format!("{e:?}").contains("MergeConflict"));
}

// ===========================================================================
// 33. TOML with comments (should be ignored)
// ===========================================================================

#[test]
fn toml_comments_ignored() {
    let t = r#"
        # This is a comment
        default_backend = "mock" # inline comment
        # log_level = "trace"  -- commented out
        log_level = "info"

        [backends.mock] # backend section
        type = "mock"
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
    assert_eq!(c.log_level.as_deref(), Some("info"));
}

// ===========================================================================
// 34. TOML whitespace tolerance
// ===========================================================================

#[test]
fn toml_extra_whitespace() {
    let t = "  default_backend  =  \"mock\"  \n\n\n  log_level = \"debug\"  ";
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
    assert_eq!(c.log_level.as_deref(), Some("debug"));
}

// ===========================================================================
// 35. Backend entry JSON roundtrip (tagged enum)
// ===========================================================================

#[test]
fn mock_entry_json_roundtrip() {
    let e = BackendEntry::Mock {};
    let j = serde_json::to_string(&e).unwrap();
    let d: BackendEntry = serde_json::from_str(&j).unwrap();
    assert_eq!(e, d);
}

#[test]
fn sidecar_entry_json_roundtrip() {
    let e = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["--flag".into(), "arg".into()],
        timeout_secs: Some(300),
    };
    let j = serde_json::to_string(&e).unwrap();
    let d: BackendEntry = serde_json::from_str(&j).unwrap();
    assert_eq!(e, d);
}

#[test]
fn sidecar_entry_no_timeout_json_roundtrip() {
    let e = BackendEntry::Sidecar {
        command: "python".into(),
        args: vec![],
        timeout_secs: None,
    };
    let j = serde_json::to_string(&e).unwrap();
    let d: BackendEntry = serde_json::from_str(&j).unwrap();
    assert_eq!(e, d);
}

// ===========================================================================
// 36. Validate after merge preserves correctness
// ===========================================================================

#[test]
fn merge_two_valid_configs_still_valid() {
    let a = full_cfg();
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        backends: BTreeMap::from([(
            "extra".into(),
            BackendEntry::Sidecar {
                command: "go".into(),
                args: vec!["run".into(), "main.go".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    let m = merge_configs(a, b);
    validate_config(&m).unwrap();
}

// ===========================================================================
// 37. Sidecar command with path separators
// ===========================================================================

#[test]
fn sidecar_command_unix_path() {
    let mut c = full_cfg();
    c.backends.insert(
        "unix".into(),
        BackendEntry::Sidecar {
            command: "/usr/local/bin/python3".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn sidecar_command_windows_path() {
    let mut c = full_cfg();
    c.backends.insert(
        "win".into(),
        BackendEntry::Sidecar {
            command: r"C:\Python39\python.exe".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

// ===========================================================================
// 38. Default config → serialize → parse → validate chain
// ===========================================================================

#[test]
fn default_serialize_parse_validate_chain() {
    let c = BackplaneConfig::default();
    let s = toml::to_string(&c).unwrap();
    let d = parse_toml(&s).unwrap();
    validate_config(&d).unwrap();
}

// ===========================================================================
// 39. Config with leading whitespace command is valid (trim check)
// ===========================================================================

#[test]
fn leading_whitespace_command_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "sp".into(),
        BackendEntry::Sidecar {
            command: "  node".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

#[test]
fn trailing_whitespace_command_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "sp".into(),
        BackendEntry::Sidecar {
            command: "node  ".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    validate_config(&c).unwrap();
}

// ===========================================================================
// 40. Error variant matching
// ===========================================================================

#[test]
fn file_not_found_is_not_parse_error() {
    let e = ConfigError::FileNotFound { path: "/x".into() };
    assert!(!matches!(e, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_is_not_validation_error() {
    let e = ConfigError::ParseError { reason: "x".into() };
    assert!(!matches!(e, ConfigError::ValidationError { .. }));
}
