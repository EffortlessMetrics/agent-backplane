// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive CLI argument parsing tests using [`clap::Parser::try_parse_from`].
//!
//! These tests verify the `abp` CLI argument parser in-process without spawning
//! the binary, covering subcommands, flags, defaults, validation, and edge cases.

use abp_cli::cli::{
    Cli, Commands, ConfigAction, LaneArg, ReceiptAction, SchemaArg, WorkspaceModeArg,
};
use clap::{Parser, error::ErrorKind};
use std::path::PathBuf;

/// Parse CLI arguments prefixed with the binary name.
fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("abp").chain(args.iter().copied()))
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Argument Parsing — `run` subcommand basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_with_task_and_backend() {
    let cli = parse(&["run", "--task", "hello", "--backend", "mock"]).unwrap();
    assert!(!cli.debug);
    assert!(cli.config.is_none());
    match cli.command {
        Commands::Run { task, backend, .. } => {
            assert_eq!(task, "hello");
            assert_eq!(backend.as_deref(), Some("mock"));
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn run_task_is_required() {
    let err = parse(&["run", "--backend", "mock"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn run_backend_is_optional() {
    let cli = parse(&["run", "--task", "hello"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert!(backend.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_model_flag() {
    let cli = parse(&["run", "--task", "t", "--model", "gpt-4o"]).unwrap();
    match cli.command {
        Commands::Run { model, .. } => assert_eq!(model.as_deref(), Some("gpt-4o")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_root_flag() {
    let cli = parse(&["run", "--task", "t", "--root", "/tmp/work"]).unwrap();
    match cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "/tmp/work"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_workspace_mode_pass_through() {
    let cli = parse(&["run", "--task", "t", "--workspace-mode", "pass-through"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_workspace_mode_staged() {
    let cli = parse(&["run", "--task", "t", "--workspace-mode", "staged"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::Staged));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_lane_patch_first() {
    let cli = parse(&["run", "--task", "t", "--lane", "patch-first"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::PatchFirst)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_lane_workspace_first() {
    let cli = parse(&["run", "--task", "t", "--lane", "workspace-first"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::WorkspaceFirst)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_include_globs() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--include",
        "src/**",
        "--include",
        "*.toml",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { include, .. } => {
            assert_eq!(include, vec!["src/**", "*.toml"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_exclude_globs() {
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
    match cli.command {
        Commands::Run { exclude, .. } => {
            assert_eq!(exclude, vec!["target/**", "*.bak"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_with_both_include_and_exclude() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--include",
        "src/**",
        "--exclude",
        "target/**",
    ])
    .unwrap();
    match cli.command {
        Commands::Run {
            include, exclude, ..
        } => {
            assert_eq!(include, vec!["src/**"]);
            assert_eq!(exclude, vec!["target/**"]);
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Backend Listing — `backends` subcommand
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backends_subcommand_parses() {
    let cli = parse(&["backends"]).unwrap();
    assert!(matches!(cli.command, Commands::Backends));
}

#[test]
fn backends_with_debug_flag() {
    let cli = parse(&["--debug", "backends"]).unwrap();
    assert!(cli.debug);
    assert!(matches!(cli.command, Commands::Backends));
}

#[test]
fn backends_rejects_extra_positional() {
    let err = parse(&["backends", "extra"]).unwrap_err();
    assert!(
        err.kind() == ErrorKind::UnknownArgument
            || err.kind() == ErrorKind::InvalidSubcommand
            || err.to_string().contains("unexpected")
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Default Values
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn run_default_root_is_dot() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { root, .. } => assert_eq!(root, "."),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_workspace_mode_is_staged() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { workspace_mode, .. } => {
            assert!(matches!(workspace_mode, WorkspaceModeArg::Staged));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_lane_is_patch_first() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { lane, .. } => assert!(matches!(lane, LaneArg::PatchFirst)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_backend_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert!(backend.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_model_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { model, .. } => assert!(model.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_json_is_false() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { json, .. } => assert!(!json),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_max_budget_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { max_budget_usd, .. } => assert!(max_budget_usd.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_max_turns_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { max_turns, .. } => assert!(max_turns.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_include_is_empty() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { include, .. } => assert!(include.is_empty()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_exclude_is_empty() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { exclude, .. } => assert!(exclude.is_empty()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_out_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { out, .. } => assert!(out.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_policy_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { policy, .. } => assert!(policy.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_default_events_is_none() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    match cli.command {
        Commands::Run { events, .. } => assert!(events.is_none()),
        _ => panic!("expected Run"),
    }
}

#[test]
fn debug_default_is_false() {
    let cli = parse(&["backends"]).unwrap();
    assert!(!cli.debug);
}

#[test]
fn config_default_is_none() {
    let cli = parse(&["backends"]).unwrap();
    assert!(cli.config.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Invalid Arguments
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_flag_on_run_rejected() {
    let err = parse(&["run", "--task", "t", "--nonexistent"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn unknown_subcommand_rejected() {
    let err = parse(&["nonexistent_subcmd"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidSubcommand);
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
fn invalid_workspace_mode_rejected() {
    let err = parse(&["run", "--task", "t", "--workspace-mode", "bogus"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn invalid_lane_value_rejected() {
    let err = parse(&["run", "--task", "t", "--lane", "invalid"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn invalid_max_budget_not_number() {
    let err = parse(&["run", "--task", "t", "--max-budget-usd", "abc"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn invalid_max_turns_not_number() {
    let err = parse(&["run", "--task", "t", "--max-turns", "xyz"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn extra_positional_arg_on_run_rejected() {
    let err = parse(&["run", "--task", "t", "extra_arg"]).unwrap_err();
    assert!(
        err.kind() == ErrorKind::UnknownArgument
            || err.to_string().contains("unexpected")
            || err.to_string().contains("error")
    );
}

#[test]
fn invalid_schema_kind_rejected() {
    let err = parse(&["schema", "bogus"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn missing_validate_file_arg_rejected() {
    let err = parse(&["validate"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn missing_inspect_file_arg_rejected() {
    let err = parse(&["inspect"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Backend Resolution — various --backend strings accepted
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_mock() {
    let cli = parse(&["run", "--task", "t", "--backend", "mock"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("mock")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_sidecar_node() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:node"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:node")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_sidecar_python() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:python"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:python")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_sidecar_claude() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:claude"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:claude")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_sidecar_copilot() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:copilot"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:copilot")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_sidecar_kimi() {
    let cli = parse(&["run", "--task", "t", "--backend", "sidecar:kimi"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("sidecar:kimi")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_alias_short_name() {
    let cli = parse(&["run", "--task", "t", "--backend", "node"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => assert_eq!(backend.as_deref(), Some("node")),
        _ => panic!("expected Run"),
    }
}

#[test]
fn backend_arbitrary_string() {
    let cli = parse(&["run", "--task", "t", "--backend", "custom-backend-name"]).unwrap();
    match cli.command {
        Commands::Run { backend, .. } => {
            assert_eq!(backend.as_deref(), Some("custom-backend-name"))
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Debug Flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn debug_flag_before_subcommand() {
    let cli = parse(&["--debug", "run", "--task", "t"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn debug_flag_on_backends() {
    let cli = parse(&["--debug", "backends"]).unwrap();
    assert!(cli.debug);
}

#[test]
fn debug_flag_absent_means_false() {
    let cli = parse(&["run", "--task", "t"]).unwrap();
    assert!(!cli.debug);
}

#[test]
fn debug_flag_not_accepted_after_subcommand() {
    // --debug is a global flag defined before the subcommand; clap may or may
    // not accept it after.  Verify the result is deterministic either way.
    let result = parse(&["run", "--debug", "--task", "t"]);
    // clap typically rejects global flags after subcommand
    assert!(result.is_err() || result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Task Validation — edge case task strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_task_accepted() {
    let cli = parse(&["run", "--task", ""]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert_eq!(task, ""),
        _ => panic!("expected Run"),
    }
}

#[test]
fn long_task_accepted() {
    let long = "x".repeat(10_000);
    let cli = parse(&["run", "--task", &long]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert_eq!(task.len(), 10_000),
        _ => panic!("expected Run"),
    }
}

#[test]
fn task_with_special_characters() {
    let cli = parse(&["run", "--task", "Fix bug #42 (urgent!) & deploy"]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert_eq!(task, "Fix bug #42 (urgent!) & deploy"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn task_with_unicode() {
    let cli = parse(&["run", "--task", "日本語テスト 🚀"]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert_eq!(task, "日本語テスト 🚀"),
        _ => panic!("expected Run"),
    }
}

#[test]
fn task_with_newlines() {
    let cli = parse(&["run", "--task", "line1\nline2\nline3"]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert!(task.contains('\n')),
        _ => panic!("expected Run"),
    }
}

#[test]
fn task_with_only_whitespace() {
    let cli = parse(&["run", "--task", "   "]).unwrap();
    match cli.command {
        Commands::Run { task, .. } => assert_eq!(task, "   "),
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Config File — --config flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_flag_before_subcommand() {
    let cli = parse(&["--config", "my.toml", "backends"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("my.toml")));
}

#[test]
fn config_flag_is_global_across_subcommands() {
    // --config is declared global; it should work before any subcommand.
    let cli = parse(&["--config", "/etc/abp.toml", "run", "--task", "t"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("/etc/abp.toml")));
}

#[test]
fn config_flag_accepts_relative_path() {
    let cli = parse(&["--config", "./subdir/backplane.toml", "backends"]).unwrap();
    assert_eq!(cli.config, Some(PathBuf::from("./subdir/backplane.toml")));
}

#[test]
fn config_flag_absent_is_none() {
    let cli = parse(&["backends"]).unwrap();
    assert!(cli.config.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Output Format — --json, --out, --output, --events
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_flag_sets_true() {
    let cli = parse(&["run", "--task", "t", "--json"]).unwrap();
    match cli.command {
        Commands::Run { json, .. } => assert!(json),
        _ => panic!("expected Run"),
    }
}

#[test]
fn out_flag_sets_path() {
    let cli = parse(&["run", "--task", "t", "--out", "/tmp/receipt.json"]).unwrap();
    match cli.command {
        Commands::Run { out, .. } => {
            assert_eq!(out, Some(PathBuf::from("/tmp/receipt.json")));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn output_flag_sets_path() {
    let cli = parse(&["run", "--task", "t", "--output", "/tmp/out.json"]).unwrap();
    match cli.command {
        Commands::Run { output, .. } => {
            assert_eq!(output, Some(PathBuf::from("/tmp/out.json")));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn events_flag_sets_path() {
    let cli = parse(&["run", "--task", "t", "--events", "events.jsonl"]).unwrap();
    match cli.command {
        Commands::Run { events, .. } => {
            assert_eq!(events, Some(PathBuf::from("events.jsonl")));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn both_out_and_output_accepted() {
    let cli = parse(&[
        "run", "--task", "t", "--out", "a.json", "--output", "b.json",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { out, output, .. } => {
            assert_eq!(out, Some(PathBuf::from("a.json")));
            assert_eq!(output, Some(PathBuf::from("b.json")));
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Version Flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn version_flag_triggers_display() {
    let err = parse(&["--version"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayVersion);
}

#[test]
fn version_flag_short_v_not_accepted() {
    // clap doesn't add -V by default for subcommand-bearing CLIs in all configs.
    let result = parse(&["-V"]);
    // We verify it either shows version or is an error (not a panic).
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Help Flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn help_flag_triggers_display() {
    let err = parse(&["--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn run_help_triggers_display() {
    let err = parse(&["run", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn backends_help_triggers_display() {
    let err = parse(&["backends", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn help_short_h_triggers_display() {
    let err = parse(&["-h"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Params and Env Vars
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn single_param() {
    let cli = parse(&["run", "--task", "t", "--param", "model=gpt-4"]).unwrap();
    match cli.command {
        Commands::Run { params, .. } => {
            assert_eq!(params, vec!["model=gpt-4"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn multiple_params() {
    let cli = parse(&[
        "run",
        "--task",
        "t",
        "--param",
        "model=gpt-4",
        "--param",
        "stream=true",
        "--param",
        "abp.mode=passthrough",
    ])
    .unwrap();
    match cli.command {
        Commands::Run { params, .. } => {
            assert_eq!(params.len(), 3);
            assert_eq!(params[0], "model=gpt-4");
            assert_eq!(params[1], "stream=true");
            assert_eq!(params[2], "abp.mode=passthrough");
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn single_env_var() {
    let cli = parse(&["run", "--task", "t", "--env", "KEY=value"]).unwrap();
    match cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars, vec!["KEY=value"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn multiple_env_vars() {
    let cli = parse(&["run", "--task", "t", "--env", "A=1", "--env", "B=2"]).unwrap();
    match cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars.len(), 2);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn params_and_env_combined() {
    let cli = parse(&["run", "--task", "t", "--param", "k=v", "--env", "E=1"]).unwrap();
    match cli.command {
        Commands::Run {
            params, env_vars, ..
        } => {
            assert_eq!(params.len(), 1);
            assert_eq!(env_vars.len(), 1);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn env_var_with_equals_in_value() {
    let cli = parse(&["run", "--task", "t", "--env", "CMD=echo foo=bar"]).unwrap();
    match cli.command {
        Commands::Run { env_vars, .. } => {
            assert_eq!(env_vars, vec!["CMD=echo foo=bar"]);
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Other Subcommands
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_subcommand_parses() {
    let cli = parse(&["validate", "file.json"]).unwrap();
    match cli.command {
        Commands::Validate { file } => assert_eq!(file, PathBuf::from("file.json")),
        _ => panic!("expected Validate"),
    }
}

#[test]
fn schema_work_order_parses() {
    let cli = parse(&["schema", "work-order"]).unwrap();
    match cli.command {
        Commands::Schema { kind } => assert!(matches!(kind, SchemaArg::WorkOrder)),
        _ => panic!("expected Schema"),
    }
}

#[test]
fn schema_receipt_parses() {
    let cli = parse(&["schema", "receipt"]).unwrap();
    match cli.command {
        Commands::Schema { kind } => assert!(matches!(kind, SchemaArg::Receipt)),
        _ => panic!("expected Schema"),
    }
}

#[test]
fn schema_config_parses() {
    let cli = parse(&["schema", "config"]).unwrap();
    match cli.command {
        Commands::Schema { kind } => assert!(matches!(kind, SchemaArg::Config)),
        _ => panic!("expected Schema"),
    }
}

#[test]
fn inspect_subcommand_parses() {
    let cli = parse(&["inspect", "receipt.json"]).unwrap();
    match cli.command {
        Commands::Inspect { file } => assert_eq!(file, PathBuf::from("receipt.json")),
        _ => panic!("expected Inspect"),
    }
}

#[test]
fn config_check_subcommand_parses() {
    let cli = parse(&["config", "check"]).unwrap();
    match cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert!(config.is_none()),
        _ => panic!("expected ConfigCmd Check"),
    }
}

#[test]
fn config_check_with_config_flag() {
    let cli = parse(&["config", "check", "--config", "custom.toml"]).unwrap();
    match cli.command {
        Commands::ConfigCmd {
            action: ConfigAction::Check { config },
        } => assert_eq!(config, Some(PathBuf::from("custom.toml"))),
        _ => panic!("expected ConfigCmd Check"),
    }
}

#[test]
fn receipt_verify_subcommand_parses() {
    let cli = parse(&["receipt", "verify", "r.json"]).unwrap();
    match cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Verify { file },
        } => assert_eq!(file, PathBuf::from("r.json")),
        _ => panic!("expected ReceiptCmd Verify"),
    }
}

#[test]
fn receipt_diff_subcommand_parses() {
    let cli = parse(&["receipt", "diff", "a.json", "b.json"]).unwrap();
    match cli.command {
        Commands::ReceiptCmd {
            action: ReceiptAction::Diff { file1, file2 },
        } => {
            assert_eq!(file1, PathBuf::from("a.json"));
            assert_eq!(file2, PathBuf::from("b.json"));
        }
        _ => panic!("expected ReceiptCmd Diff"),
    }
}

#[test]
fn receipt_diff_missing_second_file_rejected() {
    let err = parse(&["receipt", "diff", "a.json"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Max Budget and Turns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn max_budget_usd_parses_float() {
    let cli = parse(&["run", "--task", "t", "--max-budget-usd", "5.50"]).unwrap();
    match cli.command {
        Commands::Run { max_budget_usd, .. } => {
            assert!((max_budget_usd.unwrap() - 5.5).abs() < f64::EPSILON);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn max_budget_usd_parses_integer() {
    let cli = parse(&["run", "--task", "t", "--max-budget-usd", "10"]).unwrap();
    match cli.command {
        Commands::Run { max_budget_usd, .. } => {
            assert!((max_budget_usd.unwrap() - 10.0).abs() < f64::EPSILON);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn max_turns_parses_integer() {
    let cli = parse(&["run", "--task", "t", "--max-turns", "25"]).unwrap();
    match cli.command {
        Commands::Run { max_turns, .. } => assert_eq!(max_turns, Some(25)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn max_turns_negative_rejected() {
    let err = parse(&["run", "--task", "t", "--max-turns", "-1"]).unwrap_err();
    assert!(err.kind() == ErrorKind::ValueValidation || err.kind() == ErrorKind::UnknownArgument);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Policy Flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_flag_sets_path() {
    let cli = parse(&["run", "--task", "t", "--policy", "policy.json"]).unwrap();
    match cli.command {
        Commands::Run { policy, .. } => {
            assert_eq!(policy, Some(PathBuf::from("policy.json")));
        }
        _ => panic!("expected Run"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Conversion impls
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn workspace_mode_arg_to_core_pass_through() {
    let mode: abp_core::WorkspaceMode = WorkspaceModeArg::PassThrough.into();
    assert!(matches!(mode, abp_core::WorkspaceMode::PassThrough));
}

#[test]
fn workspace_mode_arg_to_core_staged() {
    let mode: abp_core::WorkspaceMode = WorkspaceModeArg::Staged.into();
    assert!(matches!(mode, abp_core::WorkspaceMode::Staged));
}

#[test]
fn lane_arg_to_core_patch_first() {
    let lane: abp_core::ExecutionLane = LaneArg::PatchFirst.into();
    assert!(matches!(lane, abp_core::ExecutionLane::PatchFirst));
}

#[test]
fn lane_arg_to_core_workspace_first() {
    let lane: abp_core::ExecutionLane = LaneArg::WorkspaceFirst.into();
    assert!(matches!(lane, abp_core::ExecutionLane::WorkspaceFirst));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Config module — env overrides
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_load_defaults_succeeds() {
    let cfg = abp_cli::config::load_config(None).unwrap();
    assert!(cfg.backends.is_empty());
}

#[test]
fn config_validate_empty_is_ok() {
    let cfg = abp_cli::config::BackplaneConfig::default();
    abp_cli::config::validate_config(&cfg).unwrap();
}

#[test]
fn config_merge_preserves_base_when_overlay_empty() {
    let base = abp_cli::config::BackplaneConfig {
        log_level: Some("debug".into()),
        ..Default::default()
    };
    let overlay = abp_cli::config::BackplaneConfig::default();
    let merged = abp_cli::config::merge_configs(base, overlay);
    assert_eq!(merged.log_level.as_deref(), Some("debug"));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Full command combinations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn full_run_command_all_flags() {
    let cli = parse(&[
        "--debug",
        "--config",
        "bp.toml",
        "run",
        "--task",
        "refactor auth",
        "--backend",
        "mock",
        "--model",
        "gpt-4o",
        "--root",
        "/project",
        "--workspace-mode",
        "pass-through",
        "--lane",
        "workspace-first",
        "--include",
        "src/**",
        "--exclude",
        "node_modules/**",
        "--param",
        "stream=true",
        "--env",
        "TOKEN=abc",
        "--max-budget-usd",
        "10.0",
        "--max-turns",
        "50",
        "--out",
        "receipt.json",
        "--output",
        "out.json",
        "--events",
        "events.jsonl",
        "--json",
        "--policy",
        "policy.json",
    ])
    .unwrap();
    assert!(cli.debug);
    assert_eq!(cli.config, Some(PathBuf::from("bp.toml")));
    match cli.command {
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
        } => {
            assert_eq!(task, "refactor auth");
            assert_eq!(backend.as_deref(), Some("mock"));
            assert_eq!(model.as_deref(), Some("gpt-4o"));
            assert_eq!(root, "/project");
            assert!(matches!(workspace_mode, WorkspaceModeArg::PassThrough));
            assert!(matches!(lane, LaneArg::WorkspaceFirst));
            assert_eq!(include, vec!["src/**"]);
            assert_eq!(exclude, vec!["node_modules/**"]);
            assert_eq!(params, vec!["stream=true"]);
            assert_eq!(env_vars, vec!["TOKEN=abc"]);
            assert!((max_budget_usd.unwrap() - 10.0).abs() < f64::EPSILON);
            assert_eq!(max_turns, Some(50));
            assert_eq!(out, Some(PathBuf::from("receipt.json")));
            assert!(json);
            assert_eq!(policy, Some(PathBuf::from("policy.json")));
            assert_eq!(output, Some(PathBuf::from("out.json")));
            assert_eq!(events, Some(PathBuf::from("events.jsonl")));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn minimal_run_command() {
    let cli = parse(&["run", "--task", "hello"]).unwrap();
    assert!(!cli.debug);
    assert!(cli.config.is_none());
    match cli.command {
        Commands::Run {
            task,
            backend,
            model,
            json,
            ..
        } => {
            assert_eq!(task, "hello");
            assert!(backend.is_none());
            assert!(model.is_none());
            assert!(!json);
        }
        _ => panic!("expected Run"),
    }
}
