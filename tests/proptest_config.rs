// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for the configuration system.

use proptest::prelude::*;
use std::collections::BTreeMap;

use abp_config::{BackendEntry, BackplaneConfig, merge_configs, parse_toml, validate_config};

// ── Strategies ──────────────────────────────────────────────────────────

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

/// A config with all fields None/empty — the merge identity element.
fn identity_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: None,
        workspace_dir: None,
        log_level: None,
        receipts_dir: None,
        backends: BTreeMap::new(),
    }
}

fn arb_valid_log_level() -> BoxedStrategy<String> {
    prop_oneof![
        Just("error".to_owned()),
        Just("warn".to_owned()),
        Just("info".to_owned()),
        Just("debug".to_owned()),
        Just("trace".to_owned()),
    ]
    .boxed()
}

fn arb_backend_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_-]{0,19}".boxed()
}

fn arb_command() -> BoxedStrategy<String> {
    prop_oneof![
        Just("node".to_owned()),
        Just("python3".to_owned()),
        Just("python".to_owned()),
        Just("ruby".to_owned()),
        "[a-z][a-z0-9_]{0,14}"
            .prop_map(|s| s)
            .prop_filter("command must not be empty", |s| !s.trim().is_empty()),
    ]
    .boxed()
}

fn arb_arg() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_./-]{1,30}".boxed()
}

fn arb_valid_timeout() -> BoxedStrategy<Option<u64>> {
    prop_oneof![Just(None), (1u64..=86_400u64).prop_map(Some),].boxed()
}

fn arb_mock_backend() -> BoxedStrategy<BackendEntry> {
    Just(BackendEntry::Mock {}).boxed()
}

fn arb_sidecar_backend() -> BoxedStrategy<BackendEntry> {
    (
        arb_command(),
        prop::collection::vec(arb_arg(), 0..4),
        arb_valid_timeout(),
    )
        .prop_map(|(command, args, timeout_secs)| BackendEntry::Sidecar {
            command,
            args,
            timeout_secs,
        })
        .boxed()
}

fn arb_backend_entry() -> BoxedStrategy<BackendEntry> {
    prop_oneof![arb_mock_backend(), arb_sidecar_backend(),].boxed()
}

fn arb_backends() -> BoxedStrategy<BTreeMap<String, BackendEntry>> {
    prop::collection::btree_map(arb_backend_name(), arb_backend_entry(), 0..5).boxed()
}

fn arb_optional_string() -> BoxedStrategy<Option<String>> {
    prop_oneof![Just(None), "[a-zA-Z0-9_/.-]{1,30}".prop_map(Some),].boxed()
}

/// OS-agnostic path strings including Windows and Unix formats.
fn arb_path_string() -> BoxedStrategy<Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some("/tmp/data".to_owned())),
        Just(Some("./data/receipts".to_owned())),
        Just(Some("C:\\Users\\test\\data".to_owned())),
        Just(Some("/home/user/workspace".to_owned())),
        Just(Some("relative/path/here".to_owned())),
        Just(Some("D:\\Projects\\agent".to_owned())),
        "[a-zA-Z0-9_./-]{1,50}".prop_map(Some),
    ]
    .boxed()
}

/// Generate a valid BackplaneConfig (one that passes validation).
fn arb_valid_config() -> BoxedStrategy<BackplaneConfig> {
    (
        arb_optional_string(),
        arb_path_string(),
        prop_oneof![Just(None), arb_valid_log_level().prop_map(Some)],
        arb_path_string(),
        arb_backends(),
    )
        .prop_map(
            |(default_backend, workspace_dir, log_level, receipts_dir, backends)| BackplaneConfig {
                default_backend,
                workspace_dir,
                log_level,
                receipts_dir,
                backends,
            },
        )
        .boxed()
}

/// Generate a BackplaneConfig with all optional fields populated.
fn arb_fully_populated_config() -> BoxedStrategy<BackplaneConfig> {
    (
        "[a-z][a-z0-9_]{0,14}".prop_map(Some),
        "[a-zA-Z0-9_/.-]{1,30}".prop_map(Some),
        arb_valid_log_level().prop_map(Some),
        "[a-zA-Z0-9_/.-]{1,30}".prop_map(Some),
        prop::collection::btree_map(arb_backend_name(), arb_backend_entry(), 1..5),
    )
        .prop_map(
            |(default_backend, workspace_dir, log_level, receipts_dir, backends)| BackplaneConfig {
                default_backend,
                workspace_dir,
                log_level,
                receipts_dir,
                backends,
            },
        )
        .boxed()
}

/// Generate a BackplaneConfig with no optional fields set.
fn arb_minimal_config() -> BoxedStrategy<BackplaneConfig> {
    arb_backends()
        .prop_map(|backends| BackplaneConfig {
            default_backend: None,
            workspace_dir: None,
            log_level: None,
            receipts_dir: None,
            backends,
        })
        .boxed()
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Serde roundtrips
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Any valid BackplaneConfig survives a JSON serialize→deserialize cycle.
    #[test]
    fn json_serde_roundtrip(cfg in arb_valid_config()) {
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: BackplaneConfig = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(cfg, cfg2);
    }

    /// Any valid BackplaneConfig survives a TOML serialize→parse cycle.
    #[test]
    fn toml_serde_roundtrip(cfg in arb_valid_config()) {
        let toml_str = toml::to_string(&cfg).unwrap();
        let cfg2 = parse_toml(&toml_str).unwrap();
        prop_assert_eq!(cfg, cfg2);
    }

    /// BackendEntry::Mock roundtrips through JSON.
    #[test]
    fn mock_backend_json_roundtrip(_i in 0..10u32) {
        let entry = BackendEntry::Mock {};
        let json = serde_json::to_string(&entry).unwrap();
        let entry2: BackendEntry = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(entry, entry2);
    }

    /// BackendEntry::Sidecar roundtrips through JSON.
    #[test]
    fn sidecar_backend_json_roundtrip(entry in arb_sidecar_backend()) {
        let json = serde_json::to_string(&entry).unwrap();
        let entry2: BackendEntry = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(entry, entry2);
    }

    /// BackendEntry roundtrips through TOML via a wrapper config.
    #[test]
    fn backend_entry_toml_roundtrip(entry in arb_backend_entry()) {
        let cfg = BackplaneConfig {
            backends: BTreeMap::from([("test".into(), entry.clone())]),
            ..identity_config()
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let cfg2 = parse_toml(&toml_str).unwrap();
        prop_assert_eq!(&cfg.backends["test"], &cfg2.backends["test"]);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  Merge properties
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// merge(a, merge(b, c)) == merge(merge(a, b), c)  — associativity.
    #[test]
    fn merge_is_associative(
        a in arb_valid_config(),
        b in arb_valid_config(),
        c in arb_valid_config(),
    ) {
        let left = merge_configs(a.clone(), merge_configs(b.clone(), c.clone()));
        let right = merge_configs(merge_configs(a, b), c);
        prop_assert_eq!(left, right);
    }

    /// merge(a, identity) == a — right identity.
    #[test]
    fn merge_right_identity(a in arb_valid_config()) {
        let merged = merge_configs(a.clone(), identity_config());
        prop_assert_eq!(a, merged);
    }

    /// merge(identity, a) == a — left identity.
    #[test]
    fn merge_left_identity(a in arb_valid_config()) {
        let merged = merge_configs(identity_config(), a.clone());
        prop_assert_eq!(a, merged);
    }

    /// Overlay backends always appear in the merged result.
    #[test]
    fn merge_overlay_backends_present(
        base in arb_valid_config(),
        overlay in arb_valid_config(),
    ) {
        let merged = merge_configs(base, overlay.clone());
        for key in overlay.backends.keys() {
            prop_assert!(merged.backends.contains_key(key));
            prop_assert_eq!(&merged.backends[key], &overlay.backends[key]);
        }
    }

    /// Overlay Option fields win when set.
    #[test]
    fn merge_overlay_option_wins(
        base in arb_valid_config(),
        overlay in arb_fully_populated_config(),
    ) {
        let merged = merge_configs(base, overlay.clone());
        prop_assert_eq!(merged.default_backend, overlay.default_backend);
        prop_assert_eq!(merged.workspace_dir, overlay.workspace_dir);
        prop_assert_eq!(merged.log_level, overlay.log_level);
        prop_assert_eq!(merged.receipts_dir, overlay.receipts_dir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Validation properties
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Default config always passes validation.
    #[test]
    fn default_config_validates(_i in 0..10u32) {
        let cfg = BackplaneConfig::default();
        validate_config(&cfg).expect("default config must validate");
    }

    /// Any generated valid config passes validation.
    #[test]
    fn valid_generated_config_validates(cfg in arb_valid_config()) {
        validate_config(&cfg).expect("valid generated config must validate");
    }

    /// Fully populated configs pass validation.
    #[test]
    fn fully_populated_config_validates(cfg in arb_fully_populated_config()) {
        validate_config(&cfg).expect("fully populated config must validate");
    }

    /// Minimal configs (no optional fields) pass validation.
    #[test]
    fn minimal_config_validates(cfg in arb_minimal_config()) {
        validate_config(&cfg).expect("minimal config must validate");
    }

    /// Invalid log level always causes a validation error.
    #[test]
    fn invalid_log_level_rejected(
        level in "[a-zA-Z]{1,10}"
            .prop_filter("must not be a valid level", |l| {
                !["error", "warn", "info", "debug", "trace"].contains(&l.as_str())
            }),
    ) {
        let cfg = BackplaneConfig {
            log_level: Some(level),
            ..identity_config()
        };
        let err = validate_config(&cfg).unwrap_err();
        match err {
            abp_config::ConfigError::ValidationError { reasons } => {
                prop_assert!(reasons.iter().any(|r| r.contains("log_level")));
            }
            other => prop_assert!(false, "expected ValidationError, got {:?}", other),
        }
    }

    /// Empty sidecar command always causes a validation error.
    #[test]
    fn empty_command_rejected(
        name in arb_backend_name(),
        whitespace in prop_oneof![Just("".to_owned()), Just("  ".to_owned()), Just("\t".to_owned())],
    ) {
        let mut cfg = identity_config();
        cfg.backends.insert(
            name,
            BackendEntry::Sidecar {
                command: whitespace,
                args: vec![],
                timeout_secs: None,
            },
        );
        let err = validate_config(&cfg).unwrap_err();
        match err {
            abp_config::ConfigError::ValidationError { reasons } => {
                prop_assert!(reasons.iter().any(|r| r.contains("command must not be empty")));
            }
            other => prop_assert!(false, "expected ValidationError, got {:?}", other),
        }
    }

    /// Empty backend name is always rejected.
    #[test]
    fn empty_backend_name_rejected(entry in arb_backend_entry()) {
        let mut cfg = identity_config();
        cfg.backends.insert(String::new(), entry);
        let err = validate_config(&cfg).unwrap_err();
        match err {
            abp_config::ConfigError::ValidationError { reasons } => {
                prop_assert!(reasons.iter().any(|r| r.contains("name must not be empty")));
            }
            other => prop_assert!(false, "expected ValidationError, got {:?}", other),
        }
    }

    /// Zero timeout is always rejected.
    #[test]
    fn zero_timeout_rejected(name in arb_backend_name()) {
        let mut cfg = identity_config();
        cfg.backends.insert(
            name,
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        );
        let err = validate_config(&cfg).unwrap_err();
        match err {
            abp_config::ConfigError::ValidationError { reasons } => {
                prop_assert!(reasons.iter().any(|r| r.contains("out of range")));
            }
            other => prop_assert!(false, "expected ValidationError, got {:?}", other),
        }
    }

    /// Timeout exceeding 86 400 is always rejected.
    #[test]
    fn excessive_timeout_rejected(
        name in arb_backend_name(),
        timeout in 86_401u64..=u64::MAX / 2,
    ) {
        let mut cfg = identity_config();
        cfg.backends.insert(
            name,
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(timeout),
            },
        );
        let err = validate_config(&cfg).unwrap_err();
        match err {
            abp_config::ConfigError::ValidationError { reasons } => {
                prop_assert!(reasons.iter().any(|r| r.contains("out of range")));
            }
            other => prop_assert!(false, "expected ValidationError, got {:?}", other),
        }
    }

    /// Valid timeout range (1..=86_400) always passes for a sidecar backend.
    #[test]
    fn valid_timeout_range_accepted(
        name in arb_backend_name(),
        timeout in 1u64..=86_400u64,
    ) {
        let mut cfg = identity_config();
        cfg.backends.insert(
            name,
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(timeout),
            },
        );
        validate_config(&cfg).expect("valid timeout must be accepted");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  TOML parsing
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Config → TOML string → parse → identical config.
    #[test]
    fn toml_parse_roundtrip(cfg in arb_valid_config()) {
        let toml_str = toml::to_string(&cfg).unwrap();
        let parsed = parse_toml(&toml_str).unwrap();
        prop_assert_eq!(cfg, parsed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5  File path handling
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Various OS path formats survive TOML roundtrip via workspace_dir.
    #[test]
    fn workspace_dir_path_roundtrip(path in arb_path_string()) {
        let cfg = BackplaneConfig {
            workspace_dir: path.clone(),
            ..identity_config()
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let cfg2 = parse_toml(&toml_str).unwrap();
        prop_assert_eq!(cfg.workspace_dir, cfg2.workspace_dir);
    }

    /// Various OS path formats survive TOML roundtrip via receipts_dir.
    #[test]
    fn receipts_dir_path_roundtrip(path in arb_path_string()) {
        let cfg = BackplaneConfig {
            receipts_dir: path.clone(),
            ..identity_config()
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let cfg2 = parse_toml(&toml_str).unwrap();
        prop_assert_eq!(cfg.receipts_dir, cfg2.receipts_dir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6  Validation + serialization coherence
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Any generated config that passes validation also serializes to
    /// valid TOML and JSON without error.
    #[test]
    fn valid_config_serializes_cleanly(cfg in arb_valid_config()) {
        if validate_config(&cfg).is_ok() {
            let toml_result = toml::to_string(&cfg);
            prop_assert!(toml_result.is_ok(), "TOML serialization failed: {:?}", toml_result.err());
            let json_result = serde_json::to_string(&cfg);
            prop_assert!(json_result.is_ok(), "JSON serialization failed: {:?}", json_result.err());
        }
    }

    /// Validated config roundtrips through TOML and remains valid.
    #[test]
    fn validated_config_stays_valid_after_roundtrip(cfg in arb_valid_config()) {
        if validate_config(&cfg).is_ok() {
            let toml_str = toml::to_string(&cfg).unwrap();
            let cfg2 = parse_toml(&toml_str).unwrap();
            validate_config(&cfg2).expect("roundtripped config must still validate");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7  Edge cases
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Merging a config with itself is idempotent.
    #[test]
    fn merge_self_idempotent(cfg in arb_valid_config()) {
        let merged = merge_configs(cfg.clone(), cfg.clone());
        prop_assert_eq!(cfg, merged);
    }

    /// Multiple backend entries with distinct names all survive roundtrip.
    #[test]
    fn multiple_backends_roundtrip(backends in arb_backends()) {
        let cfg = BackplaneConfig {
            backends: backends.clone(),
            ..identity_config()
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let cfg2 = parse_toml(&toml_str).unwrap();
        prop_assert_eq!(cfg.backends.len(), cfg2.backends.len());
        for (name, entry) in &backends {
            prop_assert_eq!(Some(entry), cfg2.backends.get(name));
        }
    }

    /// Sidecar args list (including empty) survives roundtrip.
    #[test]
    fn sidecar_args_roundtrip(
        args in prop::collection::vec(arb_arg(), 0..8),
    ) {
        let entry = BackendEntry::Sidecar {
            command: "node".into(),
            args: args.clone(),
            timeout_secs: Some(300),
        };
        let cfg = BackplaneConfig {
            backends: BTreeMap::from([("test".into(), entry)]),
            ..identity_config()
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let cfg2 = parse_toml(&toml_str).unwrap();
        match &cfg2.backends["test"] {
            BackendEntry::Sidecar { args: rt_args, .. } => {
                prop_assert_eq!(&args, rt_args);
            }
            other => prop_assert!(false, "expected Sidecar, got {:?}", other),
        }
    }
}
