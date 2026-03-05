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
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
//! Deep TOML parsing tests for `abp-config`.

use abp_config::{
    apply_env_overrides, load_config, merge_configs, parse_toml, validate_config, BackendEntry,
    BackplaneConfig, ConfigError, ConfigWarning,
};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fully-specified config that passes validation with no warnings.
fn fully_valid_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        workspace_dir: Some("/tmp/ws".into()),
        log_level: Some("info".into()),
        receipts_dir: Some("/tmp/receipts".into()),
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
    }
}

// ===========================================================================
// 1. Minimal valid TOML parses
// ===========================================================================

#[test]
fn minimal_empty_string_parses() {
    let cfg = parse_toml("").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
    assert!(cfg.log_level.is_none());
    assert!(cfg.receipts_dir.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn minimal_single_scalar_field() {
    let cfg = parse_toml(r#"default_backend = "mock""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert!(cfg.backends.is_empty());
}

#[test]
fn minimal_mock_backend_only() {
    let toml = "[backends.test]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 1);
    assert!(matches!(cfg.backends["test"], BackendEntry::Mock {}));
}

#[test]
fn minimal_sidecar_backend_required_fields_only() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "node");
            assert!(args.is_empty());
            assert!(timeout_secs.is_none());
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

// ===========================================================================
// 2. Full example TOML parses correctly
// ===========================================================================

#[test]
fn full_example_toml_all_fields() {
    let toml = r#"
default_backend = "mock"
workspace_dir = "/tmp/workspace"
log_level = "debug"
receipts_dir = "/tmp/receipts"

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
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/workspace"));
    assert_eq!(cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/tmp/receipts"));
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
    match &cfg.backends["openai"] {
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
    match &cfg.backends["anthropic"] {
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
fn full_config_roundtrips_through_toml() {
    let toml_in = r#"
default_backend = "mock"
workspace_dir = "/ws"
log_level = "info"
receipts_dir = "/receipts"

[backends.m]
type = "mock"

[backends.sc]
type = "sidecar"
command = "node"
args = ["host.js", "--flag"]
timeout_secs = 120
"#;
    let cfg = parse_toml(toml_in).unwrap();
    let serialized = toml::to_string(&cfg).unwrap();
    let cfg2 = parse_toml(&serialized).unwrap();
    assert_eq!(cfg, cfg2);
}

#[test]
fn actual_example_file_structure_parses() {
    // Mirrors the uncommented structure from backplane.example.toml
    let toml = r#"
[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["path/to/openai-sidecar.js"]

[backends.anthropic]
type = "sidecar"
command = "python3"
args = ["path/to/anthropic-sidecar.py"]
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

// ===========================================================================
// 3. Missing optional fields get defaults
// ===========================================================================

#[test]
fn missing_default_backend_is_none() {
    let cfg = parse_toml("log_level = \"info\"").unwrap();
    assert!(cfg.default_backend.is_none());
}

#[test]
fn missing_workspace_dir_is_none() {
    let cfg = parse_toml("log_level = \"info\"").unwrap();
    assert!(cfg.workspace_dir.is_none());
}

#[test]
fn missing_log_level_is_none() {
    let cfg = parse_toml(r#"default_backend = "x""#).unwrap();
    assert!(cfg.log_level.is_none());
}

#[test]
fn missing_receipts_dir_is_none() {
    let cfg = parse_toml("log_level = \"info\"").unwrap();
    assert!(cfg.receipts_dir.is_none());
}

#[test]
fn missing_backends_table_is_empty_map() {
    let cfg = parse_toml(r#"default_backend = "x""#).unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn sidecar_missing_args_defaults_to_empty_vec() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn sidecar_missing_timeout_defaults_to_none() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert!(timeout_secs.is_none()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn all_optional_fields_missing_still_parses() {
    let toml = "[backends.m]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
    assert!(cfg.log_level.is_none());
    assert!(cfg.receipts_dir.is_none());
    assert_eq!(cfg.backends.len(), 1);
}

// ===========================================================================
// 4. Unknown fields are ignored (forward compat)
// ===========================================================================

#[test]
fn unknown_top_level_field_ignored() {
    let toml = "default_backend = \"mock\"\nsome_future_field = \"value\"\nanother_key = 42";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn unknown_field_in_mock_backend_ignored() {
    let toml = "[backends.m]\ntype = \"mock\"\nnew_field = \"ignored\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["m"], BackendEntry::Mock {}));
}

#[test]
fn unknown_field_in_sidecar_backend_ignored() {
    let toml = r#"
[backends.sc]
type = "sidecar"
command = "node"
future_feature = true
priority = 5
"#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => assert_eq!(command, "node"),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn unknown_top_level_table_ignored() {
    let toml = r#"
default_backend = "mock"

[some_future_section]
key = "value"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

// ===========================================================================
// 5. Type mismatch in TOML field produces clear error
// ===========================================================================

#[test]
fn type_mismatch_log_level_integer() {
    let err = parse_toml("log_level = 42").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_log_level_boolean() {
    let err = parse_toml("log_level = true").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_default_backend_integer() {
    let err = parse_toml("default_backend = 123").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_default_backend_array() {
    let err = parse_toml(r#"default_backend = ["a", "b"]"#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_backends_is_string() {
    let err = parse_toml(r#"backends = "not a table""#).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_timeout_secs_string() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = \"three\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_timeout_secs_float() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 3.14";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_args_is_string_not_array() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = \"not-array\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn type_mismatch_args_contains_integers() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = [1, 2]";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn parse_error_message_is_nonempty() {
    let err = parse_toml("log_level = 42").unwrap_err();
    match err {
        ConfigError::ParseError { reason } => {
            assert!(!reason.is_empty(), "parse error reason must not be empty");
        }
        other => panic!("expected ParseError, got {other:?}"),
    }
}

// ===========================================================================
// 6. Nested table parsing (backends)
// ===========================================================================

#[test]
fn nested_backend_table_parses() {
    let toml = r#"
[backends.my_backend]
type = "sidecar"
command = "python3"
args = ["--verbose", "main.py"]
timeout_secs = 60
"#;
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("my_backend"));
    match &cfg.backends["my_backend"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "python3");
            assert_eq!(args.len(), 2);
            assert_eq!(*timeout_secs, Some(60));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn dotted_backend_name_with_quotes() {
    let toml = "[backends.\"my.dotted.name\"]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("my.dotted.name"));
}

#[test]
fn hyphenated_backend_name() {
    let toml = "[backends.my-backend]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("my-backend"));
}

#[test]
fn underscored_backend_name() {
    let toml = "[backends.my_backend_v2]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("my_backend_v2"));
}

#[test]
fn sidecar_all_fields_populated() {
    let toml = r#"
[backends.full]
type = "sidecar"
command = "/usr/local/bin/node"
args = ["--experimental-modules", "--max-old-space-size=4096", "host.js"]
timeout_secs = 600
"#;
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["full"] {
        BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        } => {
            assert_eq!(command, "/usr/local/bin/node");
            assert_eq!(args.len(), 3);
            assert_eq!(*timeout_secs, Some(600));
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn inline_table_for_backend() {
    let toml = "[backends]\nmock = { type = \"mock\" }";
    let cfg = parse_toml(toml).unwrap();
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
}

// ===========================================================================
// 7. Multiple backends (array-like via BTreeMap)
// ===========================================================================

#[test]
fn multiple_mock_backends() {
    let toml = r#"
[backends.mock1]
type = "mock"
[backends.mock2]
type = "mock"
[backends.mock3]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    for name in &["mock1", "mock2", "mock3"] {
        assert!(matches!(cfg.backends[*name], BackendEntry::Mock {}));
    }
}

#[test]
fn multiple_sidecar_backends() {
    let toml = r#"
[backends.node]
type = "sidecar"
command = "node"
args = ["host.js"]

[backends.python]
type = "sidecar"
command = "python3"
args = ["host.py"]

[backends.ruby]
type = "sidecar"
command = "ruby"
args = ["host.rb"]
timeout_secs = 120
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
}

#[test]
fn mixed_mock_and_sidecar_backends() {
    let toml = r#"
[backends.mock]
type = "mock"
[backends.sc1]
type = "sidecar"
command = "node"
[backends.sc2]
type = "sidecar"
command = "python3"
args = ["host.py"]
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.backends.len(), 3);
    assert!(matches!(cfg.backends["mock"], BackendEntry::Mock {}));
    assert!(matches!(cfg.backends["sc1"], BackendEntry::Sidecar { .. }));
    assert!(matches!(cfg.backends["sc2"], BackendEntry::Sidecar { .. }));
}

#[test]
fn backends_stored_in_sorted_order() {
    let toml = r#"
[backends.z_last]
type = "mock"
[backends.a_first]
type = "mock"
[backends.m_middle]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    let keys: Vec<&String> = cfg.backends.keys().collect();
    assert_eq!(keys, vec!["a_first", "m_middle", "z_last"]);
}

// ===========================================================================
// 8. Environment variable overrides
// ===========================================================================

// NOTE: env var tests race when run in parallel. Each ABP_* key is used
// by at most ONE test to avoid conflicts.

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn env_override_default_backend_adds_and_replaces() {
    let key = "ABP_DEFAULT_BACKEND";
    // Adds when field is None.
    let mut cfg = parse_toml("").unwrap();
    unsafe { std::env::set_var(key, "from_env") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.default_backend.as_deref(), Some("from_env"));
    // Replaces an existing value.
    let mut cfg2 = parse_toml(r#"default_backend = "toml_value""#).unwrap();
    apply_env_overrides(&mut cfg2);
    assert_eq!(cfg2.default_backend.as_deref(), Some("from_env"));
    unsafe { std::env::remove_var(key) };
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn env_override_log_level() {
    let key = "ABP_LOG_LEVEL";
    let mut cfg = parse_toml(r#"log_level = "info""#).unwrap();
    unsafe { std::env::set_var(key, "trace") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var(key) };
    assert_eq!(cfg.log_level.as_deref(), Some("trace"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn env_override_receipts_dir() {
    let key = "ABP_RECEIPTS_DIR";
    let mut cfg = parse_toml("").unwrap();
    unsafe { std::env::set_var(key, "/env/receipts") };
    apply_env_overrides(&mut cfg);
    unsafe { std::env::remove_var(key) };
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/env/receipts"));
}

#[test]
#[ignore = "env-var tests are inherently racy in parallel test runners"]
fn env_override_workspace_dir_and_load_config() {
    let key = "ABP_WORKSPACE_DIR";
    // Direct apply_env_overrides.
    let mut cfg = parse_toml("").unwrap();
    unsafe { std::env::set_var(key, "/env/workspace") };
    apply_env_overrides(&mut cfg);
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/env/workspace"));
    // Also verify load_config applies env overrides from file.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bp.toml");
    std::fs::write(&path, "").unwrap();
    let cfg2 = load_config(Some(&path)).unwrap();
    assert_eq!(cfg2.workspace_dir.as_deref(), Some("/env/workspace"));
    unsafe { std::env::remove_var(key) };
}

// ===========================================================================
// 9. File path resolution in config
// ===========================================================================

#[test]
fn load_config_from_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    std::fs::write(&path, r#"default_backend = "mock""#).unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn load_config_missing_file_returns_file_not_found() {
    let err = load_config(Some(Path::new("nonexistent_file_xyz_does_not_exist.toml"))).unwrap_err();
    assert!(matches!(err, ConfigError::FileNotFound { .. }));
}

#[test]
fn load_config_file_not_found_includes_path_in_message() {
    let p = Path::new("/definitely/does/not/exist.toml");
    let err = load_config(Some(p)).unwrap_err();
    match err {
        ConfigError::FileNotFound { path } => {
            assert!(path.contains("exist.toml"));
        }
        other => panic!("expected FileNotFound, got {other:?}"),
    }
}

#[test]
fn load_config_none_returns_defaults() {
    // Clean env to avoid interference.
    for key in &[
        "ABP_DEFAULT_BACKEND",
        "ABP_LOG_LEVEL",
        "ABP_RECEIPTS_DIR",
        "ABP_WORKSPACE_DIR",
    ] {
        // SAFETY: test is single-threaded with respect to these env vars.
        unsafe { std::env::remove_var(key) };
    }
    let cfg = load_config(None).unwrap();
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
}

#[test]
fn load_config_from_deeply_nested_path() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&nested).unwrap();
    let path = nested.join("config.toml");
    std::fs::write(&path, "receipts_dir = \"/r\"").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn relative_paths_in_config_values_preserved() {
    let toml = r#"
workspace_dir = "./relative/ws"
receipts_dir = "../parent/receipts"
"#;
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("./relative/ws"));
    assert_eq!(cfg.receipts_dir.as_deref(), Some("../parent/receipts"));
}

#[test]
fn windows_paths_in_config_values_preserved() {
    let toml = "workspace_dir = \"C:\\\\Users\\\\agent\\\\ws\"";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("C:\\Users\\agent\\ws"));
}

// ===========================================================================
// 10. Duration/timeout value parsing
// ===========================================================================

#[test]
fn timeout_secs_parses_as_integer() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 300";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(300)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn timeout_secs_min_value_1() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 1";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(1)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn timeout_secs_max_boundary_86400() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 86400";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(86_400)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn timeout_secs_large_value_parses_but_validation_catches() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 999999";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(999_999)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
    // Parsing succeeds; validation rejects.
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn timeout_secs_zero_parses_but_validation_catches() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 0";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { timeout_secs, .. } => assert_eq!(*timeout_secs, Some(0)),
        other => panic!("expected Sidecar, got {other:?}"),
    }
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn timeout_secs_negative_fails_parse() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = -1";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn timeout_secs_float_fails_parse() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 1.5";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

// ===========================================================================
// 11. Empty/whitespace TOML file handling
// ===========================================================================

#[test]
fn whitespace_only_toml_parses() {
    let cfg = parse_toml("   \n\t\n   ").unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn comment_only_toml_parses() {
    let toml = "# Comment\n# Another comment\n# default_backend = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn newlines_only_toml_parses() {
    let cfg = parse_toml("\n\n\n\n\n").unwrap();
    assert!(cfg.default_backend.is_none());
}

#[test]
fn empty_file_on_disk_parses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.toml");
    std::fs::write(&path, "").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn whitespace_file_on_disk_parses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ws.toml");
    std::fs::write(&path, "  \n\t \n  ").unwrap();
    let cfg = load_config(Some(&path)).unwrap();
    assert!(cfg.backends.is_empty());
}

// ===========================================================================
// 12. Unicode in config values
// ===========================================================================

#[test]
fn unicode_in_default_backend() {
    let cfg = parse_toml(r#"default_backend = "バックエンド""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("バックエンド"));
}

#[test]
fn unicode_in_workspace_dir() {
    let cfg = parse_toml(r#"workspace_dir = "/tmp/日本語/workspace""#).unwrap();
    assert_eq!(cfg.workspace_dir.as_deref(), Some("/tmp/日本語/workspace"));
}

#[test]
fn unicode_in_receipts_dir() {
    let cfg = parse_toml(r#"receipts_dir = "/données/reçus""#).unwrap();
    assert_eq!(cfg.receipts_dir.as_deref(), Some("/données/reçus"));
}

#[test]
fn unicode_in_sidecar_command_and_args() {
    let toml =
        "[backends.uni]\ntype = \"sidecar\"\ncommand = \"nöde\"\nargs = [\"—flag\", \"日本語.js\"]";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["uni"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "nöde");
            assert_eq!(args[1], "日本語.js");
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn emoji_in_string_values() {
    let cfg = parse_toml(r#"default_backend = "🚀backend""#).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("🚀backend"));
}

#[test]
fn unicode_backend_key() {
    let toml = "[backends.\"café\"]\ntype = \"mock\"";
    let cfg = parse_toml(toml).unwrap();
    assert!(cfg.backends.contains_key("café"));
}

// ===========================================================================
// 13. Very large config files parse efficiently
// ===========================================================================

#[test]
fn hundred_backends_parse() {
    let mut toml = String::new();
    for i in 0..100 {
        toml.push_str(&format!("[backends.mock_{i}]\ntype = \"mock\"\n\n"));
    }
    let cfg = parse_toml(&toml).unwrap();
    assert_eq!(cfg.backends.len(), 100);
}

#[test]
fn large_sidecar_args_list() {
    let args: Vec<String> = (0..500).map(|i| format!("\"arg_{i}\"")).collect();
    let args_str = args.join(", ");
    let toml =
        format!("[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = [{args_str}]\n");
    let cfg = parse_toml(&toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert_eq!(args.len(), 500),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn large_string_value_parses() {
    let long_val = "x".repeat(100_000);
    let toml = format!("default_backend = \"{long_val}\"");
    let cfg = parse_toml(&toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some(long_val.as_str()));
}

#[test]
fn fifty_mixed_backends_parse() {
    let mut toml = String::from("default_backend = \"mock_0\"\n");
    for i in 0..50 {
        if i % 2 == 0 {
            toml.push_str(&format!("[backends.mock_{i}]\ntype = \"mock\"\n\n"));
        } else {
            toml.push_str(&format!(
                "[backends.sc_{i}]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = [\"host.js\"]\ntimeout_secs = {}\n\n",
                i * 10
            ));
        }
    }
    let cfg = parse_toml(&toml).unwrap();
    assert_eq!(cfg.backends.len(), 50);
}

// ===========================================================================
// 14. Config validation after parsing
// ===========================================================================

#[test]
fn parsed_empty_config_validates_with_warnings() {
    let cfg = parse_toml("").unwrap();
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings.is_empty(),
        "empty config should have advisory warnings"
    );
}

#[test]
fn parsed_full_config_validates_cleanly() {
    let toml = r#"
default_backend = "mock"
log_level = "info"
receipts_dir = "/tmp/receipts"

[backends.mock]
type = "mock"
"#;
    let cfg = parse_toml(toml).unwrap();
    let warnings = validate_config(&cfg).unwrap();
    assert!(
        !warnings.iter().any(|w| matches!(
            w,
            ConfigWarning::MissingOptionalField { field, .. } if field == "default_backend"
        )),
        "should not warn about default_backend"
    );
}

#[test]
fn parsed_config_invalid_log_level_fails_validation() {
    let cfg = parse_toml("log_level = \"verbose\"").unwrap();
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn parsed_config_zero_timeout_fails_validation() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\ntimeout_secs = 0";
    let cfg = parse_toml(toml).unwrap();
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn parsed_config_empty_command_fails_validation() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"\"";
    let cfg = parse_toml(toml).unwrap();
    let err = validate_config(&cfg).unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError { .. }));
}

#[test]
fn parsed_config_large_timeout_warns() {
    let toml = r#"
default_backend = "sc"
receipts_dir = "/r"

[backends.sc]
type = "sidecar"
command = "node"
timeout_secs = 7200
"#;
    let cfg = parse_toml(toml).unwrap();
    let warnings = validate_config(&cfg).unwrap();
    assert!(warnings
        .iter()
        .any(|w| matches!(w, ConfigWarning::LargeTimeout { .. })));
}

// ===========================================================================
// 15. Config merge (file + env + CLI overrides)
// ===========================================================================

#[test]
fn merge_two_parsed_configs() {
    let base = parse_toml(
        "default_backend = \"mock\"\nlog_level = \"info\"\n\n[backends.mock]\ntype = \"mock\"",
    )
    .unwrap();
    let overlay = parse_toml(
        "log_level = \"debug\"\n\n[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"",
    )
    .unwrap();
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
    assert_eq!(merged.backends.len(), 2);
}

#[test]
fn merge_empty_overlay_preserves_base() {
    let base = parse_toml(
        "default_backend = \"mock\"\nworkspace_dir = \"/ws\"\nlog_level = \"info\"\nreceipts_dir = \"/r\"",
    )
    .unwrap();
    let overlay = parse_toml("").unwrap();
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/ws"));
    assert_eq!(merged.receipts_dir.as_deref(), Some("/r"));
}

#[test]
fn merge_overlay_replaces_backend_entry() {
    let base = parse_toml("[backends.sc]\ntype = \"sidecar\"\ncommand = \"python\"").unwrap();
    let overlay =
        parse_toml("[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = [\"host.js\"]")
            .unwrap();
    let merged = merge_configs(base, overlay);
    match &merged.backends["sc"] {
        BackendEntry::Sidecar { command, args, .. } => {
            assert_eq!(command, "node");
            assert_eq!(args, &["host.js"]);
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn merge_adds_new_backends_from_overlay() {
    let base = parse_toml("[backends.a]\ntype = \"mock\"").unwrap();
    let overlay =
        parse_toml("[backends.b]\ntype = \"mock\"\n[backends.c]\ntype = \"mock\"").unwrap();
    let merged = merge_configs(base, overlay);
    assert_eq!(merged.backends.len(), 3);
    assert!(merged.backends.contains_key("a"));
    assert!(merged.backends.contains_key("b"));
    assert!(merged.backends.contains_key("c"));
}

#[test]
fn three_way_merge_file_env_cli() {
    let file_config = parse_toml(
        r#"
default_backend = "mock"
log_level = "info"
receipts_dir = "/file/receipts"

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();

    // Simulate env overlay
    let env_overlay = BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };

    // Simulate CLI overlay — explicitly set log_level to None so it
    // doesn't clobber the env overlay (Default::default sets it to Some("info")).
    let cli_overlay = BackplaneConfig {
        default_backend: Some("openai".into()),
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::from([(
            "openai".into(),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec!["openai.js".into()],
                timeout_secs: Some(300),
            },
        )]),
        ..Default::default()
    };

    let step1 = merge_configs(file_config, env_overlay);
    let final_cfg = merge_configs(step1, cli_overlay);

    assert_eq!(final_cfg.default_backend.as_deref(), Some("openai"));
    assert_eq!(final_cfg.log_level.as_deref(), Some("debug"));
    assert_eq!(final_cfg.receipts_dir.as_deref(), Some("/file/receipts"));
    assert_eq!(final_cfg.backends.len(), 2);
    assert!(final_cfg.backends.contains_key("mock"));
    assert!(final_cfg.backends.contains_key("openai"));
}

#[test]
fn merge_defaults_do_not_clobber_base() {
    let base = parse_toml("default_backend = \"mock\"\nworkspace_dir = \"/my/ws\"").unwrap();
    let overlay = BackplaneConfig::default();
    let merged = merge_configs(base, overlay);
    // overlay.default_backend is None, so base value preserved.
    assert_eq!(merged.default_backend.as_deref(), Some("mock"));
    assert_eq!(merged.workspace_dir.as_deref(), Some("/my/ws"));
}

// ===========================================================================
// Additional TOML syntax edge cases
// ===========================================================================

#[test]
fn toml_literal_string_in_command() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = 'C:\\Program Files\\node\\node.exe'";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { command, .. } => {
            assert_eq!(command, r"C:\Program Files\node\node.exe");
        }
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_triple_quoted_string() {
    let toml = "default_backend = \"\"\"mock\"\"\"";
    let cfg = parse_toml(toml).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
}

#[test]
fn invalid_toml_syntax_gives_parse_error() {
    let err = parse_toml("this is [not valid toml =").unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn duplicate_key_gives_parse_error() {
    let toml = "default_backend = \"a\"\ndefault_backend = \"b\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn invalid_backend_type_gives_parse_error() {
    let toml = "[backends.bad]\ntype = \"nonexistent\"\ncommand = \"node\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn missing_type_field_in_backend_gives_parse_error() {
    let toml = "[backends.bad]\ncommand = \"node\"";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn sidecar_missing_required_command_gives_parse_error() {
    let toml = "[backends.bad]\ntype = \"sidecar\"\nargs = [\"host.js\"]";
    let err = parse_toml(toml).unwrap_err();
    assert!(matches!(err, ConfigError::ParseError { .. }));
}

#[test]
fn trailing_comma_in_toml_array_allowed() {
    // TOML 1.0 allows trailing commas in arrays.
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = [\"a\", \"b\",]";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert_eq!(args, &["a", "b"]),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn toml_serialization_deterministic() {
    let cfg = fully_valid_config();
    let s1 = toml::to_string(&cfg).unwrap();
    let s2 = toml::to_string(&cfg).unwrap();
    assert_eq!(s1, s2, "serialization should be deterministic");
}

#[test]
fn parse_then_serialize_roundtrip() {
    let original = r#"
default_backend = "mock"
log_level = "debug"
receipts_dir = "/receipts"
workspace_dir = "/ws"

[backends.mock]
type = "mock"

[backends.node]
type = "sidecar"
command = "node"
args = ["--flag", "host.js"]
timeout_secs = 120
"#;
    let cfg = parse_toml(original).unwrap();
    let serialized = toml::to_string(&cfg).unwrap();
    let cfg2 = parse_toml(&serialized).unwrap();
    assert_eq!(cfg, cfg2);
}

#[test]
fn empty_args_array_roundtrips() {
    let toml = "[backends.sc]\ntype = \"sidecar\"\ncommand = \"node\"\nargs = []";
    let cfg = parse_toml(toml).unwrap();
    match &cfg.backends["sc"] {
        BackendEntry::Sidecar { args, .. } => assert!(args.is_empty()),
        other => panic!("expected Sidecar, got {other:?}"),
    }
}

#[test]
fn config_error_display_parse_error_contains_reason() {
    let err = parse_toml("log_level = 42").unwrap_err();
    let display = err.to_string();
    assert!(display.contains("failed to parse config"));
}

#[test]
fn config_error_display_file_not_found_contains_path() {
    let err = load_config(Some(Path::new("nope.toml"))).unwrap_err();
    let display = err.to_string();
    assert!(display.contains("nope.toml"));
}
