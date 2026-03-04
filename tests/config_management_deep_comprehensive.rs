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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for config management in `abp-config`.
//!
//! 150+ tests covering: TOML parsing/loading, config merging hierarchy,
//! validation (required fields, valid values), env-var overrides (ABP_ prefix),
//! backend configuration selection, sidecar backend config, default values,
//! serialization/deserialization round-trips, error handling, config sections,
//! reload patterns, template generation, version migration stubs, nested config
//! value access, and vendor-specific extensions.

use abp_config::*;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

// ===========================================================================
// Helpers
// ===========================================================================

/// A fully-populated config that passes validation with zero warnings.
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

/// Extract reasons from a `ValidationError` or panic.
fn reasons(err: ConfigError) -> Vec<String> {
    match err {
        ConfigError::ValidationError { reasons } => reasons,
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

/// Write a temp TOML file inside `dir` and return its path.
fn write_toml(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    p
}

// ===========================================================================
// 1. TOML config file parsing and loading (basic)
// ===========================================================================

#[test]
fn t01_parse_empty_string_yields_none_fields() {
    let cfg = parse_toml("").unwrap();
    assert_eq!(cfg.default_backend, None);
    assert_eq!(cfg.workspace_dir, None);
    assert_eq!(cfg.log_level, None);
    assert_eq!(cfg.receipts_dir, None);
    assert!(cfg.backends.is_empty());
}

#[test]
fn t02_parse_only_default_backend() {
    let cfg = parse_toml(r#"default_backend = "openai""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("openai"));
}

#[test]
fn t03_parse_all_scalar_fields() {
    let t = r#"
        default_backend = "mock"
        workspace_dir   = "/work"
        log_level       = "debug"
        receipts_dir    = "/recv"
    "#;
    let cfg = parse_toml(t).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/work"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/recv"));
}

#[test]
fn t04_parse_mock_backend_section() {
    let cfg = parse_toml("[backends.m]\ntype = \"mock\"").unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn t05_parse_sidecar_backend_full() {
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
fn t06_parse_sidecar_without_optional_timeout() {
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
fn t07_parse_multiple_backends_at_once() {
    let t = r#"
        [backends.m1]
        type = "mock"
        [backends.m2]
        type = "mock"
        [backends.sc1]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(t).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

#[test]
fn t08_load_none_path_returns_default() {
    let cfg = load_config(None).unwrap();
    // log_level defaults to "info" unless ABP_LOG_LEVEL env is set
    assert!(cfg.log_level.is_some());
}

#[test]
fn t09_load_from_file_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(
        dir.path(),
        "bp.toml",
        "default_backend = \"mock\"\nlog_level = \"warn\"",
    );
    let cfg = load_config(Some(&p)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn t10_load_empty_file_gives_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "empty.toml", "");
    let cfg = load_config(Some(&p)).unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn t11_load_file_with_backends_only() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "be.toml", "[backends.m]\ntype = \"mock\"");
    let cfg = load_config(Some(&p)).unwrap();
    assert!(cfg.default_backend.is_none());
    assert_eq!(cfg.backends.len(), 1);
}

#[test]
fn t12_parse_toml_with_comments() {
    let t = r#"
        # comment line
        default_backend = "mock" # inline comment
        # log_level = "trace"
        log_level = "info"
        [backends.mock] # section comment
        type = "mock"
    "#;
    let cfg = parse_toml(t).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn t13_parse_toml_unknown_fields_ignored() {
    let t = r#"
        default_backend = "mock"
        unknown_field = "hello"
        another_one = 42
    "#;
    let cfg = parse_toml(t).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn t14_parse_multiline_toml_string_value() {
    let t = "workspace_dir = '''\n/tmp/\nmultiline\n'''";
    let cfg = parse_toml(t).unwrap();
    assert!(cfg.workspace_dir.unwrap().contains("multiline"));
}

// ===========================================================================
// 2. Config merging (file + env + CLI override hierarchy)
// ===========================================================================

#[test]
fn t15_merge_overlay_wins_default_backend() {
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
fn t16_merge_overlay_wins_workspace_dir() {
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
fn t17_merge_overlay_wins_log_level() {
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
fn t18_merge_overlay_wins_receipts_dir() {
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
fn t19_merge_preserves_base_when_overlay_none() {
    let base = BackplaneConfig {
        default_backend: Some("x".into()),
        workspace_dir: Some("/w".into()),
        log_level: Some("trace".into()),
        receipts_dir: Some("/r".into()),
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
    let m = merge_configs(base, overlay);
    assert_eq!(m.default_backend.as_deref(), Some("x"));
    assert_eq!(m.workspace_dir.as_deref(), Some("/w"));
    assert_eq!(m.log_level.as_deref(), Some("trace"));
    assert_eq!(m.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn t20_merge_combines_distinct_backends() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("a".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("b".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert!(m.backends.contains_key("a") && m.backends.contains_key("b"));
}

#[test]
fn t21_merge_overlay_backend_wins_on_collision() {
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
fn t22_merge_three_layers() {
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
fn t23_merge_is_not_commutative() {
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
    assert_ne!(ab.default_backend, ba.default_backend);
}

#[test]
fn t24_merge_both_none_stays_none() {
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
    assert!(m.log_level.is_none());
    assert!(m.receipts_dir.is_none());
    assert!(m.workspace_dir.is_none());
}

#[test]
fn t25_merge_replaces_mock_with_sidecar() {
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
fn t26_merge_overlay_fixes_bad_backend() {
    let mut base = full_cfg();
    base.backends.insert(
        "broken".into(),
        BackendEntry::Sidecar {
            command: "".into(),
            args: vec![],
            timeout_secs: None,
        },
    );
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "broken".into(),
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
fn t27_merge_introduces_invalid_log_level() {
    let base = full_cfg();
    let overlay = BackplaneConfig {
        log_level: Some("banana".into()),
        ..Default::default()
    };
    assert!(validate_config(&merge_configs(base, overlay)).is_err());
}

#[test]
fn t28_merge_default_overlay_has_log_level_info() {
    let base = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let m = merge_configs(base, BackplaneConfig::default());
    // Default() sets log_level=Some("info"), so overlay wins.
    assert_eq!(m.log_level.as_deref(), Some("info"));
}

#[test]
fn t29_merge_empty_backends_preserves_base() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([("m".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::new(),
        ..Default::default()
    };
    assert!(merge_configs(base, overlay).backends.contains_key("m"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t30_file_plus_env_plus_defaults_merge() {
    let file_cfg = BackplaneConfig {
        default_backend: Some("file_be".into()),
        log_level: Some("warn".into()),
        ..Default::default()
    };
    let merged = merge_configs(BackplaneConfig::default(), file_cfg);
    let mut final_cfg = merged;
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "error_mgmt_30") }
    apply_env_overrides(&mut final_cfg);
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    assert_eq!(final_cfg.default_backend.as_deref(), Some("file_be"));
    assert_eq!(final_cfg.log_level.as_deref(), Some("error_mgmt_30"));
}

// ===========================================================================
// 3. Config validation (required fields, valid values)
// ===========================================================================

#[test]
fn t31_full_config_zero_warnings() {
    let w = validate_config(&full_cfg()).unwrap();
    assert!(w.is_empty(), "unexpected warnings: {w:?}");
}

#[test]
fn t32_default_config_has_advisory_warnings() {
    let w = validate_config(&BackplaneConfig::default()).unwrap();
    assert!(!w.is_empty());
}

#[test]
fn t33_valid_log_level_error() {
    let mut c = full_cfg();
    c.log_level = Some("error".into());
    validate_config(&c).unwrap();
}

#[test]
fn t34_valid_log_level_warn() {
    let mut c = full_cfg();
    c.log_level = Some("warn".into());
    validate_config(&c).unwrap();
}

#[test]
fn t35_valid_log_level_info() {
    let mut c = full_cfg();
    c.log_level = Some("info".into());
    validate_config(&c).unwrap();
}

#[test]
fn t36_valid_log_level_debug() {
    let mut c = full_cfg();
    c.log_level = Some("debug".into());
    validate_config(&c).unwrap();
}

#[test]
fn t37_valid_log_level_trace() {
    let mut c = full_cfg();
    c.log_level = Some("trace".into());
    validate_config(&c).unwrap();
}

#[test]
fn t38_valid_log_level_none() {
    let mut c = full_cfg();
    c.log_level = None;
    validate_config(&c).unwrap();
}

#[test]
fn t39_invalid_log_level_verbose() {
    let mut c = full_cfg();
    c.log_level = Some("verbose".into());
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("invalid log_level")));
}

#[test]
fn t40_invalid_log_level_uppercase() {
    let mut c = full_cfg();
    c.log_level = Some("INFO".into());
    assert!(validate_config(&c).is_err());
}

#[test]
fn t41_invalid_log_level_empty_string() {
    let mut c = full_cfg();
    c.log_level = Some(String::new());
    assert!(validate_config(&c).is_err());
}

#[test]
fn t42_invalid_log_level_mixed_case() {
    let mut c = full_cfg();
    c.log_level = Some("Debug".into());
    assert!(validate_config(&c).is_err());
}

#[test]
fn t43_invalid_log_level_with_whitespace() {
    let mut c = full_cfg();
    c.log_level = Some(" info ".into());
    assert!(validate_config(&c).is_err());
}

#[test]
fn t44_empty_sidecar_command_error() {
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
fn t45_whitespace_only_command_error() {
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
fn t46_zero_timeout_error() {
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
fn t47_timeout_just_over_max_error() {
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
fn t48_timeout_u64_max_error() {
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
fn t49_empty_backend_name_error() {
    let mut c = full_cfg();
    c.backends.insert("".into(), BackendEntry::Mock {});
    let r = reasons(validate_config(&c).unwrap_err());
    assert!(r.iter().any(|s| s.contains("name must not be empty")));
}

#[test]
fn t50_multiple_validation_errors_collected() {
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
fn t51_error_message_references_backend_name() {
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
fn t52_timeout_error_message_shows_value() {
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

#[test]
fn t53_sidecar_timeout_1s_valid() {
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
fn t54_sidecar_timeout_max_valid() {
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
fn t55_validate_does_not_mutate_valid() {
    let c = full_cfg();
    let before = c.clone();
    validate_config(&c).unwrap();
    assert_eq!(c, before);
}

#[test]
fn t56_validate_does_not_mutate_invalid() {
    let c = BackplaneConfig {
        log_level: Some("bad".into()),
        ..full_cfg()
    };
    let before = c.clone();
    let _ = validate_config(&c);
    assert_eq!(c, before);
}

#[test]
fn t57_validate_idempotent_valid() {
    let c = full_cfg();
    let w1 = validate_config(&c).unwrap();
    let w2 = validate_config(&c).unwrap();
    assert_eq!(w1, w2);
}

#[test]
fn t58_validate_idempotent_invalid() {
    let c = BackplaneConfig {
        log_level: Some("nope".into()),
        ..full_cfg()
    };
    let r1 = reasons(validate_config(&c).unwrap_err());
    let r2 = reasons(validate_config(&c).unwrap_err());
    assert_eq!(r1, r2);
}

// ===========================================================================
// 4. Environment variable overrides (ABP_ prefix)
// ===========================================================================

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t59_env_override_default_backend() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "from_env_mgmt_59") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.default_backend.as_deref(), Some("from_env_mgmt_59"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t60_env_override_log_level() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "trace_mgmt_60") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    assert_eq!(c.log_level.as_deref(), Some("trace_mgmt_60"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t61_env_override_receipts_dir() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_RECEIPTS_DIR", "/recv_mgmt_61") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") }
    assert_eq!(c.receipts_dir.as_deref(), Some("/recv_mgmt_61"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t62_env_override_workspace_dir() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/ws_mgmt_62") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") }
    assert_eq!(c.workspace_dir.as_deref(), Some("/ws_mgmt_62"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t63_env_override_replaces_existing() {
    let mut c = BackplaneConfig {
        default_backend: Some("old".into()),
        ..Default::default()
    };
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "new_mgmt_63") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.default_backend.as_deref(), Some("new_mgmt_63"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t64_env_override_does_not_touch_unset_vars() {
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
#[ignore = "Windows removes env vars set to empty string; racy in parallel"]
fn t65_env_override_empty_string_is_set() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.default_backend.as_deref(), Some(""));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t66_env_override_does_not_affect_backends() {
    let mut c = full_cfg();
    let before = c.backends.len();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "env_mgmt_66") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    assert_eq!(c.backends.len(), before);
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t67_load_config_env_overrides_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(
        dir.path(),
        "bp67.toml",
        r#"default_backend = "file_val_67""#,
    );
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "env_val_67") }
    let c = load_config(Some(&p)).unwrap();
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    // Env override should win over file value.
    assert_eq!(c.default_backend.as_deref(), Some("env_val_67"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t68_env_all_four_vars_override() {
    let mut c = BackplaneConfig::default();
    unsafe { std::env::set_var("ABP_DEFAULT_BACKEND", "m68") }
    unsafe { std::env::set_var("ABP_LOG_LEVEL", "debug68") }
    unsafe { std::env::set_var("ABP_RECEIPTS_DIR", "/r68") }
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/w68") }
    apply_env_overrides(&mut c);
    unsafe { std::env::remove_var("ABP_DEFAULT_BACKEND") }
    unsafe { std::env::remove_var("ABP_LOG_LEVEL") }
    unsafe { std::env::remove_var("ABP_RECEIPTS_DIR") }
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") }
    assert_eq!(c.default_backend.as_deref(), Some("m68"));
    assert_eq!(c.log_level.as_deref(), Some("debug68"));
    assert_eq!(c.receipts_dir.as_deref(), Some("/r68"));
    assert_eq!(c.workspace_dir.as_deref(), Some("/w68"));
}

// ===========================================================================
// 5. Backend configuration selection
// ===========================================================================

#[test]
fn t69_select_mock_backend_by_name() {
    let c = full_cfg();
    assert!(matches!(
        c.backends.get("mock"),
        Some(BackendEntry::Mock {})
    ));
}

#[test]
fn t70_select_sidecar_backend_by_name() {
    let c = full_cfg();
    assert!(matches!(
        c.backends.get("sc"),
        Some(BackendEntry::Sidecar { .. })
    ));
}

#[test]
fn t71_nonexistent_backend_returns_none() {
    let c = full_cfg();
    assert!(!c.backends.contains_key("nonexistent"));
}

#[test]
fn t72_iterate_all_backends() {
    let c = full_cfg();
    let names: Vec<&String> = c.backends.keys().collect();
    assert!(names.contains(&&"mock".to_string()));
    assert!(names.contains(&&"sc".to_string()));
}

#[test]
fn t73_many_mock_backends_valid() {
    let mut c = full_cfg();
    for i in 0..50 {
        c.backends.insert(format!("m{i}"), BackendEntry::Mock {});
    }
    validate_config(&c).unwrap();
}

#[test]
fn t74_only_mock_backends_config() {
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
fn t75_only_sidecar_backends_config() {
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
fn t76_mixed_backends_config() {
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
// 6. Sidecar backend config (command, args, env)
// ===========================================================================

#[test]
fn t77_sidecar_command_is_required() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        args = ["x"]
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t78_sidecar_args_default_to_empty() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(t).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn t79_sidecar_timeout_defaults_to_none() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
    "#;
    let cfg = parse_toml(t).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, None),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn t80_sidecar_empty_args_array() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = []
    "#;
    let cfg = parse_toml(t).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn t81_sidecar_many_args() {
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        args = ["--a", "--b", "--c", "--d", "--e"]
    "#;
    let cfg = parse_toml(t).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert_eq!(args.len(), 5),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn t82_sidecar_with_complex_args() {
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
    validate_config(&c).unwrap();
}

#[test]
fn t83_sidecar_args_with_empty_strings() {
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
fn t84_sidecar_100_args() {
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

#[test]
fn t85_parse_example_toml_format() {
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
}

// ===========================================================================
// 7. Default config values
// ===========================================================================

#[test]
fn t86_default_log_level_is_info() {
    assert_eq!(
        BackplaneConfig::default().log_level.as_deref(),
        Some("info")
    );
}

#[test]
fn t87_default_no_default_backend() {
    assert!(BackplaneConfig::default().default_backend.is_none());
}

#[test]
fn t88_default_no_workspace_dir() {
    assert!(BackplaneConfig::default().workspace_dir.is_none());
}

#[test]
fn t89_default_no_receipts_dir() {
    assert!(BackplaneConfig::default().receipts_dir.is_none());
}

#[test]
fn t90_default_empty_backends() {
    assert!(BackplaneConfig::default().backends.is_empty());
}

#[test]
fn t91_default_config_is_valid() {
    let w = validate_config(&BackplaneConfig::default()).unwrap();
    assert!(!w.is_empty());
}

#[test]
fn t92_minimal_config_warnings_are_advisory() {
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
// 8. Config serialization/deserialization round-trip
// ===========================================================================

#[test]
fn t93_toml_roundtrip_full_config() {
    let c = full_cfg();
    let s = toml::to_string(&c).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(c, d);
}

#[test]
fn t94_toml_roundtrip_default_config() {
    let c = BackplaneConfig::default();
    let s = toml::to_string(&c).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(c, d);
}

#[test]
fn t95_json_roundtrip_full_config() {
    let c = full_cfg();
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c, d);
}

#[test]
fn t96_json_roundtrip_default_config() {
    let c = BackplaneConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c, d);
}

#[test]
fn t97_toml_roundtrip_preserves_validation() {
    let c = full_cfg();
    validate_config(&c).unwrap();
    let s = toml::to_string(&c).unwrap();
    let d = parse_toml(&s).unwrap();
    validate_config(&d).unwrap();
}

#[test]
fn t98_json_roundtrip_preserves_validation() {
    let c = full_cfg();
    validate_config(&c).unwrap();
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    validate_config(&d).unwrap();
}

#[test]
fn t99_toml_serialization_skips_none_fields() {
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
fn t100_json_serialization_skips_none_fields() {
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
fn t101_toml_roundtrip_sidecar_with_complex_args() {
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
fn t102_json_schema_can_be_generated() {
    let schema = schemars::schema_for!(BackplaneConfig);
    let j = serde_json::to_string_pretty(&schema).unwrap();
    assert!(j.contains("BackplaneConfig"));
}

#[test]
fn t103_backend_entry_tagged_with_type_mock() {
    let j = serde_json::to_string(&BackendEntry::Mock {}).unwrap();
    assert!(j.contains(r#""type":"mock""#));
}

#[test]
fn t104_backend_entry_tagged_with_type_sidecar() {
    let e = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: None,
    };
    let j = serde_json::to_string(&e).unwrap();
    assert!(j.contains(r#""type":"sidecar""#));
}

#[test]
fn t105_backends_serialize_in_alphabetical_order() {
    let mut c = full_cfg();
    c.backends.insert("zzz".into(), BackendEntry::Mock {});
    c.backends.insert("aaa".into(), BackendEntry::Mock {});
    let s = toml::to_string(&c).unwrap();
    let pos_aaa = s.find("[backends.aaa]").unwrap();
    let pos_zzz = s.find("[backends.zzz]").unwrap();
    assert!(pos_aaa < pos_zzz);
}

// ===========================================================================
// 9. Config error handling (missing file, invalid TOML, missing required)
// ===========================================================================

#[test]
fn t106_load_missing_file_gives_file_not_found() {
    let err = load_config(Some(Path::new("/nonexistent/backplane.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn t107_parse_garbage_gives_parse_error() {
    let err = parse_toml("{{{{not toml}}}}").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn t108_parse_wrong_type_for_default_backend() {
    let err = parse_toml("default_backend = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn t109_parse_wrong_type_for_log_level() {
    let err = parse_toml("log_level = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn t110_parse_wrong_type_for_backends() {
    let err = parse_toml("backends = 99").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn t111_parse_backend_missing_type_discriminator() {
    let t = r#"
        [backends.bad]
        command = "node"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t112_parse_backend_unknown_type_discriminator() {
    let t = r#"
        [backends.bad]
        type = "openai_native"
        command = "node"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t113_parse_sidecar_wrong_type_for_command() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        command = 123
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t114_parse_sidecar_wrong_type_for_args() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        command = "node"
        args = "not_an_array"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t115_parse_sidecar_wrong_type_for_timeout() {
    let t = r#"
        [backends.bad]
        type = "sidecar"
        command = "node"
        timeout_secs = "thirty"
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t116_load_invalid_toml_file_gives_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "bad.toml", "not = [valid toml");
    let err = load_config(Some(&p)).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn t117_file_not_found_display() {
    let e = ConfigError::FileNotFound {
        path: "/foo/bar.toml".into(),
    };
    assert!(e.to_string().contains("/foo/bar.toml"));
}

#[test]
fn t118_parse_error_display_contains_detail() {
    let e = ConfigError::ParseError {
        reason: "unexpected token".into(),
    };
    assert!(e.to_string().contains("unexpected token"));
}

#[test]
fn t119_validation_error_display_all_reasons() {
    let e = ConfigError::ValidationError {
        reasons: vec!["one".into(), "two".into()],
    };
    let s = e.to_string();
    assert!(s.contains("one") && s.contains("two"));
}

#[test]
fn t120_merge_conflict_display() {
    let e = ConfigError::MergeConflict {
        reason: "conflict!".into(),
    };
    assert!(e.to_string().contains("conflict!"));
}

#[test]
fn t121_config_error_debug_variants() {
    assert!(
        format!("{:?}", ConfigError::FileNotFound { path: "/p".into() }).contains("FileNotFound")
    );
    assert!(format!("{:?}", ConfigError::ParseError { reason: "x".into() }).contains("ParseError"));
    assert!(
        format!(
            "{:?}",
            ConfigError::ValidationError {
                reasons: vec!["r".into()]
            }
        )
        .contains("ValidationError")
    );
    assert!(
        format!("{:?}", ConfigError::MergeConflict { reason: "c".into() })
            .contains("MergeConflict")
    );
}

// ===========================================================================
// 10. Config sections (backends, workspace, policy, telemetry)
// ===========================================================================

#[test]
fn t122_workspace_dir_section_parsed() {
    let t = r#"workspace_dir = "/my/workspace""#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.workspace_dir.as_deref(), Some("/my/workspace"));
}

#[test]
fn t123_receipts_dir_section_parsed() {
    let t = r#"receipts_dir = "./data/receipts""#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.receipts_dir.as_deref(), Some("./data/receipts"));
}

#[test]
fn t124_log_level_section_parsed() {
    let t = r#"log_level = "trace""#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.log_level.as_deref(), Some("trace"));
}

#[test]
fn t125_backends_section_is_map() {
    let c = full_cfg();
    assert_eq!(c.backends.len(), 2);
    assert!(c.backends.contains_key("mock"));
    assert!(c.backends.contains_key("sc"));
}

#[test]
fn t126_all_optional_fields_set_parses_and_validates() {
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
// 11. Validation warnings
// ===========================================================================

#[test]
fn t127_missing_default_backend_warning() {
    let mut c = full_cfg();
    c.default_backend = None;
    let w = validate_config(&c).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
    )));
}

#[test]
fn t128_missing_receipts_dir_warning() {
    let mut c = full_cfg();
    c.receipts_dir = None;
    let w = validate_config(&c).unwrap();
    assert!(w.iter().any(|w| matches!(
        w,
        ConfigWarning::MissingOptionalField { field, .. } if field == "receipts_dir"
    )));
}

#[test]
fn t129_both_optional_missing_gives_two_warnings() {
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
fn t130_large_timeout_above_threshold_warns() {
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
fn t131_timeout_exactly_at_threshold_no_warning() {
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
fn t132_timeout_below_threshold_no_warning() {
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
fn t133_multiple_large_timeouts_produce_multiple_warnings() {
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
// 12. ConfigWarning Display
// ===========================================================================

#[test]
fn t134_deprecated_field_display_with_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "old".into(),
        suggestion: Some("new".into()),
    };
    let s = w.to_string();
    assert!(s.contains("old") && s.contains("new"));
}

#[test]
fn t135_deprecated_field_display_without_suggestion() {
    let w = ConfigWarning::DeprecatedField {
        field: "x".into(),
        suggestion: None,
    };
    let s = w.to_string();
    assert!(s.contains("x"));
    assert!(!s.contains("instead"));
}

#[test]
fn t136_missing_optional_display() {
    let w = ConfigWarning::MissingOptionalField {
        field: "f".into(),
        hint: "h".into(),
    };
    assert!(w.to_string().contains('f') && w.to_string().contains('h'));
}

#[test]
fn t137_large_timeout_display() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 9999,
    };
    let s = w.to_string();
    assert!(s.contains("sc") && s.contains("9999"));
}

#[test]
fn t138_config_warning_clone_eq() {
    let w = ConfigWarning::LargeTimeout {
        backend: "sc".into(),
        secs: 7200,
    };
    assert_eq!(w, w.clone());
}

#[test]
fn t139_config_warning_ne() {
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
fn t140_config_warning_debug() {
    let w = ConfigWarning::DeprecatedField {
        field: "x".into(),
        suggestion: None,
    };
    assert!(format!("{w:?}").contains("DeprecatedField"));
}

// ===========================================================================
// 13. Config reload/hot-reload patterns
// ===========================================================================

#[test]
fn t141_reload_picks_up_changed_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "bp.toml", r#"log_level = "info""#);
    let c1 = load_config(Some(&p)).unwrap();
    // Overwrite the file with a different value.
    std::fs::write(&p, r#"log_level = "debug""#).unwrap();
    let c2 = load_config(Some(&p)).unwrap();
    // If ABP_LOG_LEVEL env is not set, file value is used.
    if std::env::var("ABP_LOG_LEVEL").is_err() {
        assert_ne!(c1.log_level, c2.log_level);
    }
}

#[test]
fn t142_reload_detects_added_backend() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "bp.toml", "");
    let c1 = load_config(Some(&p)).unwrap();
    assert!(c1.backends.is_empty());
    std::fs::write(&p, "[backends.m]\ntype = \"mock\"").unwrap();
    let c2 = load_config(Some(&p)).unwrap();
    assert_eq!(c2.backends.len(), 1);
}

#[test]
fn t143_reload_after_file_deleted_gives_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(dir.path(), "bp.toml", "");
    let _ = load_config(Some(&p)).unwrap();
    std::fs::remove_file(&p).unwrap();
    assert!(matches!(
        load_config(Some(&p)).unwrap_err(),
        ConfigError::FileNotFound { .. }
    ));
}

// ===========================================================================
// 14. Config template generation (producing a valid example)
// ===========================================================================

#[test]
fn t144_generated_template_is_valid_toml() {
    let template = full_cfg();
    let toml_str = toml::to_string_pretty(&template).unwrap();
    let roundtrip: BackplaneConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(template, roundtrip);
}

#[test]
fn t145_generated_template_passes_validation() {
    let template = full_cfg();
    let toml_str = toml::to_string_pretty(&template).unwrap();
    let parsed = parse_toml(&toml_str).unwrap();
    validate_config(&parsed).unwrap();
}

#[test]
fn t146_default_template_contains_expected_sections() {
    let template = full_cfg();
    let toml_str = toml::to_string_pretty(&template).unwrap();
    assert!(toml_str.contains("default_backend"));
    assert!(toml_str.contains("log_level"));
    assert!(toml_str.contains("[backends.mock]"));
    assert!(toml_str.contains("[backends.sc]"));
}

// ===========================================================================
// 15. Config migration between versions
// ===========================================================================

#[test]
fn t147_v01_config_still_parses() {
    // A config file from v0.1 (the current format) should continue to parse.
    let t = r#"
        default_backend = "mock"
        log_level = "info"
        [backends.mock]
        type = "mock"
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
}

#[test]
fn t148_extra_fields_forward_compat() {
    // A future config with extra fields should still parse (unknown fields ignored).
    let t = r#"
        default_backend = "mock"
        log_level = "info"
        future_field = "hello"
        version = "0.2"
        [backends.mock]
        type = "mock"
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
}

// ===========================================================================
// 16. Nested config value access / dotted keys
// ===========================================================================

#[test]
fn t149_dotted_backend_access() {
    let c = full_cfg();
    let mock = &c.backends["mock"];
    assert!(matches!(mock, BackendEntry::Mock {}));
    let sc = &c.backends["sc"];
    match sc {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn t150_nested_backend_fields_via_toml_dotted() {
    let t = r#"
        backends.m.type = "mock"
    "#;
    // TOML dotted key syntax for nested tables
    let c = parse_toml(t).unwrap();
    assert!(matches!(c.backends["m"], BackendEntry::Mock {}));
}

// ===========================================================================
// 17. Config with vendor-specific extensions
// ===========================================================================

#[test]
fn t151_vendor_extension_in_unknown_fields_ignored() {
    let t = r#"
        default_backend = "mock"
        [backends.mock]
        type = "mock"
        [vendor]
        abp_mode = "passthrough"
        custom_key = "value"
    "#;
    let c = parse_toml(t).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("mock"));
}

#[test]
fn t152_vendor_nested_tables_ignored() {
    let t = r#"
        [backends.mock]
        type = "mock"
        [vendor.openai]
        api_key_env = "OPENAI_API_KEY"
        model = "gpt-4"
    "#;
    let c = parse_toml(t).unwrap();
    assert!(matches!(c.backends["mock"], BackendEntry::Mock {}));
}

// ===========================================================================
// 18. Edge cases – strings, special values, derive checks
// ===========================================================================

#[test]
fn t153_very_long_backend_name_valid() {
    let mut c = full_cfg();
    c.backends.insert("a".repeat(10_000), BackendEntry::Mock {});
    validate_config(&c).unwrap();
}

#[test]
fn t154_very_long_command_valid() {
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
fn t155_unicode_in_backend_name_and_command() {
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
fn t156_special_chars_in_paths() {
    let c = BackplaneConfig {
        workspace_dir: Some("/tmp/agent (copy)/ws!/@#$%".into()),
        receipts_dir: Some(r"C:\Users\日本語\receipts".into()),
        ..full_cfg()
    };
    validate_config(&c).unwrap();
}

#[test]
fn t157_backend_name_with_dots_and_dashes() {
    let mut c = full_cfg();
    c.backends
        .insert("my-backend_v2.0".into(), BackendEntry::Mock {});
    validate_config(&c).unwrap();
}

#[test]
fn t158_config_clone_equals_original() {
    let c = full_cfg();
    assert_eq!(c, c.clone());
}

#[test]
fn t159_config_debug_not_empty() {
    let c = full_cfg();
    let dbg = format!("{c:?}");
    assert!(!dbg.is_empty());
    assert!(dbg.contains("BackplaneConfig"));
}

#[test]
fn t160_backend_entry_sidecar_clone_equals() {
    let e = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec!["a".into()],
        timeout_secs: Some(60),
    };
    assert_eq!(e, e.clone());
}

#[test]
fn t161_backend_entry_mock_clone_equals() {
    let e = BackendEntry::Mock {};
    assert_eq!(e, e.clone());
}

#[test]
fn t162_config_ne_when_different() {
    let a = full_cfg();
    let mut b = full_cfg();
    b.log_level = Some("debug".into());
    assert_ne!(a, b);
}

#[test]
fn t163_independent_config_instances() {
    let c1 = full_cfg();
    let mut c2 = full_cfg();
    c2.log_level = Some("trace".into());
    assert_ne!(c1, c2);
    assert_eq!(c1.log_level.as_deref(), Some("info"));
}

#[test]
fn t164_parse_error_from_toml_gives_parse_error() {
    let err = parse_toml("{{bad}}").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to parse config"));
}

#[test]
fn t165_validate_idempotent_warnings() {
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

#[test]
fn t166_merge_many_backends_from_both() {
    let mut base_backends = BTreeMap::new();
    for i in 0..20 {
        base_backends.insert(format!("base_{i}"), BackendEntry::Mock {});
    }
    let mut overlay_backends = BTreeMap::new();
    for i in 0..20 {
        overlay_backends.insert(format!("overlay_{i}"), BackendEntry::Mock {});
    }
    let base = BackplaneConfig {
        backends: base_backends,
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: overlay_backends,
        ..Default::default()
    };
    let m = merge_configs(base, overlay);
    assert_eq!(m.backends.len(), 40);
}

#[test]
fn t167_parse_toml_negative_timeout_fails() {
    // TOML integers are signed, but timeout_secs is u64 — negative should fail.
    let t = r#"
        [backends.sc]
        type = "sidecar"
        command = "node"
        timeout_secs = -1
    "#;
    assert!(parse_toml(t).is_err());
}

#[test]
fn t168_toml_roundtrip_config_with_no_backends() {
    let c = BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/ws".into()),
        log_level: Some("trace".into()),
        receipts_dir: Some("/r".into()),
        backends: BTreeMap::new(),
        ..Default::default()
    };
    let s = toml::to_string(&c).unwrap();
    let d: BackplaneConfig = toml::from_str(&s).unwrap();
    assert_eq!(c, d);
}

#[test]
fn t169_json_roundtrip_config_many_backends() {
    let mut c = full_cfg();
    for i in 0..10 {
        c.backends.insert(
            format!("sidecar_{i}"),
            BackendEntry::Sidecar {
                command: format!("cmd_{i}"),
                args: vec![format!("arg_{i}")],
                timeout_secs: Some(60 + i),
            },
        );
    }
    let j = serde_json::to_string(&c).unwrap();
    let d: BackplaneConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(c, d);
}

#[test]
fn t170_parse_toml_with_only_workspace_dir() {
    let c = parse_toml(r#"workspace_dir = "/ws""#).unwrap();
    assert_eq!(c.workspace_dir.as_deref(), Some("/ws"));
    assert!(c.default_backend.is_none());
}

#[test]
fn t171_parse_toml_with_only_receipts_dir() {
    let c = parse_toml(r#"receipts_dir = "/r""#).unwrap();
    assert_eq!(c.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn t172_merge_sidecar_replaces_sidecar() {
    let base = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python".into(),
                args: vec!["old.py".into()],
                timeout_secs: Some(30),
            },
        )]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([(
            "sc".into(),
            BackendEntry::Sidecar {
                command: "python3".into(),
                args: vec!["new.py".into()],
                timeout_secs: Some(60),
            },
        )]),
        ..Default::default()
    };
    match &merge_configs(base, overlay).backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert_eq!(args, &["new.py"]);
            assert_eq!(*timeout_secs, Some(60));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn t173_sidecar_timeout_just_below_max_valid() {
    let mut c = full_cfg();
    c.backends.insert(
        "edge".into(),
        BackendEntry::Sidecar {
            command: "x".into(),
            args: vec![],
            timeout_secs: Some(86_399),
        },
    );
    // 86_399 is above large threshold (3600) so warning, but no hard error.
    let w = validate_config(&c).unwrap();
    assert!(
        w.iter()
            .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. }))
    );
}

#[test]
fn t174_json_serialization_includes_type_tag() {
    let c = full_cfg();
    let j = serde_json::to_string_pretty(&c).unwrap();
    assert!(j.contains("\"type\""));
    assert!(j.contains("\"mock\""));
    assert!(j.contains("\"sidecar\""));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn t175_load_config_from_file_applies_env_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let p = write_toml(
        dir.path(),
        "bp175.toml",
        "default_backend = \"file_val_175\"",
    );
    unsafe { std::env::set_var("ABP_WORKSPACE_DIR", "/env_ws_175") }
    let c = load_config(Some(&p)).unwrap();
    unsafe { std::env::remove_var("ABP_WORKSPACE_DIR") }
    assert_eq!(c.default_backend.as_deref(), Some("file_val_175"));
    assert_eq!(c.workspace_dir.as_deref(), Some("/env_ws_175"));
}

#[test]
fn t176_parse_toml_preserves_whitespace_in_values() {
    let c = parse_toml(r#"default_backend = "  spaces  ""#).unwrap();
    assert_eq!(c.default_backend.as_deref(), Some("  spaces  "));
}

#[test]
fn t177_validate_config_with_many_mixed_backends() {
    let mut c = full_cfg();
    for i in 0..25 {
        if i % 2 == 0 {
            c.backends.insert(format!("m_{i}"), BackendEntry::Mock {});
        } else {
            c.backends.insert(
                format!("s_{i}"),
                BackendEntry::Sidecar {
                    command: format!("cmd{i}"),
                    args: vec![],
                    timeout_secs: Some(60),
                },
            );
        }
    }
    validate_config(&c).unwrap();
}

#[test]
fn t178_merge_four_layers() {
    let a = BackplaneConfig {
        default_backend: Some("a".into()),
        backends: BTreeMap::from([("a_be".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let b = BackplaneConfig {
        log_level: Some("debug".into()),
        backends: BTreeMap::from([("b_be".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    let c = BackplaneConfig {
        workspace_dir: Some("/ws_c".into()),
        log_level: None,
        ..Default::default()
    };
    let d = BackplaneConfig {
        receipts_dir: Some("/r_d".into()),
        log_level: None,
        ..Default::default()
    };
    let m = merge_configs(merge_configs(merge_configs(a, b), c), d);
    assert_eq!(m.default_backend.as_deref(), Some("a"));
    assert_eq!(m.log_level.as_deref(), Some("debug"));
    assert_eq!(m.workspace_dir.as_deref(), Some("/ws_c"));
    assert_eq!(m.receipts_dir.as_deref(), Some("/r_d"));
    assert!(m.backends.contains_key("a_be"));
    assert!(m.backends.contains_key("b_be"));
}

#[test]
fn t179_parse_toml_escaped_strings() {
    let t = r#"workspace_dir = "C:\\Users\\test\\workspace""#;
    let c = parse_toml(t).unwrap();
    assert_eq!(
        c.workspace_dir.as_deref(),
        Some("C:\\Users\\test\\workspace")
    );
}

#[test]
fn t180_sidecar_with_no_timeout_no_args() {
    let t = r#"
        [backends.minimal_sc]
        type = "sidecar"
        command = "bash"
    "#;
    let c = parse_toml(t).unwrap();
    match &c.backends["minimal_sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "bash");
            assert!(args.is_empty());
            assert_eq!(*timeout_secs, None);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
    validate_config(&c).unwrap();
}

#[test]
fn t181_merge_replaces_sidecar_with_mock() {
    let base = BackplaneConfig {
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
    let overlay = BackplaneConfig {
        backends: BTreeMap::from([("x".into(), BackendEntry::Mock {})]),
        ..Default::default()
    };
    assert!(matches!(
        merge_configs(base, overlay).backends["x"],
        BackendEntry::Mock {}
    ));
}

#[test]
fn t182_toml_pretty_and_compact_both_roundtrip() {
    let c = full_cfg();
    let pretty = toml::to_string_pretty(&c).unwrap();
    let compact = toml::to_string(&c).unwrap();
    let from_pretty: BackplaneConfig = toml::from_str(&pretty).unwrap();
    let from_compact: BackplaneConfig = toml::from_str(&compact).unwrap();
    assert_eq!(c, from_pretty);
    assert_eq!(c, from_compact);
}
