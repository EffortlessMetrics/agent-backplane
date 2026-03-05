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
//! Deep CLI integration tests for the `abp` binary.
//!
//! Tests cover argument parsing via [`clap::Parser::try_parse_from`],
//! output formatting, configuration loading, receipt subcommands,
//! backend selection, and edge-case validation.

use abp_cli::cli::{
    Cli, Commands, ConfigAction, LaneArg, ReceiptAction, SchemaArg, WorkspaceModeArg,
};
use abp_cli::config::{self, BackendConfig, BackplaneConfig};
use abp_cli::format::{Formatter, OutputFormat};
use clap::{error::ErrorKind, Parser};
use std::collections::HashMap;
use std::path::PathBuf;

/// Parse CLI arguments prefixed with the binary name.
fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("abp").chain(args.iter().copied()))
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Parse `run` command — basic and composite
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_task_hello_backend_mock() {
    let cli = parse(&["run", "--task", "hello", "--backend", "mock"]).unwrap();
    match &cli.command {
        Commands::Run { task, backend, .. } => {
            assert_eq!(task, "hello");
            assert_eq!(backend.as_deref(), Some("mock"));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_task_only_no_backend() {
    let cli = parse(&["run", "--task", "do stuff"]).unwrap();
    match &cli.command {
        Commands::Run { task, backend, .. } => {
            assert_eq!(task, "do stuff");
            assert!(backend.is_none());
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_with_model_and_root() {
    let cli = parse(&[
        "run", "--task", "t", "--model", "claude-3", "--root", "/src",
    ])
    .unwrap();
    match &cli.command {
        Commands::Run {
            model, root, task, ..
        } => {
            assert_eq!(task, "t");
            assert_eq!(model.as_deref(), Some("claude-3"));
            assert_eq!(root, "/src");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_out_and_output_both_set() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--out",
        "receipt.json",
        "--output",
        "output.json",
    ])
    .unwrap();
    match &cli.command {
        Commands::Run { out, output, .. } => {
            assert_eq!(out.as_deref(), Some(std::path::Path::new("receipt.json")));
            assert_eq!(output.as_deref(), Some(std::path::Path::new("output.json")));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_events_flag() {
    let cli = parse(&["run", "--task", "t", "--events", "stream.jsonl"]).unwrap();
    match &cli.command {
        Commands::Run { events, .. } => {
            assert_eq!(
                events.as_deref(),
                Some(std::path::Path::new("stream.jsonl"))
            );
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_policy_flag() {
    let cli = parse(&["run", "--task", "t", "--policy", "strict.json"]).unwrap();
    match &cli.command {
        Commands::Run { policy, .. } => {
            assert_eq!(policy.as_deref(), Some(std::path::Path::new("strict.json")));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Parse `backends` command
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backends_subcommand_parses_cleanly() {
    let cli = parse(&["backends"]).unwrap();
    assert!(matches!(cli.command, Commands::Backends { .. }));
    assert!(!cli.debug);
}

#[test]
fn backends_with_global_config() {
    let cli = parse(&["--config", "bp.toml", "backends"]).unwrap();
    assert!(matches!(cli.command, Commands::Backends { .. }));
    assert_eq!(cli.config, Some(PathBuf::from("bp.toml")));
}

#[test]
fn backends_rejects_unknown_arg() {
    let err = parse(&["backends", "--unknown"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Parse with all flags: --debug, --config, --format
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_and_config_before_run() {
    let cli = parse(&["--debug", "--config", "c.toml", "run", "--task", "t"]).unwrap();
    assert!(cli.debug);
    assert_eq!(cli.config, Some(PathBuf::from("c.toml")));
}

#[test]
fn debug_before_backends() {
    let cli = parse(&["--debug", "backends"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn config_path_with_spaces() {
    let cli = parse(&["--config", "path with spaces/bp.toml", "backends"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("path with spaces/bp.toml")));
}

#[test]
fn json_flag_on_run() {
    let cli = parse(&["run", "--task", "t", "--json"]).unwrap();
    match &cli.command {
        Commands::Run { json, .. } => assert!(json),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn json_flag_absent_defaults_false() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run { json, .. } => assert!(!json),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn all_global_and_run_flags_combined() {
    let cli = parse(&[
        "--debug",
        "--config",
        "my.toml",
        "run",
        "--task",
        "do it",
        "--backend",
        "sidecar:claude",
        "--model",
        "opus",
        "--root",
        "/work",
        "--workspace-mode",
        "pass-through",
        "--lane",
        "workspace-first",
        "--include",
        "*.rs",
        "--exclude",
        "target/**",
        "--param",
        "key=val",
        "--env",
        "K=V",
        "--max-budget-usd",
        "3.14",
        "--max-turns",
        "10",
        "--out",
        "r.json",
        "--output",
        "o.json",
        "--events",
        "e.jsonl",
        "--json",
        "--policy",
        "p.json",
    ])
    .unwrap();
    assert!(cli.debug);
    assert_eq!(cli.config, Some(PathBuf::from("my.toml")));
    match &cli.command {
        Commands::Run {
            task,
            backend,
            model,
            root,
            workspace_mode,
            lane,
            include,
            exclude,
            params,
            env_vars,
            max_budget_usd,
            max_turns,
            out,
            json,
            policy,
            output,
            events,
            ..
        } => {
            assert_eq!(task, "do it");
            assert_eq!(backend.as_deref(), Some("sidecar:claude"));
            assert_eq!(model.as_deref(), Some("opus"));
            assert_eq!(root, "/work");
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
            assert!(matches!(lane, LaneArg::WorkspaceFirst));
            assert_eq!(include, &["*.rs"]);
            assert_eq!(exclude, &["target/**"]);
            assert_eq!(params, &["key=val"]);
            assert_eq!(env_vars, &["K=V"]);
            assert!((max_budget_usd.unwrap() - 3.14).abs() < f64::EPSILON);
            assert_eq!(*max_turns, Some(10));
            assert_eq!(out.as_deref(), Some(std::path::Path::new("r.json")));
            assert!(json);
            assert_eq!(policy.as_deref(), Some(std::path::Path::new("p.json")));
            assert_eq!(output.as_deref(), Some(std::path::Path::new("o.json")));
            assert_eq!(events.as_deref(), Some(std::path::Path::new("e.jsonl")));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Invalid commands — missing required args → error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_missing_task_is_missing_required() {
    let err = parse(&["run"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn run_missing_task_with_backend_is_error() {
    let err = parse(&["run", "--backend", "mock"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn no_subcommand_is_error() {
    let err = parse(&[]).unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
}

#[test]
fn unknown_subcommand_rejected() {
    let err = parse(&["foobar"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn unknown_flag_on_run_rejected() {
    let err = parse(&["run", "--task", "t", "--bogus-flag"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn invalid_workspace_mode_is_invalid_value() {
    let err = parse(&["run", "--task", "t", "--workspace-mode", "imaginary"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn invalid_lane_is_invalid_value() {
    let err = parse(&["run", "--task", "t", "--lane", "chaos"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn max_budget_usd_non_numeric() {
    let err = parse(&["run", "--task", "t", "--max-budget-usd", "free"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn max_turns_non_numeric() {
    let err = parse(&["run", "--task", "t", "--max-turns", "many"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn max_turns_negative_rejected() {
    let err = parse(&["run", "--task", "t", "--max-turns", "-5"]).unwrap_err();
    assert!(err.kind() == ErrorKind::ValueValidation || err.kind() == ErrorKind::UnknownArgument);
}

#[test]
fn validate_no_args_parses_as_config_mode() {
    let cli = parse(&["validate"]).unwrap();
    match &cli.command {
        Commands::Validate { file, config_file } => {
            assert!(file.is_none());
            assert!(config_file.is_none());
        }
        other => panic!("expected Validate, got {other:?}"),
    }
}

#[test]
fn inspect_missing_file_arg() {
    let err = parse(&["inspect"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn schema_missing_kind_arg() {
    let err = parse(&["schema"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn schema_invalid_kind_rejected() {
    let err = parse(&["schema", "dinosaur"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn extra_positional_on_run_rejected() {
    let err = parse(&["run", "--task", "t", "stray"]).unwrap_err();
    assert!(err.kind() == ErrorKind::UnknownArgument || err.to_string().contains("unexpected"));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Help text
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn help_flag_triggers_display_help() {
    let err = parse(&["--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn help_short_h() {
    let err = parse(&["-h"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn run_help_flag() {
    let err = parse(&["run", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn backends_help_flag() {
    let err = parse(&["backends", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn config_help_flag() {
    let err = parse(&["config", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn receipt_help_flag() {
    let err = parse(&["receipt", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn schema_help_flag() {
    let err = parse(&["schema", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Version
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn version_flag_triggers_display_version() {
    let err = parse(&["--version"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayVersion);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Backend selection — various backend strings accepted at parse level
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_mock_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "mock"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("mock")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_node_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:node"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:node")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_claude_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:claude"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:claude")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_copilot_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:copilot"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:copilot")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_python_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:python"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:python")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_kimi_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:kimi"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:kimi")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_gemini_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:gemini"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:gemini")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_sidecar_codex_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:codex"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:codex")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_short_alias_node() {
    let cli = parse(&["run", "--task", "t", "--backend", "node"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("node")),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn backend_arbitrary_custom_name_accepted_at_parse() {
    let cli = parse(&["run", "--task", "t", "--backend", "my-custom-backend"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => {
            assert_eq!(backend.as_deref(), Some("my-custom-backend"));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Config file — --config path parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_absolute_path() {
    let cli = parse(&["--config", "/etc/abp/config.toml", "backends"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("/etc/abp/config.toml")));
}

#[test]
fn config_relative_path() {
    let cli = parse(&["--config", "configs/dev.toml", "run", "--task", "t"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("configs/dev.toml")));
}

#[test]
fn config_absent_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    assert!(cli.config.is_none());
}

#[test]
fn config_load_none_returns_default() {
    let cfg = config::load_config(None).unwrap();
    assert!(cfg.backends.is_empty());
    assert!(cfg.default_backend.is_none());
}

#[test]
fn config_load_valid_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bp.toml");
    std::fs::write(
        &path,
        r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"
"#,
    )
    .unwrap();
    let cfg = config::load_config(Some(&path)).unwrap();
    assert_eq!(cfg.default_backend.as_deref(), Some("mock"));
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.backends.contains_key("mock"));
}

#[test]
fn config_load_invalid_toml_gives_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[[[ not valid").unwrap();
    let err = config::load_config(Some(&path)).unwrap_err();
    assert!(err.to_string().contains("failed to parse"));
}

#[test]
fn config_validate_empty_succeeds() {
    config::validate_config(&BackplaneConfig::default()).unwrap();
}

#[test]
fn config_validate_sidecar_empty_command_fails() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "bad".into(),
            BackendConfig::Sidecar {
                command: "".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let errs = config::validate_config(&cfg).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, config::ConfigError::InvalidBackend { .. })));
}

#[test]
fn config_validate_excessive_timeout_fails() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(100_000),
            },
        )]),
        ..Default::default()
    };
    let errs = config::validate_config(&cfg).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, config::ConfigError::InvalidTimeout { .. })));
}

#[test]
fn config_validate_zero_timeout_fails() {
    let cfg = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: Some(0),
            },
        )]),
        ..Default::default()
    };
    let errs = config::validate_config(&cfg).unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, config::ConfigError::InvalidTimeout { value: 0 })));
}

#[test]
fn config_merge_overlay_wins() {
    let base = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("warn".into()),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        default_backend: Some("sidecar:claude".into()),
        ..Default::default()
    };
    let merged = config::merge_configs(base, overlay);
    assert_eq!(merged.default_backend.as_deref(), Some("sidecar:claude"));
    assert_eq!(merged.log_level.as_deref(), Some("warn"));
}

#[test]
fn config_merge_backends_combined() {
    let base = BackplaneConfig {
        backends: HashMap::from([("mock".into(), BackendConfig::Mock {})]),
        ..Default::default()
    };
    let overlay = BackplaneConfig {
        backends: HashMap::from([(
            "sc".into(),
            BackendConfig::Sidecar {
                command: "node".into(),
                args: vec![],
                timeout_secs: None,
            },
        )]),
        ..Default::default()
    };
    let merged = config::merge_configs(base, overlay);
    assert!(merged.backends.contains_key("mock"));
    assert!(merged.backends.contains_key("sc"));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Output format — parsing and formatting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn output_format_parse_json() {
    assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
}

#[test]
fn output_format_parse_json_pretty() {
    assert_eq!(
        "json-pretty".parse::<OutputFormat>().unwrap(),
        OutputFormat::JsonPretty
    );
}

#[test]
fn output_format_parse_json_pretty_underscore() {
    assert_eq!(
        "json_pretty".parse::<OutputFormat>().unwrap(),
        OutputFormat::JsonPretty
    );
}

#[test]
fn output_format_parse_text() {
    assert_eq!("text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
}

#[test]
fn output_format_parse_table() {
    assert_eq!(
        "table".parse::<OutputFormat>().unwrap(),
        OutputFormat::Table
    );
}

#[test]
fn output_format_parse_compact() {
    assert_eq!(
        "compact".parse::<OutputFormat>().unwrap(),
        OutputFormat::Compact
    );
}

#[test]
fn output_format_parse_unknown_rejected() {
    assert!("yaml".parse::<OutputFormat>().is_err());
}

#[test]
fn output_format_display_roundtrip() {
    for fmt in &[
        OutputFormat::Json,
        OutputFormat::JsonPretty,
        OutputFormat::Text,
        OutputFormat::Table,
        OutputFormat::Compact,
    ] {
        let s = fmt.to_string();
        let parsed: OutputFormat = s.parse().unwrap();
        assert_eq!(&parsed, fmt);
    }
}

#[test]
fn output_format_case_insensitive() {
    assert_eq!("JSON".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
    assert_eq!("Text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
    assert_eq!(
        "TABLE".parse::<OutputFormat>().unwrap(),
        OutputFormat::Table
    );
}

#[test]
fn formatter_receipt_json_is_valid_json() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let fmt = Formatter::new(OutputFormat::Json);
    let out = fmt.format_receipt(&receipt);
    let _: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
}

#[test]
fn formatter_receipt_json_pretty_is_valid_json() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let fmt = Formatter::new(OutputFormat::JsonPretty);
    let out = fmt.format_receipt(&receipt);
    let _: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
    assert!(out.contains('\n'), "pretty JSON should have newlines");
}

#[test]
fn formatter_receipt_text_has_outcome() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let fmt = Formatter::new(OutputFormat::Text);
    let out = fmt.format_receipt(&receipt);
    assert!(out.contains("complete"));
    assert!(out.contains("mock"));
}

#[test]
fn formatter_receipt_table_has_labels() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Failed)
        .build();
    let fmt = Formatter::new(OutputFormat::Table);
    let out = fmt.format_receipt(&receipt);
    assert!(out.contains("outcome"));
    assert!(out.contains("backend"));
    assert!(out.contains("failed"));
}

#[test]
fn formatter_receipt_compact_single_line() {
    let receipt = abp_core::ReceiptBuilder::new("mock")
        .outcome(abp_core::Outcome::Partial)
        .build();
    let fmt = Formatter::new(OutputFormat::Compact);
    let out = fmt.format_receipt(&receipt);
    assert!(!out.contains('\n'));
    assert!(out.contains("partial"));
}

#[test]
fn formatter_error_json() {
    let fmt = Formatter::new(OutputFormat::Json);
    let out = fmt.format_error("something broke");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"], "something broke");
}

#[test]
fn formatter_error_text() {
    let fmt = Formatter::new(OutputFormat::Text);
    let out = fmt.format_error("oh no");
    assert!(out.starts_with("Error:"));
    assert!(out.contains("oh no"));
}

#[test]
fn formatter_work_order_json() {
    let wo = abp_core::WorkOrderBuilder::new("test task").build();
    let fmt = Formatter::new(OutputFormat::Json);
    let out = fmt.format_work_order(&wo);
    let _: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Debug mode — flag placement
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_before_run_sets_true() {
    let cli = parse(&["--debug", "run", "--task", "t"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn debug_before_backends_sets_true() {
    let cli = parse(&["--debug", "backends"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn debug_absent_defaults_false() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    assert!(!cli.debug);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Unknown backend — parse-level acceptance, runtime rejection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_backend_string_accepted_at_parse_level() {
    // Backend is a free-form string; validation happens at runtime.
    let cli = parse(&["run", "--task", "t", "--backend", "does_not_exist"]).unwrap();
    match &cli.command {
        Commands::Run { backend, .. } => {
            assert_eq!(backend.as_deref(), Some("does_not_exist"));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn runtime_unknown_backend_lookup_returns_none() {
    let rt = abp_runtime::Runtime::with_default_backends();
    assert!(rt.backend("totally-fake-xyz").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Receipt actions — subcommand parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_verify_parses() {
    let cli = parse(&["receipt", "verify", "r.json"]).unwrap();
    match &cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Verify { file },
        } => assert_eq!(file, &PathBuf::from("r.json")),
        other => panic!("expected ReceiptCmd Verify, got {other:?}"),
    }
}

#[test]
fn receipt_diff_parses_two_files() {
    let cli = parse(&["receipt", "diff", "a.json", "b.json"]).unwrap();
    match &cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Diff { file1, file2 },
        } => {
            assert_eq!(file1, &PathBuf::from("a.json"));
            assert_eq!(file2, &PathBuf::from("b.json"));
        }
        other => panic!("expected ReceiptCmd Diff, got {other:?}"),
    }
}

#[test]
fn receipt_diff_missing_second_file() {
    let err = parse(&["receipt", "diff", "a.json"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn receipt_verify_missing_file() {
    let err = parse(&["receipt", "verify"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn receipt_no_action_is_error() {
    let err = parse(&["receipt"]).unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Other subcommands — validate, schema, inspect, config
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_parses_file_path() {
    let cli = parse(&["validate", "wo.json"]).unwrap();
    match &cli.command {
        Commands::Validate { file, .. } => assert_eq!(file, &Some(PathBuf::from("wo.json"))),
        other => panic!("expected Validate, got {other:?}"),
    }
}

#[test]
fn schema_work_order_parses() {
    let cli = parse(&["schema", "work-order"]).unwrap();
    match &cli.command {
        Commands::Schema { kind, .. } => assert!(matches!(kind, SchemaArg::WorkOrder)),
        other => panic!("expected Schema, got {other:?}"),
    }
}

#[test]
fn schema_receipt_parses() {
    let cli = parse(&["schema", "receipt"]).unwrap();
    match &cli.command {
        Commands::Schema { kind, .. } => assert!(matches!(kind, SchemaArg::Receipt)),
        other => panic!("expected Schema, got {other:?}"),
    }
}

#[test]
fn schema_config_parses() {
    let cli = parse(&["schema", "config"]).unwrap();
    match &cli.command {
        Commands::Schema { kind, .. } => assert!(matches!(kind, SchemaArg::Config)),
        other => panic!("expected Schema, got {other:?}"),
    }
}

#[test]
fn inspect_parses_file_path() {
    let cli = parse(&["inspect", "receipt.json"]).unwrap();
    match &cli.command {
        Commands::Inspect { file } => assert_eq!(file, &PathBuf::from("receipt.json")),
        other => panic!("expected Inspect, got {other:?}"),
    }
}

#[test]
fn config_check_no_path() {
    let cli = parse(&["config", "check"]).unwrap();
    match &cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert!(config.is_none()),
        other => panic!("expected ConfigCmd Check, got {other:?}"),
    }
}

#[test]
fn config_check_with_path() {
    let cli = parse(&["config", "check", "--config", "c.toml"]).unwrap();
    match &cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert_eq!(config.as_deref(), Some(std::path::Path::new("c.toml"))),
        other => panic!("expected ConfigCmd Check, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Default values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_defaults_root_to_dot() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "."),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_defaults_workspace_mode_to_staged() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::Staged));
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_defaults_lane_to_patch_first() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::PatchFirst)),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_defaults_include_exclude_empty() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run {
            include, exclude, ..
        } => {
            assert!(include.is_empty());
            assert!(exclude.is_empty());
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_defaults_params_empty() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run { params, .. } => assert!(params.is_empty()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_defaults_env_vars_empty() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match &cli.command {
        Commands::Run { env_vars, .. } => assert!(env_vars.is_empty()),
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Conversion impls — WorkspaceModeArg / LaneArg → core types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_mode_pass_through_converts() {
    let mode: abp_core::WorkspaceMode = WorkspaceModeArg::PassThrough.into();
    assert!(matches!(mode, abp_core::WorkspaceMode::PassThrough));
}

#[test]
fn workspace_mode_staged_converts() {
    let mode: abp_core::WorkspaceMode = WorkspaceModeArg::Staged.into();
    assert!(matches!(mode, abp_core::WorkspaceMode::Staged));
}

#[test]
fn lane_patch_first_converts() {
    let lane: abp_core::ExecutionLane = LaneArg::PatchFirst.into();
    assert!(matches!(lane, abp_core::ExecutionLane::PatchFirst));
}

#[test]
fn lane_workspace_first_converts() {
    let lane: abp_core::ExecutionLane = LaneArg::WorkspaceFirst.into();
    assert!(matches!(lane, abp_core::ExecutionLane::WorkspaceFirst));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Edge cases — unusual task strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn task_empty_string() {
    let cli = parse(&["run", "--task", ""]).unwrap();
    match &cli.command {
        Commands::Run { task, .. } => assert!(task.is_empty()),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn task_with_unicode_and_emoji() {
    let cli = parse(&["run", "--task", "修复 🐛 bug"]).unwrap();
    match &cli.command {
        Commands::Run { task, .. } => assert_eq!(task, "修复 🐛 bug"),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn task_with_newlines_preserved() {
    let cli = parse(&["run", "--task", "line1\nline2"]).unwrap();
    match &cli.command {
        Commands::Run { task, .. } => assert!(task.contains('\n')),
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn task_very_long_string() {
    let long = "a".repeat(50_000);
    let cli = parse(&["run", "--task", &long]).unwrap();
    match &cli.command {
        Commands::Run { task, .. } => assert_eq!(task.len(), 50_000),
        other => panic!("expected Run, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Multiple repeated flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multiple_include_globs() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--include",
        "a/**",
        "--include",
        "b/**",
        "--include",
        "c.rs",
    ])
    .unwrap();
    match &cli.command {
        Commands::Run { include, .. } => {
            assert_eq!(include.len(), 3);
            assert_eq!(include[0], "a/**");
            assert_eq!(include[1], "b/**");
            assert_eq!(include[2], "c.rs");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn multiple_exclude_globs() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--exclude",
        "target/**",
        "--exclude",
        "*.bak",
    ])
    .unwrap();
    match &cli.command {
        Commands::Run { exclude, .. } => {
            assert_eq!(exclude.len(), 2);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn multiple_params_parsed() {
    let cli = parse(&[
        "run", "--task", "t", "--param", "a=1", "--param", "b=2", "--param", "c=3",
    ])
    .unwrap();
    match &cli.command {
        Commands::Run { params, .. } => {
            assert_eq!(params.len(), 3);
            assert_eq!(params[0], "a=1");
            assert_eq!(params[1], "b=2");
            assert_eq!(params[2], "c=3");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn multiple_env_vars_parsed() {
    let cli = parse(&["run", "--task", "t", "--env", "A=1", "--env", "B=2"]).unwrap();
    match &cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars.len(), 2);
            assert_eq!(env_vars[0], "A=1");
            assert_eq!(env_vars[1], "B=2");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn env_var_with_equals_in_value_preserved() {
    let cli = parse(&["run", "--task", "t", "--env", "CMD=echo a=b"]).unwrap();
    match &cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars, &["CMD=echo a=b"]);
        }
        other => panic!("expected Run, got {other:?}"),
    }
}
